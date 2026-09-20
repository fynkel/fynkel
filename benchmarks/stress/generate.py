#!/usr/bin/env python3
"""The stress scene (blueprint §15.3, §19.2d item 6, ADR-0011): thousands of
objects, tracked per milestone against the benchmark manifest.

Writes an ordinary Fynkel project to benchmarks/stress/project/ (ignored by
git — the generator is the source):

  5 000 static meshes    "Static NNNN": cubes, pyramids and quads from the
                          showcase's cube.glb and robot.glb over four
                          materials — 12 distinct (mesh, material) pairs, so
                          the renderer's draw count has a ceiling to check
  500 rigid bodies       "Body NNN": cubes with the `body` material, each a
                          dynamic RigidBody on a box Collider, dropped from
                          3–7 m onto the field (item 8: the physics stress)
  300 behaviours         "Behaviour NNN": pyramids with the `behaviour`
                          material, static until item 10 attaches the 300
                          ticking Dart behaviours (and the native movers)
  floor (a box Collider: the ground the bodies land on), sun (casting),
  Environment, one perspective camera over the field. The static meshes
  carry box colliders too (5 500 colliders in the broad phase), so the
  bodies come to rest on them as well as on the floor.

The camera sits above the near edge looking across the field, so the far
corners and the strip behind it fall outside the frustum: `culled` is never
zero, and `instances` is the number that reaches the scene passes.

The field is deterministic: --seed picks the layout, the ids are v5 UUIDs of
the entity's name in the project's namespace, so a regenerated project cooks
to the same cache keys. Run it, then `fynkel project lock`, `fynkel cook`:
benchmarks/m3/capture.py does all of that.

With --app the project is a runnable game as well: `fynkel create` writes
the starter app around it (runners, `lib/main.dart` with the HUD — fps,
draws, culled, shadow map, GPU/CPU ms — `game.dart`, the Spinner behaviour
and its generated files; the starter's own scene, cube, material, swatch
and test are dropped, since the stress scene is the startup scene) and
the showcase's `game.Orbit` circles the camera around the field (its
start pose is the bare camera's, 0.08 rad/s), so it runs on this Mac and
on a phone: `fynkel run --project benchmarks/stress/project -d <iphone>`
(the cook stages the cache into the bundle; Q cycles the quality
profiles). The bare project (the default) is what CI and capture.py cook
and measure; the ids and the field are the same either way, the app's
scene differs by the one Orbit component on the camera. `fynkel create` needs `flutter` on PATH and
the CLI at $FYNKEL_CLI or target/debug/fynkel.

Usage: python3 benchmarks/stress/generate.py [--out DIR] [--static 5000]
       [--bodies 500] [--behaviours 300] [--seed 1] [--app]
"""
import argparse
import json
import math
import os
import random
import shutil
import subprocess
import sys
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SHOWCASE = ROOT / "examples/showcase"
PROJECT_ID = uuid.UUID("5b6a1c2e-0000-4000-8000-00000000c0de")
SCENE_ID = uuid.UUID("5b6a1c2e-0000-4000-8000-0000000005ce")
CUBE_GLB = "f74c43aa-7e0b-4a48-a124-ef002a901e34"   # showcase cube.glb (mesh/0)
ROBOT_GLB = "0b89f746-0ebe-4a88-b91f-e632b7c7e9e1"  # showcase robot.glb (mesh/0 cube, mesh/1/0 pyramid, mesh/1/1 quad)

MESHES = {
    "cube": {"asset": CUBE_GLB, "sub": "mesh/0"},
    "pyramid": {"asset": ROBOT_GLB, "sub": "mesh/1/0"},
    "quad": {"asset": ROBOT_GLB, "sub": "mesh/1/1"},
}

