#!/usr/bin/env python3
"""M2 tooling capture (blueprint §19.2c item 18).

The M0/M1 capture (benchmarks/m0/capture.py) measures the frame loop; this
one measures the authoring tools that M2 added, as wall-clock time of the
release `fynkel` binary from a shell — what an agent or a person waits for:

  validate        `fynkel validate` on the fixture project, a fresh
                  `fynkel create` project, and a synthetic scene of N entities
  scene load      `fynkel test --headless --steps 0` (parse + validate +
                  expand + spawn) on the same scenes
  glTF import     `fynkel import` of the 4 KB fixture robot and a generated
                  200×200 grid mesh (40 000 vertices, 79 202 triangles)
  cook            `fynkel cook` with an empty cache (miss), unchanged (hit),
                  and after one texture edit (partial)
  codegen (b)     `fynkel codegen --project` cold (no .dart_tool) and warm
  create          `fynkel create --no-flutter`

Each fast command runs `--runs` times; we report min and p50. `fynkel schema
list` is the process-startup baseline: subtract it to see the work itself.
The synthetic inputs are generated here so the numbers scale beyond the
hand-written fixtures; nothing is written into the repository except the
results:

  benchmarks/m2/results/<date>-<commit>.json   raw + aggregated numbers
  benchmarks/m2/results/<date>-<commit>.md     summary table

Usage: python3 benchmarks/m2/capture.py [--skip-build] [--runs 10] [--keep]
"""
import argparse
import datetime as dt
import json
import math
import os
import shutil
import statistics
import struct
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PROFILE = ROOT / "benchmarks/profiles/mac-m3max-2026.json"
FLUTTER_BIN = Path.home() / "Documents/flutter/flutter/bin"
FYNKEL = ROOT / "target/release/fynkel"
FIXTURE = ROOT / "crates/fynkel_document/fixtures/project"
ENV = {**os.environ, "PATH": f"{FLUTTER_BIN}:{os.environ.get('PATH', '')}", "CARGO_TERM_COLOR": "never"}

# Ids the fixture project already declares (see its fynkel.project.json).
ROBOT_MESH = "c0000000-0000-4000-8000-000000000001"
RUSTED_MATERIAL = "c0000000-0000-4000-8000-000000000020"
NS = uuid.UUID("6b1c2a3e-6f24-4c7e-9a4c-2f0a8e6b1d10")  # synthetic ids, stable per run


def sh(cmd, cwd=ROOT, check=True):
    return subprocess.run(cmd, cwd=cwd, check=check, text=True, capture_output=True, env=ENV).stdout.strip()


def environment():
    return {
        "captured_at": dt.datetime.now().astimezone().isoformat(timespec="seconds"),
        "commit": sh(["git", "rev-parse", "--short", "HEAD"]),
        "dirty": bool(sh(["git", "status", "--porcelain"], check=False)),
        "os": sh(["sw_vers", "-productVersion"]),
        "cpu": sh(["sysctl", "-n", "machdep.cpu.brand_string"]),
        "ram_gib": int(sh(["sysctl", "-n", "hw.memsize"])) // 2**30,
        "rustc": sh(["rustc", "--version"]),
        "dart": sh([str(FLUTTER_BIN / "dart"), "--version"], check=False),
    }


def build():
    print("building…", flush=True)
    sh(["cargo", "build", "--release", "-p", "fynkel_cli"])


def timed(cmd, cwd, runs, expect_ok=True):
    """Wall clock of `cmd` from a shell, `runs` times; min and p50 in ms plus the last stdout."""
    samples, out = [], ""
    for _ in range(runs):
        t0 = time.perf_counter()
        p = subprocess.run(cmd, cwd=cwd, text=True, capture_output=True, env=ENV)
        samples.append((time.perf_counter() - t0) * 1000)
        out = p.stdout
        if expect_ok and p.returncode != 0:
            raise SystemExit(f"{' '.join(map(str, cmd))} failed ({p.returncode}):\n{p.stdout}\n{p.stderr}")
    return {"min_ms": round(min(samples), 2), "p50_ms": round(statistics.median(samples), 2), "runs": runs}, out


def sid(kind, i):
    return str(uuid.uuid5(NS, f"{kind}/{i}"))


