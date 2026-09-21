#!/usr/bin/env python3
"""M3 benchmark capture (blueprint §15.3, §19.2d items 6 and 16): the stress
scene and the reference game against the manifest.

Generates benchmarks/stress/project (benchmarks/stress/generate.py), locks
and cooks it with this build's `fynkel`, renders it once headlessly for the
renderer's counts (instances, draws, culled, batches, casters), then runs it
in the standalone host (`--cache`) and the Flutter host (`FYNKEL_PROJECT`,
the `--profile` bundle: AOT with the VM service the C3 heap probe reads)
for warm-up + sample seconds each, parsing their per-second lines; cooks
examples/reference_game, renders its Playground for the counts, replays the
route (`test/collect_all.input.json`) headlessly for the tick-side timers,
runs the Playground in the standalone host and the game itself
(`FYNKEL_CAPTURE=1`: straight into the level, one `[stats]` line a second)
for warm-up + sample seconds each, and writes:

  benchmarks/m3/results/<date>-<commit>.json   raw + aggregated numbers
  benchmarks/m3/results/<date>-<commit>.md     summary table + gate

Gate (item 6): 5 000 static meshes at the display's 120 Hz on the profile,
draw calls ≤ distinct (mesh, material) pairs, culling dropping what is off
screen. Gate (item 8): the 500 rigid bodies settle in the headless host
(`fynkel test --headless --steps 600`) at a fixed per-tick cost — p50 step
time under 4 ms while they fall and once they sleep. Gate (item 10): the
5 000 statics are native movers (Rotator/Oscillate/LookAtCamera) ticked
in the catalogue's slots for under 1 ms a tick together, the per-system
times are in the headless report, and the scene still holds 120 Hz. Gate
(item 19c): 200 animated rigs (the box character: 200 skins, 2 200 joints,
their animators on clips, a blend space and 50 additive layers) are posed
by the animation system for under 1 ms a tick together, their markers are
counted, and the tick's p50 stays under 4 ms with everything else on. The
300 ticking Dart behaviours need the `--app` project (generate.py --app,
`game.Wobble`); their HUD numbers are recorded by hand in the results
(capture.py drives the bare project). Aggregation as in
benchmarks/m0/capture.py (median of per-second p50/p95, max of per-second
p99). Gate (item 16): the 12 ms p95 total-work target (blueprint §15.2:
`simulate + render()` per frame, `FynkelFrameStats.totalWork`) checked on
both scenes in their Flutter apps, the GPU pass timestamps read back, the
headless report carrying the same timers, the reference game's Dart heap
steady (allocation and GC per frame recorded, not gated).

Usage: python3 benchmarks/m3/capture.py [--skip-build] [--sample 10] [--warmup 2]
       [--quality <profile>] [--no-flutter]
"""
import argparse
import datetime as dt
import importlib.util
import json
import re
import signal
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "benchmarks/m0"))
m0 = importlib.import_module("capture")

PROFILE = ROOT / "benchmarks/profiles/mac-m3max-2026.json"
STRESS = ROOT / "benchmarks/stress/project"
CACHE = STRESS / ".fynkel/cache"
FYNKEL = ROOT / "target/release/fynkel"
STANDALONE = ROOT / "target/release/fynkel_host_standalone"
REFERENCE = ROOT / "examples/reference_game"
REFERENCE_CACHE = REFERENCE / ".fynkel/cache"
REFERENCE_APP = REFERENCE / "build/macos/Build/Products/Profile/reference_game.app/Contents/MacOS/reference_game"
PLAYGROUND = REFERENCE / "assets/scenes/playground.scene.json"
ROUTE = REFERENCE / "test/collect_all.input.json"
NUM = m0.NUM
TOTAL_WORK_P95_MS = 12.0


def build():
    m0.build()
    print("building the reference game (profile)…", flush=True)
    m0.sh([str(m0.FLUTTER), "build", "macos", "--profile"], cwd=REFERENCE)


def prepare_stress():
    print("stress scene…", flush=True)
    gen = m0.sh([sys.executable, str(ROOT / "benchmarks/stress/generate.py")])
    m0.sh([str(FYNKEL), "project", "lock", "--project", str(STRESS)])
    m0.sh([str(FYNKEL), "cook", "--project", str(STRESS)])
    out = m0.sh([str(FYNKEL), "render", "--project", str(STRESS), "--no-cook", "--json",
                 "--out", str(ROOT / "benchmarks/stress/render.png")])
    r = json.loads(out)
    return {
        "generator": gen,
        "entities": r["scene"]["entities"],
        "meshes": r["meshes"], "instances": r["instances"], "culled": r["culled"],
        "draws": r["draws"], "batches": r["batches"], "transparent": r["transparent"],
        "shadow_casters": r["shadowCasters"], "shadow_draws": r["shadowDraws"], "shadow_map": r["shadowMap"],
        "quality": r["quality"],
    }