# name → (base colour, metallic, roughness, extra fields)
MATERIALS = {
    "concrete": ([0.6, 0.58, 0.55, 1.0], 0.0, 0.9, {}),
    "brick": ([0.55, 0.25, 0.18, 1.0], 0.0, 0.8, {}),
    "steel": ([0.7, 0.72, 0.75, 1.0], 1.0, 0.35, {}),
    "moss": ([0.25, 0.45, 0.2, 1.0], 0.0, 0.7, {"double_sided": True}),
    "body": ([0.9, 0.35, 0.1, 1.0], 0.0, 0.5, {}),
    "behaviour": ([0.2, 0.5, 0.95, 1.0], 0.0, 0.4, {}),
    "floor": ([0.45, 0.45, 0.47, 1.0], 0.0, 0.95, {}),
}

STATIC_PAIRS = [(m, mat) for m in ("cube", "pyramid", "quad") for mat in ("concrete", "brick", "steel", "moss")]

FIELD_W, FIELD_D = 120.0, 60.0


def uid(name):
    return str(uuid.uuid5(PROJECT_ID, name))


def quat_axis_angle(axis, angle):
    x, y, z = axis
    s = math.sin(angle / 2)
    return [x * s, y * s, z * s, math.cos(angle / 2)]


def quat_mul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]


def r6(v):
    return [round(x, 6) for x in v]


def transform(position, rotation=(0, 0, 0, 1), scale=(1, 1, 1)):
    return {"type": "fynkel.Transform", "version": 1, "data": {"position": r6(position), "rotation": r6(rotation), "scale": r6(scale)}}


def mesh_renderer(mesh, material, cast=True, receive=True):
    return {"type": "fynkel.MeshRenderer", "version": 1, "data": {
        "mesh": MESHES[mesh], "material": {"asset": uid(f"material/{material}")},
        "cast_shadows": cast, "receive_shadows": receive, "visible": True}}


def collider(size=(0.5, 0.5, 0.5), friction=0.6, layer=1):
    return {"type": "fynkel.Collider", "version": 1, "data": {
        "shape": "box", "size": list(size), "mesh": None, "offset": [0.0, 0.0, 0.0],
        "friction": friction, "restitution": 0.05, "is_sensor": False, "layer": layer, "mask": 4294967295}}


def rigid_body(mass):
    return {"type": "fynkel.RigidBody", "version": 1, "data": {
        "body_type": "dynamic", "mass": round(mass, 3), "linear_damping": 0.0, "angular_damping": 0.05,
        "gravity_scale": 1.0, "ccd": False}}


def entity(name, components):
    return uid(f"entity/{name}"), {"name": name, "parent": None, "children": [],
                                   "components": {uid(f"component/{name}/{c['type']}"): c for c in components}}


def material_doc(name):
    color, metallic, roughness, extra = MATERIALS[name]
    return {"$schema": "https://fynkel.dev/schemas/material.schema.json", "formatVersion": 2,
            "id": uid(f"material/{name}"), "name": name.capitalize(), "shader": "fynkel.pbr@1",
            "base_color": color, "metallic": metallic, "roughness": roughness, **extra}


def project_doc():
    native = ["fynkel.Transform", "fynkel.MeshRenderer", "fynkel.Camera", "fynkel.DirectionalLight",
              "fynkel.PointLight", "fynkel.SpotLight", "fynkel.Environment", "fynkel.RigidBody", "fynkel.Collider"]
    return {
        "$schema": "https://fynkel.dev/schemas/project.schema.json", "formatVersion": 1,
        "id": str(PROJECT_ID), "name": "Fynkel Stress", "startupScene": {"asset": str(SCENE_ID)},
        "assets": "assets",
        "inputActions": {"move": {"type": "axis2d", "bindings": ["keyboard:WASD", "gamepad:leftStick", "touch:overlay/stick"]}},
        "physics": {"gravity": [0.0, -9.81, 0.0], "layers": ["default", "bodies"]},
        "quality": {"default": "medium", "profiles": {
            "low": {"shadows": False, "msaa": 1, "renderScale": 0.75, "anisotropy": 1},
            "medium": {"shadowResolution": 2048, "msaa": 4},
            "high": {"shadowResolution": 4096, "msaa": 4, "anisotropy": 16, "renderScale": 1.5}}},
        "components": {c: {"kind": "native"} for c in native},
        "targets": {"macos": {"enabled": True, "profile": "dev"}, "ios": {"enabled": False}, "android": {"enabled": False}},
    }


