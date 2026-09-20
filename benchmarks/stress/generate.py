#!/usr/bin/env python3
"""The stress scene (blueprint §15.3, §19.2d item 6, ADR-0011): thousands of
objects, tracked per milestone against the benchmark manifest.

Writes an ordinary Fynkel project to benchmarks/stress/project/ (ignored by
git — the generator is the source):

  5 000 native movers    "Static NNNN": cubes, pyramids and quads from the
                          showcase's cube.glb and robot.glb over four
                          materials — 12 distinct (mesh, material) pairs, so
                          the renderer's draw count has a ceiling to check.
                          Since item 10 every one moves through the native
                          catalogue, no Dart: the cubes carry a `Rotator`
                          (yaw), the pyramids an `Oscillate` (a world-Y
                          bob), the quads a `LookAtCamera` (billboards)
  500 rigid bodies       "Body NNN": cubes with the `body` material, each a
                          dynamic RigidBody on a box Collider, dropped from
                          3–7 m onto the field (item 8: the physics stress)
  300 behaviours         "Behaviour NNN": pyramids with the `behaviour`
                          material; with --app each carries `game.Wobble`,
                          a Dart behaviour writing its transform every tick
                          (the 300 ticking behaviours §15.3 names). The bare
                          project has no Dart, so they stand still there
  Lantern                a PointLight with a `Follow` on "Body 000"
  Fountain               an `Emitter` at the field's centre: 10 Sparks/s
                          (assets/prefabs/spark.prefab.json, a small dynamic
                          cube) launched upwards, 4 s each, 40 alive at most
  floor (a box Collider: the ground the bodies land on), sun (casting),
  Environment, one perspective camera over the field. The movers carry
  box colliders too (5 500 colliders in the broad phase), so the bodies
  come to rest on them as well as on the floor — and their motion is
  synced into the broad phase every tick, which is part of what the item
  10 gate measures.

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
start pose is the bare camera's, 0.08 rad/s) and `game.Wobble`
(benchmarks/stress/wobble.dart) ticks on the 300 Behaviour pyramids, so it runs on this Mac and
on a phone: `fynkel run --project benchmarks/stress/project -d <iphone>`
(the cook stages the cache into the bundle; Q cycles the quality
profiles). The bare project (the default) is what CI and capture.py cook
and measure; the ids and the field are the same either way, the app's
scene differs by the Orbit component on the camera and the Wobble on
the 300 pyramids. `fynkel create` needs `flutter` on PATH and
the CLI at $FYNKEL_CLI or target/debug/fynkel.

Usage: python3 benchmarks/stress/generate.py [--out DIR] [--static 5000]
       [--bodies 500] [--behaviours 300] [--seed 1] [--app]
       [--no-movers]   (the statics stand still: the item 6/8 baseline)
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
SPARK_ID = uuid.UUID("5b6a1c2e-0000-4000-8000-00000000fab1")
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


def rotator(speed):
    return {"type": "fynkel.Rotator", "version": 1, "data": {"axis": [0.0, 1.0, 0.0], "speed": round(speed, 4), "space": "local"}}


def oscillate(amplitude, frequency, phase):
    return {"type": "fynkel.Oscillate", "version": 1, "data": {
        "axis": [0.0, 1.0, 0.0], "amplitude": round(amplitude, 4), "frequency": round(frequency, 4),
        "phase": round(phase, 4), "space": "world"}}


def look_at_camera():
    return {"type": "fynkel.LookAtCamera", "version": 1, "data": {"lock_y": True}}


def follow(target, offset, smoothing):
    return {"type": "fynkel.Follow", "version": 1, "data": {
        "target": target, "offset": list(offset), "smoothing": smoothing, "look_at": False}}


def emitter(prefab, rate, lifetime, max_alive, velocity):
    return {"type": "fynkel.Emitter", "version": 1, "data": {
        "prefab": {"asset": str(prefab)}, "rate": rate, "lifetime": lifetime, "max_alive": max_alive,
        "velocity": list(velocity), "enabled": True}}


def entity(name, components):
    return uid(f"entity/{name}"), {"name": name, "parent": None, "children": [],
                                   "components": {uid(f"component/{name}/{c['type']}"): c for c in components}}


def spark_prefab():
    """A small dynamic cube the Fountain emits (item 10)."""
    k, v = entity("Spark", [transform((0, 0, 0), scale=(0.3, 0.3, 0.3)), mesh_renderer("cube", "body"),
                            rigid_body(mass=0.1), collider(friction=0.5, layer=2)])
    return {"$schema": "https://fynkel.dev/schemas/prefab.schema.json", "formatVersion": 2,
            "id": str(SPARK_ID), "name": "Spark", "roots": [k], "entities": {k: v}}


def material_doc(name):
    color, metallic, roughness, extra = MATERIALS[name]
    return {"$schema": "https://fynkel.dev/schemas/material.schema.json", "formatVersion": 2,
            "id": uid(f"material/{name}"), "name": name.capitalize(), "shader": "fynkel.pbr@1",
            "base_color": color, "metallic": metallic, "roughness": roughness, **extra}


def project_doc():
    native = ["fynkel.Transform", "fynkel.MeshRenderer", "fynkel.Camera", "fynkel.DirectionalLight",
              "fynkel.PointLight", "fynkel.SpotLight", "fynkel.Environment", "fynkel.RigidBody", "fynkel.Collider",
              "fynkel.CharacterController", "fynkel.ThirdPersonCamera", "fynkel.Rotator", "fynkel.Oscillate",
              "fynkel.Follow", "fynkel.LookAtCamera", "fynkel.LifetimeDespawn", "fynkel.Emitter",
              "fynkel.TriggerZone"]
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


def scene_doc(n_static, n_bodies, n_behaviours, seed, orbit=False, movers=True):
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
    if movers:
        # The catalogue's other three (item 10): a light that follows the
        # first body down and around, a fountain of sparks at the centre.
        add("Lantern", [transform((0, 8, 0)), follow("Body 000", (0.0, 2.0, 0.0), 4.0),
                        {"type": "fynkel.PointLight", "version": 1, "data": {"color": [1.0, 0.85, 0.6], "intensity": 400.0, "range": 12.0}}])
        add("Fountain", [transform((0, 1.5, 0)), emitter(SPARK_ID, rate=10.0, lifetime=4.0, max_alive=40, velocity=(0.0, 9.0, 0.0))])

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
        components = [transform((x, y, z), yaw, (s, s, s)), mesh_renderer(mesh, mat), collider()]
        if movers:
            if mesh == "cube":
                components.append(rotator(rng.uniform(0.3, 1.5) * rng.choice((-1, 1))))
            elif mesh == "pyramid":
                components.append(oscillate(rng.uniform(0.15, 0.4), rng.uniform(0.2, 0.6), rng.uniform(0, math.tau)))
            else:
                components.append(look_at_camera())
        add(f"Static {n:04}", components)
    for n in range(n_bodies):
        x, z = place(i); i += 1
        s = rng.uniform(0.5, 0.9)
        add(f"Body {n:03}", [transform((x, 3.0 + rng.uniform(0, 4), z), quat_axis_angle((0, 1, 0), rng.uniform(0, math.tau)), (s, s, s)),
                              mesh_renderer("cube", "body"), rigid_body(mass=s ** 3 * 2.0), collider(friction=0.5, layer=2)])
    for n in range(n_behaviours):
        x, z = place(i); i += 1
        components = [transform((x, 0.5, z), quat_axis_angle((0, 1, 0), rng.uniform(0, math.tau)), (0.8, 0.8, 0.8)),
                      mesh_renderer("pyramid", "behaviour")]
        if orbit:  # the app: a Dart behaviour ticking on each
            components.append({"type": "game.Wobble", "version": 1, "data": {
                "amplitude": round(rng.uniform(0.2, 0.5), 3), "frequency": round(rng.uniform(0.3, 0.8), 3),
                "spin": round(rng.uniform(0.5, 2.0), 3), "phase": round(rng.uniform(0, math.tau), 3)}})
        add(f"Behaviour {n:03}", components)
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
    # The showcase's orbiting camera and the stress scene's own Wobble;
    # `fynkel codegen` (below) generates their schemas and registers them.
    shutil.copyfile(SHOWCASE / "lib/game/orbit.dart", out / "lib/game/orbit.dart")
    shutil.copyfile(Path(__file__).resolve().parent / "wobble.dart", out / "lib/game/wobble.dart")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", type=Path, default=ROOT / "benchmarks/stress/project")
    ap.add_argument("--static", type=int, default=5000)
    ap.add_argument("--bodies", type=int, default=500)
    ap.add_argument("--behaviours", type=int, default=300)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--app", action="store_true", help="wrap the project in the `fynkel create` starter app so it runs on a device")
    ap.add_argument("--no-movers", action="store_true", help="no catalogue components: the statics stand still (the item 6/8 baseline)")
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
        created["components"]["game.Wobble"] = {"kind": "behaviour", "version": 1,
                                                "description": "Bobs and spins its pyramid every tick (lib/game/wobble.dart)."}
        project = created
    else:
        # The lock names this build's tools; `fynkel project lock` rewrites it.
        shutil.copyfile(SHOWCASE / "fynkel.lock.json", out / "fynkel.lock.json")
        if not (out / ".gitignore").exists():  # an earlier --app run keeps the starter's
            (out / ".gitignore").write_text(".fynkel/\n")
    (out / "fynkel.project.json").write_text(json.dumps(project, indent=2) + "\n")
    scene = scene_doc(a.static, a.bodies, a.behaviours, a.seed, orbit=a.app, movers=not a.no_movers)
    (out / "assets/scenes/stress.scene.json").write_text(json.dumps(scene) + "\n")
    (out / "assets/prefabs/spark.prefab.json").write_text(json.dumps(spark_prefab(), indent=2) + "\n")
    if a.app:
        cli = os.environ.get("FYNKEL_CLI") or str(ROOT / "target/debug/fynkel")
        subprocess.run([cli, "codegen", "--project", str(out), "--root", str(ROOT)], check=True)
    pairs = len(STATIC_PAIRS) + 2 + 1  # + body, behaviour, floor
    print(f"{out}: {len(scene['entities'])} entities ({a.static} {'static' if a.no_movers else 'native movers'}, "
          f"{a.bodies} bodies, {a.behaviours} behaviours{' (game.Wobble)' if a.app else ''}), "
          f"{len(MATERIALS)} materials, {pairs} distinct (mesh, material) pairs")


if __name__ == "__main__":
    main()