def capture_physics(steps=600):
    """The stress scene's bodies in the headless host: step-time
    percentiles and the physics counts of a run of `steps` ticks (item 8)."""
    print("headless host, stress scene physics…", flush=True)
    out = m0.sh([str(FYNKEL), "test", "--headless", "--project", str(STRESS), "--no-cook", "--steps", str(steps), "--json"])
    r = json.loads(out)
    # The tail of the run, bodies asleep: the settled cost.
    tail = json.loads(m0.sh([str(FYNKEL), "test", "--headless", "--project", str(STRESS), "--no-cook",
                             "--steps", str(steps * 3), "--json"]))
    return {"steps": steps, "step_time_us": r["step_time"], "total_ms": r["total_ms"], "checksum": r["checksum"],
            "settled_steps": steps * 3, "settled_step_time_us": tail["step_time"], "settled_checksum": tail["checksum"],
            "live_entities": tail["live_entities"],
            # Per-system time (item 10): the settled run's mean/max per
            # `World::step` phase and what the last tick touched.
            "systems": tail.get("systems", []),
            # The rigs (item 19c): what the animation system stepped and
            # the markers crossed over the settled run.
            "animation": tail.get("animation") or {},
            **(r.get("physics") or {})}


def med(windows, key):
    """Median of a per-window p50/p95 pair (the M3-16 timers the standalone
    host prints without a p99)."""
    vals = [w[key] for w in windows if key in w]
    if not vals:
        return None
    return {"p50": statistics.median(v["p50"] for v in vals), "p95": statistics.median(v["p95"] for v in vals),
            "windows": len(vals)}


def capture_standalone(warmup, sample, quality, cache=CACHE, scene=None, label="stress scene"):
    print(f"standalone host, {label}…", flush=True)
    cmd = [str(STANDALONE), "--cache", str(cache), "--seconds", str(warmup + sample + 1)]
    if scene:
        cmd += ["--scene", scene]
    if quality:
        cmd += ["--quality", quality]
    out = m0.run_for(cmd, warmup + sample + 8, env={"RUST_LOG": "info"})
    windows = []
    for m in re.finditer(r"\[1s\] frames=(\d+) skipped=(\d+) \| interval ms p50=" + NUM + " p95=" + NUM + " p99=" + NUM +
                         r" \| acquire-wait ms p50=" + NUM + " p95=" + NUM + r" \| cpu ms p50=" + NUM + " p95=" + NUM + " p99=" + NUM +
                         r" \| draws=(\d+) instances=(\d+) culled=(\d+) batches=(\d+) skinned=(\d+) shadow=(\d+)/(\d+)"
                         r"(?: \| sim ms p50=" + NUM + " p95=" + NUM + r" \| prepare ms p50=" + NUM + " p95=" + NUM +
                         r" \| gpu ms p50=" + NUM + " p95=" + NUM + r" n=(\d+) untimed=(\d+) \| mem MB native=" + NUM +
                         " resident=" + NUM + " gpu=" + NUM + ")?", out):
        g = [None if x is None else float(x) for x in m.groups()]
        w = {"frames": g[0], "skipped": g[1],
             "interval_ms": {"p50": g[2], "p95": g[3], "p99": g[4]},
             "cpu_ms": {"p50": g[7], "p95": g[8], "p99": g[9]},
             "draws": int(g[10]), "instances": int(g[11]), "culled": int(g[12]), "batches": int(g[13]),
             "skinned": int(g[14]), "shadow_casters": int(g[15]), "shadow_draws": int(g[16])}
        if g[17] is not None:  # the M3-16 timers and memory
            w.update({"sim_ms": {"p50": g[17], "p95": g[18]}, "prepare_ms": {"p50": g[19], "p95": g[20]},
                      "gpu_ms": {"p50": g[21], "p95": g[22]}, "gpu_timed": int(g[23]), "gpu_untimed": int(g[24]),
                      "mem_native_mb": g[25], "mem_resident_mb": g[26], "mem_gpu_mb": g[27]})
        windows.append(w)
    w = windows[warmup:]
    last = w[-1] if w else {}
    return {"windows_total": len(windows), "frames_per_s": statistics.median(x["frames"] for x in w) if w else None,
            "skipped_total": sum(x["skipped"] for x in w),
            "interval_ms": m0.agg(w, "interval_ms"), "cpu_encode_submit_ms": m0.agg(w, "cpu_ms"),
            "sim_ms": med(w, "sim_ms"), "prepare_ms": med(w, "prepare_ms"), "gpu_frame_ms": med(w, "gpu_ms"),
            "gpu_timed_per_s": statistics.median(x["gpu_timed"] for x in w if "gpu_timed" in x) if any("gpu_timed" in x for x in w) else None,
            # The host prints the renderer's running total of frames without a stamp.
            "gpu_untimed_total": last.get("gpu_untimed"),
            **{k: last.get(k) for k in ("draws", "instances", "culled", "batches", "skinned", "shadow_casters", "shadow_draws",
                                         "mem_native_mb", "mem_resident_mb", "mem_gpu_mb")}}