def scene_doc(n_static, n_bodies, n_behaviours, seed, orbit=False):
    rng = random.Random(seed)
    entities = {}

    def add(name, components):
        k, v = entity(name, components)
        entities[k] = v

    # Camera above the near edge (+Z), looking across the field and down.
    # With --app the showcase's game.Orbit circles it around the field's
    # centre; its start pose (phase 0) is this exact transform, so the
    # app's first frame and the bare project's render agree.
    eye = (0.0, 26.0, FIELD_D / 2 + 30.0)
    pitch = -math.atan2(eye[1], eye[2] - 0.0)
    camera = [transform(eye, quat_axis_angle((1, 0, 0), pitch)),
              {"type": "fynkel.Camera", "version": 1, "data": {
                  "projection": "perspective", "fov_y": 1.0, "ortho_height": 10.0, "near": 0.1, "far": 200.0,
                  "priority": 0, "exposure_mode": "auto", "exposure": 12.0}}]
    if orbit:
        camera.append({"type": "game.Orbit", "version": 1, "data": {
            "target": [0.0, 0.0, 0.0], "radius": eye[2], "height": eye[1], "speed": 0.08, "phase": 0.0}})
    add("Camera", camera)
    # A morning sun from +X, 40° up.
    sun_rot = quat_mul(quat_axis_angle((0, 1, 0), math.radians(-60)), quat_axis_angle((1, 0, 0), math.radians(-40)))
    add("Sun", [transform((0, 10, 0), sun_rot),
                {"type": "fynkel.DirectionalLight", "version": 1, "data": {"color": [1.0, 0.95, 0.85], "intensity": 10000.0, "cast_shadows": True}}])
    add("Environment", [{"type": "fynkel.Environment", "version": 1, "data": {
        "ambient_color": [0.55, 0.65, 0.9], "ambient_intensity": 2500.0, "sky": None, "sky_intensity": 0.0, "sky_rotation": 0.0}}])
    add("Floor", [transform((0, -0.1, 0), scale=(FIELD_W + 20, 0.2, FIELD_D + 20)), mesh_renderer("cube", "floor", cast=False),
                  collider()])

    # A jittered grid over the field for everything that stands on it.
    total = n_static + n_bodies + n_behaviours
    cols = int(math.ceil(math.sqrt(total * FIELD_W / FIELD_D)))
    rows = int(math.ceil(total / cols))
    cells = [(c, r) for r in range(rows) for c in range(cols)]
    rng.shuffle(cells)
    dx, dz = FIELD_W / cols, FIELD_D / rows

    def place(i):
        c, r = cells[i]
        x = -FIELD_W / 2 + (c + 0.5) * dx + rng.uniform(-0.25, 0.25) * dx
        z = -FIELD_D / 2 + (r + 0.5) * dz + rng.uniform(-0.25, 0.25) * dz
        return x, z

    i = 0
    for n in range(n_static):
        mesh, mat = STATIC_PAIRS[n % len(STATIC_PAIRS)]
        x, z = place(i); i += 1
        s = rng.uniform(0.4, 1.1)
        yaw = quat_axis_angle((0, 1, 0), rng.uniform(0, math.tau))
        y = s * 0.5
        add(f"Static {n:04}", [transform((x, y, z), yaw, (s, s, s)), mesh_renderer(mesh, mat), collider()])
    for n in range(n_bodies):
        x, z = place(i); i += 1
        s = rng.uniform(0.5, 0.9)
        add(f"Body {n:03}", [transform((x, 3.0 + rng.uniform(0, 4), z), quat_axis_angle((0, 1, 0), rng.uniform(0, math.tau)), (s, s, s)),
                              mesh_renderer("cube", "body"), rigid_body(mass=s ** 3 * 2.0), collider(friction=0.5, layer=2)])
    for n in range(n_behaviours):
        x, z = place(i); i += 1
        add(f"Behaviour {n:03}", [transform((x, 0.5, z), quat_axis_angle((0, 1, 0), rng.uniform(0, math.tau)), (0.8, 0.8, 0.8)),
                                   mesh_renderer("pyramid", "behaviour")])
    return {"$schema": "https://fynkel.dev/schemas/scene.schema.json", "formatVersion": 2,
            "id": str(SCENE_ID), "name": "Stress", "entities": entities}