def synthetic_scene(n):
    """A scene of n entities in a grid: Transform + MeshRenderer (robot mesh, rusted material), game.Spinner on every other."""
    side = math.ceil(math.sqrt(n))
    entities = {}
    for i in range(n):
        comps = {
            sid("transform", i): {"type": "fynkel.Transform", "version": 1, "data": {
                "position": [float(i % side) * 2.0, 0.0, float(i // side) * 2.0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1]}},
            sid("mesh", i): {"type": "fynkel.MeshRenderer", "version": 1, "data": {
                "mesh": {"asset": ROBOT_MESH, "sub": "mesh/0"}, "material": {"asset": RUSTED_MATERIAL},
                "cast_shadows": True, "receive_shadows": True, "visible": True}},
        }
        if i % 2 == 0:
            comps[sid("spinner", i)] = {"type": "game.Spinner", "version": 1, "data": {"speed": 1.0 + (i % 7) * 0.25, "axis": [0, 1, 0]}}
        entities[sid("entity", i)] = {"name": f"Robot {i}", "parent": None, "children": [], "components": comps}
    return {"$schema": "https://fynkel.dev/schemas/scene.schema.json", "formatVersion": 1,
            "id": sid("scene", n), "name": f"Synthetic {n}", "entities": entities}


def grid_glb(side):
    """A GLB with one primitive: a (side × side)-vertex grid, POSITION/NORMAL/TEXCOORD_0 + u32 indices, one material."""
    verts = []
    for z in range(side):
        for x in range(side):
            verts += [x / (side - 1) - 0.5, 0.0, z / (side - 1) - 0.5, 0.0, 1.0, 0.0, x / (side - 1), z / (side - 1)]
    idx = []
    for z in range(side - 1):
        for x in range(side - 1):
            a = z * side + x
            idx += [a, a + side, a + 1, a + 1, a + side, a + side + 1]
    vbytes = struct.pack(f"<{len(verts)}f", *verts)
    ibytes = struct.pack(f"<{len(idx)}I", *idx)
    nv, ni = side * side, len(idx)
    bin_chunk = vbytes + ibytes
    bin_chunk += b"\0" * (-len(bin_chunk) % 4)
    gltf = {
        "asset": {"version": "2.0", "generator": "fynkel benchmarks/m2/capture.py"},
        "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0, "name": "Grid"}],
        "meshes": [{"name": "Grid", "primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2}, "indices": 3, "material": 0}]}],
        "materials": [{"name": "Grid", "pbrMetallicRoughness": {"baseColorFactor": [0.8, 0.8, 0.8, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.7}}],
        "buffers": [{"byteLength": len(bin_chunk)}],
        "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": len(vbytes), "byteStride": 32, "target": 34962},
                        {"buffer": 0, "byteOffset": len(vbytes), "byteLength": len(ibytes), "target": 34963}],
        "accessors": [
            {"bufferView": 0, "byteOffset": 0, "componentType": 5126, "count": nv, "type": "VEC3", "min": [-0.5, 0.0, -0.5], "max": [0.5, 0.0, 0.5]},
            {"bufferView": 0, "byteOffset": 12, "componentType": 5126, "count": nv, "type": "VEC3"},
            {"bufferView": 0, "byteOffset": 24, "componentType": 5126, "count": nv, "type": "VEC2"},
            {"bufferView": 1, "byteOffset": 0, "componentType": 5125, "count": ni, "type": "SCALAR"},
        ],
    }
    json_chunk = json.dumps(gltf, separators=(",", ":")).encode()
    json_chunk += b" " * (-len(json_chunk) % 4)
    total = 12 + 8 + len(json_chunk) + 8 + len(bin_chunk)
    return (b"glTF" + struct.pack("<II", 2, total) + struct.pack("<II", len(json_chunk), 0x4E4F534A) + json_chunk
            + struct.pack("<II", len(bin_chunk), 0x004E4942) + bin_chunk), nv, ni // 3


def prepare(work):
    """The fixture project (plus a grid mesh) and a second copy carrying the synthetic scenes, under `work`.

    Two copies because `validate`, `cook` and `load_project` walk every document under assets/: the
    10 000-entity scene must not sit inside the project whose per-document numbers we report."""
    fx = work / "fixture"
    shutil.copytree(FIXTURE, fx)
    big = work / "fixture_big"
    shutil.copytree(FIXTURE, big)
    scenes = {}
    for n in (1_000, 10_000):
        p = big / f"assets/scenes/synthetic_{n}.scene.json"
        p.write_text(json.dumps(synthetic_scene(n), indent=2) + "\n")
        scenes[n] = p
    glb, nv, nt = grid_glb(200)
    grid = fx / "assets/models/grid.glb"
    grid.write_bytes(glb)
    # The sidecar gives the source an id and lets `validate` see it as a regular asset.
    sh([str(FYNKEL), "asset", "init", str(grid)], cwd=fx)
    return fx, scenes, {"path": grid, "bytes": len(glb), "vertices": nv, "triangles": nt}


def capture_validate(fx, scenes, starter, runs):
    print("validate…", flush=True)
    r = {}
    r["baseline_schema_list"], _ = timed([FYNKEL, "schema", "list"], fx, runs)
    r["fixture_project"], out = timed([FYNKEL, "validate", "--json"], fx, runs)
    j = json.loads(out)
    r["fixture_project"].update(documents=j["documents"], sources=j["sources"])
    r["starter_project"], out = timed([FYNKEL, "validate", "--json"], starter, runs)
    j = json.loads(out)
    r["starter_project"].update(documents=j["documents"], sources=j["sources"])
    for n, p in scenes.items():
        r[f"scene_{n}"], _ = timed([FYNKEL, "scene", "validate", str(p)], p.parent, runs)
        r[f"scene_{n}"]["entities"] = n
    return r


def capture_scene_load(fx, scenes, starter, runs):
    print("scene load…", flush=True)
    r = {}
    r["fixture_project"], out = timed([FYNKEL, "test", "--headless", "--steps", "0", "--json"], fx, runs)
    r["fixture_project"]["entities"] = json.loads(out)["scene"]["entities"]
    r["starter_project"], out = timed([FYNKEL, "test", "--headless", "--steps", "0", "--json"], starter, runs)
    r["starter_project"]["entities"] = json.loads(out)["scene"]["entities"]
    for n, p in scenes.items():
        r[f"scene_{n}"], out = timed([FYNKEL, "test", "--headless", "--scene", str(p), "--steps", "0", "--json"], p.parent, runs)
        j = json.loads(out)
        assert j["scene"]["entities"] == n and j["scene"]["rejected_components"] == 0, j["scene"]
        r[f"scene_{n}"]["entities"] = n
    return r


def capture_import(fx, grid, runs):
    print("glTF import…", flush=True)
    r = {}
    r["robot_glb"], out = timed([FYNKEL, "import", "assets/models/robot.glb", "--json"], fx, runs)
    r["robot_glb"]["bytes"] = (fx / "assets/models/robot.glb").stat().st_size
    r["grid_glb"], out = timed([FYNKEL, "import", str(grid["path"]), "--json"], fx, runs)
    j = json.loads(out)
    assert j["ok"], j
    r["grid_glb"].update(bytes=grid["bytes"], vertices=grid["vertices"], triangles=grid["triangles"])
    return r


def capture_cook(fx, runs):
    print("cook…", flush=True)
    r = {}
    cache = fx / ".fynkel/cache"

    def cook_once(label, mutate=None):
        samples = []
        for _ in range(runs):
            if mutate:
                mutate()
            t0 = time.perf_counter()
            p = subprocess.run([FYNKEL, "cook", "--json"], cwd=fx, text=True, capture_output=True, env=ENV)
            samples.append((time.perf_counter() - t0) * 1000)
            j = json.loads(p.stdout)
            assert p.returncode == 0 and j["failed"] == 0, p.stdout
        r[label] = {"min_ms": round(min(samples), 2), "p50_ms": round(statistics.median(samples), 2), "runs": runs,
                    "cooked": j["cooked"], "cached": j["cached"], "assets": len(j["assets"])}

    cook_once("miss_empty_cache", lambda: shutil.rmtree(cache, ignore_errors=True))
    cook_once("hit_unchanged")
    swatch = fx / "assets/textures/swatch.png"
    original = swatch.read_bytes()
    edits = iter(range(1, runs + 1))

    def touch():
        # Keys are content hashes, so a new mtime alone is a hit; append a growing tail after
        # IEND (decoders ignore it) so every run sees a texture the cache has not cooked yet.
        swatch.write_bytes(original + b"\0" * next(edits))

    cook_once("partial_texture_edit", touch)
    swatch.write_bytes(original)
    return r


def capture_codegen(starter):
    print("codegen (b)…", flush=True)
    r = {}
    shutil.rmtree(starter / ".dart_tool", ignore_errors=True)
    r["cold_no_dart_tool"], out = timed([FYNKEL, "codegen", "--project", "."], starter, 1)
    r["warm"], out = timed([FYNKEL, "codegen", "--project", "."], starter, 3)
    r["check_warm"], out = timed([FYNKEL, "codegen", "--check", "--project", "."], starter, 3)
    return r


def capture_create(work):
    print("create…", flush=True)
    samples = []
    for i in range(3):
        d = work / f"created_{i}"
        t0 = time.perf_counter()
        sh([str(FYNKEL), "create", str(d), "--no-flutter"], cwd=work)
        samples.append((time.perf_counter() - t0) * 1000)
    return {"no_flutter": {"min_ms": round(min(samples), 2), "p50_ms": round(statistics.median(samples), 2), "runs": 3}}


def ms(a):
    return f"{a['min_ms']:.1f} / {a['p50_ms']:.1f}"


def markdown(r):
    e, v, l, i, c, g, cr = r["environment"], r["validate"], r["scene_load"], r["import"], r["cook"], r["codegen"], r["create"]
    base = v["baseline_schema_list"]["min_ms"]
    lines = [
        f"# M2 tooling capture {e['captured_at']} — commit {e['commit']}{' (dirty)' if e['dirty'] else ''}",
        "", f"Profile `{r['profile']['profile']}`; release `fynkel` binary, wall clock from a shell, {r['runs']} runs per cell; "
        f"values are min / p50 in ms. Process start-up (`fynkel schema list`) is {base:.1f} ms and is included in every number.", "",
        "| Command | Input | min / p50 (ms) |", "| --- | --- | --- |",
        f"| `fynkel validate` | fixture project ({v['fixture_project']['documents']} documents, {v['fixture_project']['sources']} sources) | {ms(v['fixture_project'])} |",
        f"| `fynkel validate` | `fynkel create` project ({v['starter_project']['documents']} documents, {v['starter_project']['sources']} sources) | {ms(v['starter_project'])} |",
        f"| `fynkel scene validate` | synthetic scene, 1 000 entities | {ms(v['scene_1000'])} |",
        f"| `fynkel scene validate` | synthetic scene, 10 000 entities | {ms(v['scene_10000'])} |",
        f"| scene load (`test --headless --steps 0`) | fixture project startup scene ({l['fixture_project']['entities']} entities incl. prefab) | {ms(l['fixture_project'])} |",
        f"| scene load | `fynkel create` startup scene ({l['starter_project']['entities']} entities) | {ms(l['starter_project'])} |",
        f"| scene load | synthetic scene, 1 000 entities | {ms(l['scene_1000'])} |",
        f"| scene load | synthetic scene, 10 000 entities | {ms(l['scene_10000'])} |",
        f"| `fynkel import` | robot.glb ({i['robot_glb']['bytes']} B) | {ms(i['robot_glb'])} |",
        f"| `fynkel import` | grid.glb ({i['grid_glb']['bytes'] / 2**20:.1f} MiB, {i['grid_glb']['vertices']:,} vertices, {i['grid_glb']['triangles']:,} triangles) | {ms(i['grid_glb'])} |",
        f"| `fynkel cook` miss | empty cache, {c['miss_empty_cache']['assets']} assets incl. grid.glb | {ms(c['miss_empty_cache'])} |",
        f"| `fynkel cook` hit | unchanged ({c['hit_unchanged']['cached']} cached) | {ms(c['hit_unchanged'])} |",
        f"| `fynkel cook` partial | one texture edited ({c['partial_texture_edit']['cooked']} re-cooked, {c['partial_texture_edit']['cached']} cached) | {ms(c['partial_texture_edit'])} |",
        f"| `fynkel codegen --project` | cold (no .dart_tool, pub get + build_runner AOT) | {g['cold_no_dart_tool']['min_ms'] / 1000:.1f} s |",
        f"| `fynkel codegen --project` | warm | {g['warm']['min_ms'] / 1000:.1f} / {g['warm']['p50_ms'] / 1000:.1f} s |",
        f"| `fynkel codegen --check --project` | warm | {g['check_warm']['min_ms'] / 1000:.1f} / {g['check_warm']['p50_ms'] / 1000:.1f} s |",
        f"| `fynkel create --no-flutter` | starter project | {ms(cr['no_flutter'])} |",
    ]
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--runs", type=int, default=10)
    ap.add_argument("--keep", action="store_true", help="keep the temporary work directory")
    a = ap.parse_args()
    if not a.skip_build:
        build()
    work = Path(tempfile.mkdtemp(prefix="fynkel-m2-"))
    try:
        fx, scenes, grid = prepare(work)
        starter = work / "starter_game"
        sh([str(FYNKEL), "create", str(starter), "--no-flutter"], cwd=work)
        result = {
            "profile": json.loads(PROFILE.read_text()), "environment": environment(), "runs": a.runs,
            "validate": capture_validate(fx, scenes, starter, a.runs),
            "scene_load": capture_scene_load(fx, scenes, starter, a.runs),
            "import": capture_import(fx, grid, a.runs),
            "cook": capture_cook(fx, a.runs),
            "codegen": capture_codegen(starter),
            "create": capture_create(work),
        }
        result["import"]["grid_glb"]["path"] = str(grid["path"].relative_to(work))
    finally:
        if a.keep:
            print(f"kept {work}")
        else:
            shutil.rmtree(work, ignore_errors=True)
    stem = f"{dt.date.today().isoformat()}-{result['environment']['commit']}"
    out_dir = ROOT / "benchmarks/m2/results"
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / f"{stem}.json").write_text(json.dumps(result, indent=2))
    md = markdown(result)
    (out_dir / f"{stem}.md").write_text(md)
    print(md)
    print(f"wrote {out_dir / stem}.{{json,md}}")


if __name__ == "__main__":
    sys.exit(main())