def parse_stats(out):
    """The `[stats]` lines `FynkelFrameStats.summary()` prints: one dict per
    window; `a/b/c` values become p50/p95/p99, `a/b` pairs stay a string."""
    windows = []
    for line in out.splitlines():
        i = line.find("[stats] ")
        if i < 0:
            continue
        w = {}
        for kv in line[i + 8:].split():
            k, _, v = kv.partition("=")
            parts = v.split("/")
            if len(parts) == 3:
                w[k] = {"p50": float(parts[0]), "p95": float(parts[1]), "p99": float(parts[2])}
            elif v in ("true", "false"):
                w[k] = v == "true"
            else:
                try:
                    w[k] = int(v)
                except ValueError:
                    try:
                        w[k] = float(v)
                    except ValueError:
                        w[k] = v
        windows.append(w)
    return windows


def summarize_stats(windows, warmup):
    """Aggregate the `[stats]` windows after the warm-up: timers as
    median p50/p95, max p99; counts from the last window; totals summed;
    the Dart heap per frame."""
    w = windows[warmup:]
    if not w:
        return {"windows_total": len(windows)}
    last = w[-1]
    frames = sum(x.get("frames", 0) for x in w)
    res = {"windows_total": len(windows), "frames_total": frames,
           "frames_per_s": statistics.median(x["frames"] for x in w),
           "ticks_per_s": statistics.median(x["ticks"] for x in w),
           "dropped_total": sum(x.get("dropped", 0) for x in w), "missed_total": sum(x.get("missed", 0) for x in w)}
    for k in ("interval", "simulate", "render", "behaviours", "native", "work", "build", "raster", "extract", "prepare",
              "encode", "cpu", "gpu_complete", "gpu_frame", "gpu_shadow", "gpu_scene", "gpu_post"):
        res[k + "_ms"] = m0.agg(w, k)
    for k in ("entities", "instances", "draws", "culled", "shadow", "snapshot", "snapshot_peak", "mem_native", "mem_resident",
              "mem_assets", "mem_gpu", "gpu_timestamps"):
        res[k] = last.get(k)
    timed = [x["gpu_n"] for x in w if "gpu_n" in x]
    res["gpu_timed_per_s"] = statistics.median(timed) if timed else None
    res["gpu_untimed_total"] = last.get("gpu_untimed")  # the renderer's running total
    if "heap" in last:
        heap = [x for x in w if "heap" in x]
        hf = sum(x.get("frames", 0) for x in heap)
        res["dart_heap"] = {
            "windows": len(heap), "runtime": "flutter profile bundle (AOT)",
            "heap_bytes": {"p50": statistics.median(x["heap"] for x in heap), "max": max(x["heap"] for x in heap)},
            "capacity_bytes_max": max(x["heap_capacity"] for x in heap),
            "allocated_bytes_per_frame": (sum(x["heap_alloc"] for x in heap) / hf) if hf else None,
            "gc_count": sum(x["gc"] for x in heap), "gc_new": sum(x["gc_new"] for x in heap),
            "gc_old": sum(x["gc_old"] for x in heap), "gc_total_ms": sum(x["gc_pause"] for x in heap),
        }
    return res


