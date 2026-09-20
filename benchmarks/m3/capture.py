#!/usr/bin/env python3
"""M3 benchmark capture (blueprint §15.3, §19.2d items 6 and 16): the stress
scene against the manifest — and, once items 14/15 exist, the reference
game.

Generates benchmarks/stress/project (benchmarks/stress/generate.py), locks
and cooks it with this build's `fynkel`, renders it once headlessly for the
renderer's counts (instances, draws, culled, batches, casters), then runs it
in the standalone host (`--cache`) and the Flutter host (`FYNKEL_PROJECT`)
for warm-up + sample seconds each, parsing their per-second lines, and
writes:

  benchmarks/m3/results/<date>-<commit>.json   raw + aggregated numbers
  benchmarks/m3/results/<date>-<commit>.md     summary table + gate

Gate (item 6): 5 000 static meshes at the display's 120 Hz on the profile,
draw calls ≤ distinct (mesh, material) pairs, culling dropping what is off
screen. Aggregation as in benchmarks/m0/capture.py (median of per-second
p50/p95, max of per-second p99).

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
NUM = m0.NUM


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


def capture_standalone(warmup, sample, quality):
    print("standalone host, stress scene…", flush=True)
    cmd = [str(STANDALONE), "--cache", str(CACHE), "--seconds", str(warmup + sample + 1)]
    if quality:
        cmd += ["--quality", quality]
    out = m0.run_for(cmd, warmup + sample + 8, env={"RUST_LOG": "info"})
    windows = []
    for m in re.finditer(r"\[1s\] frames=(\d+) skipped=(\d+) \| interval ms p50=" + NUM + " p95=" + NUM + " p99=" + NUM +
                         r" \| acquire-wait ms p50=" + NUM + " p95=" + NUM + r" \| cpu ms p50=" + NUM + " p95=" + NUM + " p99=" + NUM +
                         r" \| draws=(\d+) instances=(\d+) culled=(\d+) batches=(\d+) shadow=(\d+)/(\d+)", out):
        g = [float(x) for x in m.groups()]
        windows.append({"frames": g[0], "skipped": g[1],
                        "interval_ms": {"p50": g[2], "p95": g[3], "p99": g[4]},
                        "cpu_ms": {"p50": g[7], "p95": g[8], "p99": g[9]},
                        "draws": int(g[10]), "instances": int(g[11]), "culled": int(g[12]), "batches": int(g[13]),
                        "shadow_casters": int(g[14]), "shadow_draws": int(g[15])})
    w = windows[warmup:]
    last = w[-1] if w else {}
    return {"windows_total": len(windows), "frames_per_s": statistics.median(x["frames"] for x in w) if w else None,
            "skipped_total": sum(x["skipped"] for x in w),
            "interval_ms": m0.agg(w, "interval_ms"), "cpu_encode_submit_ms": m0.agg(w, "cpu_ms"),
            **{k: last.get(k) for k in ("draws", "instances", "culled", "batches", "shadow_casters", "shadow_draws")}}


def capture_flutter(warmup, sample):
    print("flutter host, stress scene…", flush=True)
    app = m0.FLUTTER_APP
    p = subprocess.Popen([str(app)], cwd=m0.FLUTTER_DIR,
                         env={**m0.os.environ, "RUST_LOG": "info", "FYNKEL_HOST_MODE": "viewport", "FYNKEL_PROJECT": str(STRESS)},
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
    return [{"check": c, "passed": bool(ok)} for c, ok in checks]


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
        f"| viewport (physical px) | 2560×1440 | {'×'.join(map(str, f.get('viewport_physical', ['?'])))} |",
        "", "## Gate (item 6)", "",
    ] + [f"- {'PASS' if g['passed'] else 'FAIL'} — {g['check']}" for g in r["gate"]]
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
        m0.build()
    result = {
        "profile": profile, "environment": m0.environment(), "warmup_seconds": warmup, "sample_seconds": sample,
        "stress": prepare_stress(),
    }
    result["standalone"] = capture_standalone(warmup, sample, a.quality)
    result["flutter_path_a"] = {} if a.no_flutter else capture_flutter(warmup, sample)
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