def create_app(out):
    """`fynkel create` the starter app into `out` (once: a pubspec marks it
    done), then drop the starter's own content the stress scene replaces."""
    if not (out / "pubspec.yaml").is_file():
        cli = os.environ.get("FYNKEL_CLI") or str(ROOT / "target/debug/fynkel")
        if out.exists():
            shutil.rmtree(out)
        cmd = [cli, "create", str(out), "--name", "fynkel_stress", "--title", "Fynkel Stress", "--root", str(ROOT)]
        print(" ".join(cmd), flush=True)
        subprocess.run(cmd, check=True)
    for rel in ("assets/scenes/main.scene.json", "assets/materials/starter.material.json", "assets/models/cube.glb",
                "assets/models/cube.glb.meta.json", "assets/textures/swatch.png", "assets/textures/swatch.png.meta.json",
                "test/game_test.dart"):
        (out / rel).unlink(missing_ok=True)
    # The showcase's orbiting camera; `fynkel codegen` (below) generates
    # its schema and registers it.
    shutil.copyfile(SHOWCASE / "lib/game/orbit.dart", out / "lib/game/orbit.dart")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", type=Path, default=ROOT / "benchmarks/stress/project")
    ap.add_argument("--static", type=int, default=5000)
    ap.add_argument("--bodies", type=int, default=500)
    ap.add_argument("--behaviours", type=int, default=300)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--app", action="store_true", help="wrap the project in the `fynkel create` starter app so it runs on a device")
    a = ap.parse_args()
    out = a.out
    if a.app:
        create_app(out)
    for d in ("assets/models", "assets/materials", "assets/scenes", "assets/prefabs", "assets/textures", "assets/audio"):
        (out / d).mkdir(parents=True, exist_ok=True)
    for f in ("cube.glb", "cube.glb.meta.json", "robot.glb", "robot.glb.meta.json"):
        shutil.copyfile(SHOWCASE / "assets/models" / f, out / "assets/models" / f)
    for name in MATERIALS:
        (out / f"assets/materials/{name}.material.json").write_text(json.dumps(material_doc(name), indent=2) + "\n")
    project = project_doc()
    if a.app:
        # The starter's project file keeps its registry (game.Spinner stays
        # registered, no scene uses it), input actions and targets; the
        # stress scene, ids, name and quality profiles are ours.
        created = json.loads((out / "fynkel.project.json").read_text())
        for key in ("id", "name", "startupScene", "quality"):
            created[key] = project[key]
        created["components"].update(project["components"])
        created["components"]["game.Orbit"] = {"kind": "behaviour", "version": 1,
                                               "description": "Circles the camera around the field (lib/game/orbit.dart)."}
        project = created
    else:
        # The lock names this build's tools; `fynkel project lock` rewrites it.
        shutil.copyfile(SHOWCASE / "fynkel.lock.json", out / "fynkel.lock.json")
        if not (out / ".gitignore").exists():  # an earlier --app run keeps the starter's
            (out / ".gitignore").write_text(".fynkel/\n")
    (out / "fynkel.project.json").write_text(json.dumps(project, indent=2) + "\n")
    scene = scene_doc(a.static, a.bodies, a.behaviours, a.seed, orbit=a.app)
    (out / "assets/scenes/stress.scene.json").write_text(json.dumps(scene) + "\n")
    if a.app:
        cli = os.environ.get("FYNKEL_CLI") or str(ROOT / "target/debug/fynkel")
        subprocess.run([cli, "codegen", "--project", str(out), "--root", str(ROOT)], check=True)
    pairs = len(STATIC_PAIRS) + 2 + 1  # + body, behaviour, floor
    print(f"{out}: {len(scene['entities'])} entities ({a.static} static, {a.bodies} bodies, {a.behaviours} behaviours), "
          f"{len(MATERIALS)} materials, {pairs} distinct (mesh, material) pairs")


if __name__ == "__main__":
    main()