def run_app(app, env, warmup, sample):
    p = subprocess.Popen([str(app)], cwd=ROOT,
                         env={**m0.os.environ, "RUST_LOG": "info", **env},
                         text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    time.sleep(2.5)
    m0.place_window(app.name)
    time.sleep(0.5)
    time.sleep(warmup + sample)
    p.send_signal(signal.SIGTERM)
    try:
        out, _ = p.communicate(timeout=5)
    except subprocess.TimeoutExpired:
        p.kill(); out, _ = p.communicate()
    return out


def capture_flutter(warmup, sample):
    print("flutter host, stress scene…", flush=True)
    app = m0.FLUTTER_PROFILE_APP if m0.FLUTTER_PROFILE_APP.exists() else m0.FLUTTER_APP
    out = run_app(app, {"FYNKEL_HOST_MODE": "viewport", "FYNKEL_PROJECT": str(STRESS)}, warmup, sample)
    windows, frames = m0.parse_flutter(out)
    counts = {}
    for m in re.finditer(r"instances=(\d+) draws=(\d+) culled=(\d+) shadow=(\d+)/(\d+)", out):
        counts = {"instances": int(m.group(1)), "draws": int(m.group(2)), "culled": int(m.group(3)),
                  "shadow_casters": int(m.group(4)), "shadow_draws": int(m.group(5))}
    w, f = windows[warmup:], frames[warmup:]
    res = {"windows_total": len(windows), **counts}
    if w:
        res.update({
            "viewport_physical": w[-1]["viewport"],
            "ticks_per_s": statistics.median(x["ticks"] for x in w),
            "dropped_total": sum(x["dropped"] for x in w), "stale_presents_total": sum(x["stale"] for x in w),
            "rendered_per_s": statistics.median(x["rendered"] for x in w),
            "tick_interval_ms": m0.agg(w, "tick_interval_ms"), "render_call_ms": m0.agg(w, "render_call_ms"),
            "rust_cpu_ms": m0.agg(w, "rust_cpu_ms"), "gpu_complete_ms": m0.agg(w, "gpu_complete_ms"),
            "present_lag_frames": m0.agg(w, "present_lag_frames"),
        })
    if f:
        res.update({"flutter_frames_per_s": statistics.median(x["n"] for x in f),
                    "ui_build_ms": m0.agg(f, "ui_build_ms"), "raster_ms": m0.agg(f, "raster_ms")})
    # M3-16: the same window as the `[stats]` line — the frame's split,
    # the GPU passes, memory, the Dart heap (profile bundle).
    res["stats"] = summarize_stats(parse_stats(out), warmup)
    res["bundle"] = "profile" if app == m0.FLUTTER_PROFILE_APP else "release"
    return res


def prepare_reference():
    """The reference game cooked with this build; its Playground's render
    counts and the route replayed headlessly (item 16: the tick-side timers
    `fynkel test --headless --json` reports — step, extract, frame, memory,
    systems, physics, animation — under the route's checksum)."""
    print("reference game…", flush=True)
    m0.sh([str(FYNKEL), "cook", "--project", str(REFERENCE)])
    scene = json.loads(PLAYGROUND.read_text())["id"]
    r = json.loads(m0.sh([str(FYNKEL), "render", "--project", str(REFERENCE), "--no-cook", "--scene", scene, "--json",
                          "--out", str(ROOT / "benchmarks/m3/reference_render.png")]))
    route = json.loads(m0.sh([str(FYNKEL), "test", "--headless", "--project", str(REFERENCE), "--no-cook", "--scene", scene,
                              "--input", str(ROUTE), "--steps", "1400", "--json"]))
    return {
        "scene": scene, "entities": r["scene"]["entities"], "meshes": r["meshes"], "instances": r["instances"],
        "culled": r["culled"], "draws": r["draws"], "batches": r["batches"], "skinned": r["skinned"],
        "shadow_casters": r["shadowCasters"], "shadow_draws": r["shadowDraws"], "quality": r["quality"],
        "route": {k: route.get(k) for k in ("steps", "live_entities", "checksum", "total_ms", "step_time", "extract_time",
                                            "frame_time", "memory", "systems", "physics", "animation", "audio", "input",
                                            "diagnostics", "missed_deadlines")},
    }


def capture_reference_app(warmup, sample):
    """The reference game itself (its `--profile` bundle, `FYNKEL_CAPTURE=1`:
    the Playground from the first frame, the player idle at the spawn),
    one `[stats]` line a second."""
    print("reference game app, Playground…", flush=True)
    if not REFERENCE_APP.exists():
        return {"error": f"no profile bundle at {REFERENCE_APP} (flutter build macos --profile)"}
    out = run_app(REFERENCE_APP, {"FYNKEL_CAPTURE": "1", "FYNKEL_PROJECT": str(REFERENCE)}, warmup, sample)
    res = summarize_stats(parse_stats(out), warmup)
    if "frames_total" not in res:
        res["tail"] = out[-800:]
    return res


def gate(r):
    s, st = r["standalone"], r["stress"]
    pairs = st["batches"]  # distinct opaque (mesh, material) pairs among the records
    checks = [
        ("5 000 static meshes cooked and drawn", st["meshes"] >= 5000 and st["instances"] + st["culled"] == st["meshes"]),
        ("standalone at the display's 120 Hz (≥ 118 frames/s, p99 interval < 10 ms)",
         (s.get("frames_per_s") or 0) >= 118 and (s.get("interval_ms") or {"p99": 99})["p99"] < 10),
        (f"draw calls ≤ distinct (mesh, material) pairs ({st['draws']} ≤ {pairs})", st["draws"] <= pairs),
        (f"culling drops off-screen instances ({st['culled']} culled)", st["culled"] > 0),
    ]
    p = r.get("physics") or {}
    if p:
        falling, settled = p["step_time_us"]["p50_us"] / 1000, p["settled_step_time_us"]["p50_us"] / 1000
        checks += [
            (f"≥ 500 rigid bodies stepped headlessly ({p['bodies']} bodies, {p['colliders']} colliders, "
             f"{p['collision_enter']} contacts)", p["bodies"] >= 500 and p["collision_enter"] >= p["bodies"]),
            (f"physics at a fixed cost: p50 tick {falling:.2f} ms falling, {settled:.2f} ms settled (< 4 ms)",
             falling < 4 and settled < 4),
            (f"deterministic: the {p['steps']}-tick checksum repeats ({p['checksum']})",
             p["checksum"] == json.loads(m0.sh([str(FYNKEL), "test", "--headless", "--project", str(STRESS), "--no-cook",
                                                "--steps", str(p["steps"]), "--json"]))["checksum"]),
        ]
    systems = {x["system"]: x for x in p.get("systems", [])}
    if systems:
        movers = {k: systems.get(k, {"entities": 0, "mean_us": 0.0}) for k in ("rotator", "oscillate", "lookAtCamera")}
        moved = sum(m["entities"] for m in movers.values())
        cost = sum(m["mean_us"] for m in movers.values()) / 1000
        parts = sum(x["mean_us"] for k, x in systems.items() if k != "step")
        checks += [
            (f"5 000 native movers ticked by the catalogue ({moved} components, {cost:.2f} ms a tick together, < 1 ms)",
             moved >= 5000 and cost < 1),
            (f"per-system time in the stats ({len(systems)} phases; step {systems['step']['mean_us'] / 1000:.2f} ms ≥ "
             f"the phases' {parts / 1000:.2f} ms; emitter keeps {systems.get('lifetimeDespawn', {}).get('entities', 0)} sparks alive)",
             len(systems) == 15 and systems["step"]["mean_us"] >= parts * 0.9 and systems.get("emitter", {}).get("entities", 0) == 1),
        ]
    anim = p.get("animation") or {}
    if anim and systems:
        a = systems.get("animation", {"entities": 0, "mean_us": 0.0})
        settled = p["settled_step_time_us"]["p50_us"] / 1000
        checks += [
            (f"200 animated rigs posed by the animation system ({anim['skinned_meshes']} skinned meshes over {anim['joints']} joints, "
             f"{a['entities']} animators + skins, {anim['markers']} markers crossed; {a['mean_us'] / 1000:.2f} ms a tick, < 1 ms; "
             f"p50 tick {settled:.2f} ms with everything else on, < 4 ms)",
             anim["skinned_meshes"] >= 200 and a["entities"] >= 400 and anim["markers"] > 0 and a["mean_us"] < 1000 and settled < 4),
            (f"the rigs drawn skinned by the standalone host at 120 Hz ({s.get('skinned')} skinned instances in view, "
             f"{s.get('frames_per_s')} frames/s)", (s.get("skinned") or 0) >= 150 and (s.get("frames_per_s") or 0) >= 118),
        ]
    # Item 16: the 12 ms p95 total-work target checked on both scenes, the
    # GPU timestamps read back, the headless report's timers present.
    fs = (r.get("flutter_path_a") or {}).get("stats") or {}
    ra = r.get("reference_app") or {}
    ref = r.get("reference") or {}
    route = ref.get("route") or {}
    def work(stats):
        return (stats.get("work_ms") or {}).get("p95")
    if fs.get("work_ms"):
        checks.append((f"12 ms p95 total work, stress scene in the Flutter host: simulate + render() p95 {work(fs):.2f} ms "
                       f"({fs['frames_per_s']:.0f} frames/s, {fs.get('bundle', r['flutter_path_a'].get('bundle'))} bundle)",
                       work(fs) < TOTAL_WORK_P95_MS))
    else:
        checks.append(("12 ms p95 total work, stress scene in the Flutter host: no [stats] windows", False))
    if ra.get("work_ms"):
        checks.append((f"12 ms p95 total work, reference game Playground in its own app: simulate + render() p95 {work(ra):.2f} ms "
                       f"(behaviours p95 {ra['behaviours_ms']['p95']:.2f}, native step {ra['native_ms']['p95']:.2f}, "
                       f"render() {ra['render_ms']['p95']:.2f}; {ra['frames_per_s']:.0f} frames/s)", work(ra) < TOTAL_WORK_P95_MS))
    else:
        checks.append(("12 ms p95 total work, reference game: no [stats] windows", False))
    timed = [x for x in (fs, ra) if x.get("gpu_timestamps")]
    def gpu95(stats):
        v = (stats.get("gpu_frame_ms") or {}).get("p95")
        return "—" if v is None else f"{v:.2f}"
    checks.append((f"GPU pass timestamps read back in both apps (stress {fs.get('gpu_timed_per_s')} frames/s timed, "
                   f"frame p95 {gpu95(fs)} ms; reference {ra.get('gpu_timed_per_s')} timed, frame p95 {gpu95(ra)} ms)",
                   len(timed) == 2 and all((x.get("gpu_timed_per_s") or 0) > 0 for x in timed)))
    checks.append((f"the headless report carries the same timers on the route (step p95 "
                   f"{(route.get('step_time') or {}).get('p95_us')} µs, extract p95 {(route.get('extract_time') or {}).get('p95_us')} µs, "
                   f"frame p95 {(route.get('frame_time') or {}).get('p95_us')} µs, native {((route.get('memory') or {}).get('native_bytes') or 0) / 2**20:.1f} MB; "
                   f"checksum {route.get('checksum')})",
                   bool(route.get("extract_time")) and bool(route.get("frame_time")) and bool(route.get("memory"))
                   and (route.get("steps") or 0) > 0 and route.get("diagnostics") == 0))
    if ra.get("dart_heap"):
        h = ra["dart_heap"]
        checks.append((f"reference game Dart heap recorded (C3): {h['allocated_bytes_per_frame'] / 1024:.1f} KB/frame allocated, "
                       f"{h['gc_count']} collections in {h['windows']} s ({h['gc_total_ms']:.2f} ms), heap p50 "
                       f"{h['heap_bytes']['p50'] / 2**20:.1f} MB", True))
    return [{"check": c, "passed": bool(ok)} for c, ok in checks]


def fmt2(a):
    return "—" if not a else f"{a['p50']:.2f} / {a['p95']:.2f} ms"


def mb(b):
    return "—" if b is None else f"{b / 2**20:.1f}"


def heap_line(h):
    if not h:
        return "— (no VM service)"
    return (f"{h['heap_bytes']['p50'] / 2**20:.1f} MB (max {h['heap_bytes']['max'] / 2**20:.1f}), "
            f"{h['allocated_bytes_per_frame'] / 1024:.1f} KB/frame allocated, {h['gc_count']} GC in {h['windows']} s "
            f"({h['gc_new']} new, {h['gc_old']} old; {h['gc_total_ms']:.2f} ms)")


def markdown(r):
    s, f, st, env = r["standalone"], r["flutter_path_a"], r["stress"], r["environment"]
    q = st["quality"]
    lines = [
        f"# M3 capture {env['captured_at']} — commit {env['commit']}{' (dirty)' if env['dirty'] else ''}",
        "", f"Profile `{r['profile']['profile']}`; warm-up {r['warmup_seconds']} s, sample {r['sample_seconds']} s; "
        "values are p50 / p95 / p99 (median of per-second p50/p95, max of per-second p99).", "",
        f"## Stress scene (`benchmarks/stress`)", "",
        f"{st['entities']} entities, {st['meshes']} meshes: {st['instances']} drawn in {st['draws']} draws over {st['batches']} "
        f"(mesh, material) batches, {st['culled']} culled, {st['transparent']} blended; {st['shadow_casters']} casters in "
        f"{st['shadow_draws']} draws into a {st['shadow_map']}² shadow map; quality `{q['profile']}` "
        f"(msaa {q['msaa']}, scale {q['renderScale']}, {q['renderSize'][0]}×{q['renderSize'][1]}).", "",
        "| Metric | Standalone | Flutter Path A |", "| --- | --- | --- |",
        f"| frames or ticks per second | {s.get('frames_per_s')} | {f.get('ticks_per_s')} |",
        f"| presented interval / Dart tick interval | {m0.fmt(s.get('interval_ms'))} | {m0.fmt(f.get('tick_interval_ms'))} |",
        f"| engine CPU prepare+encode+submit | {m0.fmt(s.get('cpu_encode_submit_ms'))} | {m0.fmt(f.get('rust_cpu_ms'))} |",
        f"| ffi render() call | — | {m0.fmt(f.get('render_call_ms'))} |",
        f"| submit → GPU complete | — | {m0.fmt(f.get('gpu_complete_ms'))} |",
        f"| present lag (frames) | 0 | {m0.fmt(f.get('present_lag_frames'), '')} |",
        f"| Flutter UI build / raster | — | {m0.fmt(f.get('ui_build_ms'))} / {m0.fmt(f.get('raster_ms'))} |",
        f"| skipped / dropped (sample) | {s.get('skipped_total')} | {f.get('dropped_total')} |",
        f"| instances / draws / culled | {s.get('instances')} / {s.get('draws')} / {s.get('culled')} | {f.get('instances')} / {f.get('draws')} / {f.get('culled')} |",
        f"| skinned instances | {s.get('skinned')} | — |",
        f"| viewport (physical px) | 2560×1440 | {'×'.join(map(str, f.get('viewport_physical', ['?'])))} |",
    ]
    fs = f.get("stats") or {}
    lines += ["", f"M3-16 timers (the standalone host's `[1s]` line; the Flutter host's `[stats]` line, {f.get('bundle', '?')} bundle):", "",
              "| Metric | Standalone | Flutter Path A |", "| --- | --- | --- |",
              f"| simulate (ticks + extract) | {fmt2(s.get('sim_ms'))} | {m0.fmt(fs.get('simulate_ms'))} |",
              f"| Dart behaviours / native step | — | {m0.fmt(fs.get('behaviours_ms'))} / {m0.fmt(fs.get('native_ms'))} |",
              f"| **total work** (simulate + render()) | — | **{m0.fmt(fs.get('work_ms'))}** |",
              f"| extract / prepare / encode | — / {fmt2(s.get('prepare_ms'))} / — | {m0.fmt(fs.get('extract_ms'))} / {m0.fmt(fs.get('prepare_ms'))} / {m0.fmt(fs.get('encode_ms'))} |",
              f"| GPU frame (timestamps) | {fmt2(s.get('gpu_frame_ms'))} ({s.get('gpu_timed_per_s')}/s timed, {s.get('gpu_untimed_total')} untimed) | {m0.fmt(fs.get('gpu_frame_ms'))} ({fs.get('gpu_timed_per_s')}/s timed, {fs.get('gpu_untimed_total')} untimed) |",
              f"| GPU shadow / scene / post | — | {m0.fmt(fs.get('gpu_shadow_ms'))} / {m0.fmt(fs.get('gpu_scene_ms'))} / {m0.fmt(fs.get('gpu_post_ms'))} |",
              f"| memory native / resident / GPU (MB) | {s.get('mem_native_mb')} / {s.get('mem_resident_mb')} / {s.get('mem_gpu_mb')} | "
              f"{mb(fs.get('mem_native'))} / {mb(fs.get('mem_resident'))} / {mb(fs.get('mem_gpu'))} (assets {mb(fs.get('mem_assets'))}, snapshot {fs.get('snapshot')} B) |",
              f"| Dart heap (C3) | — | {heap_line(fs.get('dart_heap'))} |"]
    ref, rs, ra = r.get("reference") or {}, r.get("reference_standalone") or {}, r.get("reference_app") or {}
    if ref:
        route = ref.get("route") or {}
        st_, ex, fr, mem = route.get("step_time") or {}, route.get("extract_time") or {}, route.get("frame_time") or {}, route.get("memory") or {}
        lines += [
            "", "## Reference game (`examples/reference_game`, the Playground)", "",
            f"{ref['entities']} entities, {ref['meshes']} meshes: {ref['instances']} drawn in {ref['draws']} draws over {ref['batches']} "
            f"batches, {ref['culled']} culled, {ref['skinned']} skinned; {ref['shadow_casters']} casters in {ref['shadow_draws']} shadow "
            f"draws; quality `{ref['quality']['profile']}`.", "",
            f"The route replayed headlessly (`fynkel test --headless --input test/collect_all.input.json`, {route.get('steps')} ticks, "
            f"{route.get('live_entities')} live entities at the end, checksum `{route.get('checksum')}`): step p50 / p95 / max "
            f"{st_.get('p50_us')} / {st_.get('p95_us')} / {st_.get('max_us')} µs, extract {ex.get('p50_us')} / {ex.get('p95_us')} / {ex.get('max_us')} µs, "
            f"frame {fr.get('p50_us')} / {fr.get('p95_us')} / {fr.get('max_us')} µs; native {mb(mem.get('native_bytes'))} MB "
            f"(peak {mb(mem.get('native_peak_bytes'))}), resident {mb(mem.get('resident_bytes'))} MB, assets {mb(mem.get('residency_bytes'))} MB; "
            f"{(route.get('physics') or {}).get('bodies')} bodies, {(route.get('animation') or {}).get('skinned_meshes')} skinned meshes, "
            f"{(route.get('audio') or {}).get('started')} voices started.", "",
            "| Metric | Standalone | The game (Flutter, profile bundle, idle at the spawn) |", "| --- | --- | --- |",
            f"| frames or ticks per second | {rs.get('frames_per_s')} | {ra.get('ticks_per_s')} ({ra.get('frames_per_s')} frames) |",
            f"| presented interval / Dart tick interval | {m0.fmt(rs.get('interval_ms'))} | {m0.fmt(ra.get('interval_ms'))} |",
            f"| simulate (ticks + extract) | {fmt2(rs.get('sim_ms'))} | {m0.fmt(ra.get('simulate_ms'))} |",
            f"| Dart behaviours / native step | — | {m0.fmt(ra.get('behaviours_ms'))} / {m0.fmt(ra.get('native_ms'))} |",
            f"| render() call (extract + encode + submit) | — | {m0.fmt(ra.get('render_ms'))} |",
            f"| **total work** (simulate + render()) | — | **{m0.fmt(ra.get('work_ms'))}** |",
            f"| engine CPU prepare+encode+submit | {m0.fmt(rs.get('cpu_encode_submit_ms'))} | {m0.fmt(ra.get('cpu_ms'))} |",
            f"| extract / prepare / encode | — / {fmt2(rs.get('prepare_ms'))} / — | {m0.fmt(ra.get('extract_ms'))} / {m0.fmt(ra.get('prepare_ms'))} / {m0.fmt(ra.get('encode_ms'))} |",
            f"| GPU frame (timestamps) | {fmt2(rs.get('gpu_frame_ms'))} ({rs.get('gpu_timed_per_s')}/s timed) | {m0.fmt(ra.get('gpu_frame_ms'))} ({ra.get('gpu_timed_per_s')}/s timed, {ra.get('gpu_untimed_total')} untimed) |",
            f"| GPU shadow / scene / post | — | {m0.fmt(ra.get('gpu_shadow_ms'))} / {m0.fmt(ra.get('gpu_scene_ms'))} / {m0.fmt(ra.get('gpu_post_ms'))} |",
            f"| Flutter UI build / raster | — | {m0.fmt(ra.get('build_ms'))} / {m0.fmt(ra.get('raster_ms'))} |",
            f"| instances / draws / culled | {rs.get('instances')} / {rs.get('draws')} / {rs.get('culled')} | {ra.get('instances')} / {ra.get('draws')} / {ra.get('culled')} |",
            f"| memory native / resident / GPU (MB) | {rs.get('mem_native_mb')} / {rs.get('mem_resident_mb')} / {rs.get('mem_gpu_mb')} | "
            f"{mb(ra.get('mem_native'))} / {mb(ra.get('mem_resident'))} / {mb(ra.get('mem_gpu'))} (assets {mb(ra.get('mem_assets'))}, snapshot {ra.get('snapshot')} B) |",
            f"| Dart heap (C3) | — | {heap_line(ra.get('dart_heap'))} |",
            f"| dropped / missed deadlines (sample) | {rs.get('skipped_total')} skipped | {ra.get('dropped_total')} / {ra.get('missed_total')} |",
        ]
    p = r.get("physics") or {}
    if p:
        lines += [
            "", "## Physics (`fynkel test --headless`, item 8)", "",
            f"{p['bodies']} dynamic bodies over {p['colliders']} colliders; {p['steps']} ticks in {p['total_ms']:.0f} ms, "
            f"step p50 / p95 / max {p['step_time_us']["p50_us"] / 1000:.2f} / {p['step_time_us']['p95_us'] / 1000:.2f} / "
            f"{p['step_time_us']['max_us'] / 1000:.2f} ms; settled ({p['settled_steps']} ticks) "
            f"{p['settled_step_time_us']["p50_us"] / 1000:.2f} / {p['settled_step_time_us']['p95_us'] / 1000:.2f} / "
            f"{p['settled_step_time_us']['max_us'] / 1000:.2f} ms; {p['collision_enter']} enter / {p['collision_exit']} exit "
            f"events; checksum `{p['checksum']}`.",
        ]
    if p.get("systems"):
        lines += ["", "## Per-system time (`fynkel test --headless --json` → `systems`, item 10)", "",
                  f"The settled run ({p['settled_steps']} ticks, {p['live_entities']} live entities at the end): "
                  "wall time per `World::step` phase, mean and worst tick, and what the last tick touched.", "",
                  "| System | mean µs | max µs | entities (last tick) |", "| --- | --- | --- | --- |"]
        lines += [f"| {x['system']} | {x['mean_us']:.0f} | {x['max_us']} | {x['entities']} |" for x in p["systems"]]
    if p.get("animation"):
        an = p["animation"]
        lines += ["", "## Animation (`fynkel test --headless --json` → `animation`, item 19c)", "",
                  f"{an['skinned_meshes']} skinned meshes over {an['joints']} joints, {an['entities']} animators and skins stepped "
                  f"({an['clips']} clips and spaces, {an['skins']} skins registered); {an['markers']} markers crossed over the settled "
                  f"run, {an['ended']} clips ended, {an['diagnostics']} diagnostics."]
    lines += ["", "## Gate (items 6, 8, 10, 19c and 16)", ""]
    lines += [f"- {'PASS' if g['passed'] else 'FAIL'} — {g['check']}" for g in r["gate"]]
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--no-flutter", action="store_true")
    ap.add_argument("--warmup", type=int, default=None)
    ap.add_argument("--sample", type=int, default=None)
    ap.add_argument("--quality", default=None, help="one of the stress project's profiles (default: its default)")
    a = ap.parse_args()
    profile = json.loads(PROFILE.read_text())
    warmup = a.warmup or profile["warm_up_seconds"]
    sample = a.sample or profile["sample_seconds"]
    if not a.skip_build:
        build()
    result = {
        "profile": profile, "environment": m0.environment(), "warmup_seconds": warmup, "sample_seconds": sample,
        "stress": prepare_stress(),
    }
    result["physics"] = capture_physics()
    result["standalone"] = capture_standalone(warmup, sample, a.quality)
    result["flutter_path_a"] = {} if a.no_flutter else capture_flutter(warmup, sample)
    # Item 16: the reference game — cooked, rendered, the route headless,
    # the Playground in the standalone host and in the game itself.
    result["reference"] = prepare_reference()
    result["reference_standalone"] = capture_standalone(warmup, sample, None, cache=REFERENCE_CACHE,
                                                        scene=result["reference"]["scene"], label="reference game Playground")
    result["reference_app"] = {} if a.no_flutter else capture_reference_app(warmup, sample)
    result["gate"] = gate(result)
    stem = f"{dt.date.today().isoformat()}-{result['environment']['commit']}"
    out_dir = ROOT / "benchmarks/m3/results"
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / f"{stem}.json").write_text(json.dumps(result, indent=2))
    md = markdown(result)
    (out_dir / f"{stem}.md").write_text(md)
    print(md)
    print(f"wrote {out_dir / stem}.{{json,md}}")
    return 0 if all(g["passed"] for g in result["gate"]) else 1


if __name__ == "__main__":
    sys.exit(main())
