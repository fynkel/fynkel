#!/usr/bin/env python3
"""M0 benchmark capture (blueprint §15.1, backlog #12).

Builds the standalone host, the fynkel_platform_apple dylib (which exports
fynkel_native too) and the Flutter macOS host, runs each workload for
warm-up + sample seconds, parses their per-second stats lines and writes:

  benchmarks/m0/results/<date>-<commit>.json   raw + aggregated numbers
  benchmarks/m0/results/<date>-<commit>.md     summary table

Aggregation: each host prints p50/p95/p99 once per second. After dropping
the warm-up windows we report the median of the per-window p50/p95 and the
max of the per-window p99 — conservative and reproducible, but not identical
to a single distribution over all frames (which would need per-frame dumps).

Usage: python3 benchmarks/m0/capture.py [--skip-build] [--sample 10] [--warmup 2]
"""
import argparse
import datetime as dt
import json
import os
import re
import signal
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PROFILE = ROOT / "benchmarks/profiles/mac-m3max-2026.json"
FLUTTER_DIR = ROOT / "hosts/flutter_apple"
FLUTTER_BIN = Path.home() / "Documents/flutter/flutter/bin"
DART = FLUTTER_BIN / "dart"
FLUTTER = FLUTTER_BIN / "flutter"
FLUTTER_APP = FLUTTER_DIR / "build/macos/Build/Products/Release/fynkel_host_flutter.app/Contents/MacOS/fynkel_host_flutter"
# The profile bundle is AOT like release but keeps the VM service the C3 heap probe (M3-16) reads.
FLUTTER_PROFILE_APP = FLUTTER_DIR / "build/macos/Build/Products/Profile/fynkel_host_flutter.app/Contents/MacOS/fynkel_host_flutter"


def sh(cmd, cwd=ROOT, check=True):
    return subprocess.run(cmd, cwd=cwd, check=check, text=True, capture_output=True).stdout.strip()


def environment():
    return {
        "captured_at": dt.datetime.now().astimezone().isoformat(timespec="seconds"),
        "commit": sh(["git", "rev-parse", "--short", "HEAD"]),
        "dirty": bool(sh(["git", "status", "--porcelain"], check=False)),
        "os": sh(["sw_vers", "-productVersion"]),
        "cpu": sh(["sysctl", "-n", "machdep.cpu.brand_string"]),
        "ram_gib": int(sh(["sysctl", "-n", "hw.memsize"])) // 2**30,
        "rustc": sh(["rustc", "--version"]),
        "flutter": next((l for l in sh([str(FLUTTER), "--version"], check=False).splitlines() if l.startswith("Flutter")), "?"),
        "dart": sh([str(DART), "--version"], check=False),
        "xcode": sh(["xcodebuild", "-version"], check=False).splitlines()[0],
    }


def build():
    print("building…", flush=True)
    sh(["cargo", "build", "--release", "--workspace"])
    # One build serves all host modes; FYNKEL_HOST_MODE (viewport | none) is read at runtime.
    # package:fynkel's build hook (ADR-0015) builds and bundles the native runtime.
    sh([str(FLUTTER), "build", "macos", "--release"], cwd=FLUTTER_DIR)
    sh([str(FLUTTER), "build", "macos", "--profile"], cwd=FLUTTER_DIR)


def run_for(cmd, seconds, cwd=ROOT, env=None):
    p = subprocess.Popen(cmd, cwd=cwd, env={**os.environ, **(env or {})}, text=True,
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    try:
        out, _ = p.communicate(timeout=seconds)
    except subprocess.TimeoutExpired:
        p.send_signal(signal.SIGTERM)
        try:
            out, _ = p.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            p.kill()
            out, _ = p.communicate()
    return out


def place_window(name, x=100, y=100, w=1200, h=820):
    script = f'''
    tell application "{name}" to activate
    tell application "System Events" to tell process "{name}"
      set position of window 1 to {{{x}, {y}}}
      set size of window 1 to {{{w}, {h}}}
    end tell'''
    subprocess.run(["osascript", "-e", script], capture_output=True)


NUM = r"([0-9.]+)"


def agg(windows, key):
    """windows: list of dicts with p50/p95/p99 -> median p50/p95, max p99."""
    vals = [w[key] for w in windows if key in w]
    if not vals:
        return None
    return {
        "p50": statistics.median(v["p50"] for v in vals),
        "p95": statistics.median(v["p95"] for v in vals),
        "p99": max(v["p99"] for v in vals),
        "windows": len(vals),
    }


def capture_standalone(warmup, sample):
    print("standalone host…", flush=True)
    out = run_for([str(ROOT / "target/release/fynkel_host_standalone"), "--seconds", str(warmup + sample + 1)],
                  warmup + sample + 5, env={"RUST_LOG": "info"})
    windows = []
    for m in re.finditer(r"\[1s\] frames=(\d+) skipped=(\d+) \| interval ms p50=" + NUM + " p95=" + NUM + " p99=" + NUM +
                         r" \| acquire-wait ms p50=" + NUM + " p95=" + NUM + r" \| cpu ms p50=" + NUM + " p95=" + NUM + " p99=" + NUM, out):
        g = [float(x) for x in m.groups()]
        windows.append({"frames": g[0], "skipped": g[1],
                        "interval_ms": {"p50": g[2], "p95": g[3], "p99": g[4]},
                        "cpu_ms": {"p50": g[7], "p95": g[8], "p99": g[9]}})
    w = windows[warmup:]
    return {"windows_total": len(windows), "frames_per_s": statistics.median(x["frames"] for x in w) if w else None,
            "interval_ms": agg(w, "interval_ms"), "cpu_encode_submit_ms": agg(w, "cpu_ms")}


def parse_flutter(out):
    windows, frames = [], []
    for line in out.splitlines():
        if "path C surface" in line:
            d = {}
            m = re.search(r"surface (\d+)x(\d+)", line); d["viewport"] = [int(m.group(1)), int(m.group(2))]
            m = re.search(r"present interval ms p50=" + NUM + " p95=" + NUM + " p99=" + NUM + r"\s+\(n=(\d+), skipped=(\d+)\)", line)
            if not m or int(m.group(4)) < 30:
                continue
            d["interval_ms"] = {"p50": float(m.group(1)), "p95": float(m.group(2)), "p99": float(m.group(3))}
            d["frames"] = int(m.group(4)); d["skipped"] = int(m.group(5))
            for key, label in [("acquire_ms", "acquire wait ms"), ("cpu_ms", r"cpu encode\+submit")]:
                m = re.search(label + r"\s+p50=" + NUM + " p95=" + NUM + " p99=" + NUM, line)
                if m:
                    d[key] = {"p50": float(m.group(1)), "p95": float(m.group(2)), "p99": float(m.group(3))}
            windows.append(d)
        elif "viewport " in line and "dart tick" in line:
            d = {}
            m = re.search(r"viewport (\d+)x(\d+)", line); d["viewport"] = [int(m.group(1)), int(m.group(2))]
            m = re.search(r"dart tick interval ms\s+p50=" + NUM + " p95=" + NUM + " p99=" + NUM + r"\s+\(n=(\d+)", line)
            if not m or int(m.group(4)) < 30:
                continue  # startup window
            d["tick_interval_ms"] = {"p50": float(m.group(1)), "p95": float(m.group(2)), "p99": float(m.group(3))}
            d["ticks"] = int(m.group(4))
            for key, label in [("render_call_ms", r"ffi render\(\) call ms"), ("rust_cpu_ms", "rust cpu encode\\+submit"),
                               ("gpu_complete_ms", "submit→gpu complete")]:
                m = re.search(label + r"\s+p50=" + NUM + " p95=" + NUM + " p99=" + NUM, line)
                if m:
                    d[key] = {"p50": float(m.group(1)), "p95": float(m.group(2)), "p99": float(m.group(3))}
            m = re.search(r"rendered=(\d+) dropped\(ring full\)=(\d+)/\d+ presented=(\d+) stalePresent=(\d+)", line)
            d.update(rendered=int(m.group(1)), dropped=int(m.group(2)), presented=int(m.group(3)), stale=int(m.group(4)))
            m = re.search(r"present lag .* p50=(\d+) p95=(\d+) max=(\d+)\s+raster acquire wait ms p50=" + NUM + " p95=" + NUM + " max=" + NUM, line)
            if m:
                d["present_lag_frames"] = {"p50": int(m.group(1)), "p95": int(m.group(2)), "p99": int(m.group(3))}
                d["raster_acquire_wait_ms"] = {"p50": float(m.group(4)), "p95": float(m.group(5)), "p99": float(m.group(6))}
            windows.append(d)
        elif "flutter frames n=" in line:
            m = re.search(r"n=(\d+) viewport=(\w+)\s+ui build ms p50=" + NUM + " p95=" + NUM + r"\s+raster ms p50=" + NUM + " p95=" + NUM + " p99=" + NUM, line)
            if m and int(m.group(1)) >= 30:
                frames.append({"n": int(m.group(1)), "viewport": m.group(2),
                               "ui_build_ms": {"p50": float(m.group(3)), "p95": float(m.group(4)), "p99": float(m.group(4))},
                               "raster_ms": {"p50": float(m.group(5)), "p95": float(m.group(6)), "p99": float(m.group(7))}})
    return windows, frames


def capture_flutter(app, mode, warmup, sample):
    print(f"flutter host (mode {mode})…", flush=True)
    p = subprocess.Popen([str(app)], cwd=FLUTTER_DIR, env={**os.environ, "RUST_LOG": "info", "FYNKEL_HOST_MODE": mode},
                         text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    time.sleep(2.5)
    place_window(app.name)
    time.sleep(0.5)
    time.sleep(warmup + sample)
    p.send_signal(signal.SIGTERM)
    try:
        out, _ = p.communicate(timeout=5)
    except subprocess.TimeoutExpired:
        p.kill(); out, _ = p.communicate()
    windows, frames = parse_flutter(out)
    w, f = windows[warmup:], frames[warmup:]
    res = {"windows_total": len(windows), "flutter_frame_windows_total": len(frames)}
    if w and "frames" in w[0]:  # path C: engine-thread stats
        res.update({
            "viewport_physical": w[-1]["viewport"],
            "frames_per_s": statistics.median(x["frames"] for x in w),
            "skipped_total": sum(x["skipped"] for x in w),
            "interval_ms": agg(w, "interval_ms"), "acquire_ms": agg(w, "acquire_ms"), "rust_cpu_ms": agg(w, "cpu_ms"),
        })
    elif w:
        res.update({
            "viewport_physical": w[-1]["viewport"],
            "ticks_per_s": statistics.median(x["ticks"] for x in w),
            "dropped_total": sum(x["dropped"] for x in w), "stale_presents_total": sum(x["stale"] for x in w),
            "rendered_per_s": statistics.median(x["rendered"] for x in w),
            "tick_interval_ms": agg(w, "tick_interval_ms"), "render_call_ms": agg(w, "render_call_ms"),
            "rust_cpu_ms": agg(w, "rust_cpu_ms"), "gpu_complete_ms": agg(w, "gpu_complete_ms"),
            "present_lag_frames": agg(w, "present_lag_frames"), "raster_acquire_wait_ms": agg(w, "raster_acquire_wait_ms"),
        })
    if f:
        res.update({"flutter_frames_per_s": statistics.median(x["n"] for x in f),
                    "ui_build_ms": agg(f, "ui_build_ms"), "raster_ms": agg(f, "raster_ms")})
    return res


def capture_bridge():
    print("bridge bandwidth…", flush=True)
    out = sh([str(DART), "test", "--plain-name", "bandwidth"], cwd=ROOT / "packages/fynkel", check=False)
    m = re.search(r"bridge @ (\d+) entities: encode\+submit (\d+) us/tick, step (\d+) us/tick, read (\d+) us/tick \| in " + NUM + r" KiB/tick, out " + NUM + " KiB/tick", out)
    if not m:
        return {"error": out[-500:]}
    return {"entities": int(m.group(1)), "encode_submit_us": int(m.group(2)), "step_us": int(m.group(3)),
            "read_us": int(m.group(4)), "in_kib": float(m.group(5)), "out_kib": float(m.group(6)), "runtime": "dart test (JIT)"}


def capture_alloc():
    """Dart allocations + GC per frame on the M1 scene (packages/fynkel/tool/alloc_bench.dart)."""
    print("dart allocations…", flush=True)
    out = sh([str(DART), "run", "tool/alloc_bench.dart", "--frames", "3000", "--json"], cwd=ROOT / "packages/fynkel", check=False)
    line = next((l for l in out.splitlines() if l.startswith("{")), None)
    if line is None:
        return {"error": out[-500:]}
    return {**json.loads(line), "runtime": "dart run (JIT)"}


def capture_alloc_aot(warmup, sample):
    """C3 (M3-16): the Flutter host's `[alloc]` lines — Dart heap, bytes allocated and GC per one-second window
    (`DartHeapProbe`: the VM's GC timeline events + `getMemoryUsage`), in an AOT (profile) build of the real host
    rather than `dart run`."""
    print("dart allocations (AOT profile build)…", flush=True)
    if not FLUTTER_PROFILE_APP.exists():
        return {"error": f"no profile bundle at {FLUTTER_PROFILE_APP} (flutter build macos --profile)"}
    p = subprocess.Popen([str(FLUTTER_PROFILE_APP)], cwd=FLUTTER_DIR,
                         env={**os.environ, "RUST_LOG": "info", "FYNKEL_HOST_MODE": "viewport"},
                         text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    time.sleep(2.5)
    place_window(FLUTTER_PROFILE_APP.name)
    time.sleep(0.5 + warmup + sample)
    p.send_signal(signal.SIGTERM)
    try:
        out, _ = p.communicate(timeout=5)
    except subprocess.TimeoutExpired:
        p.kill(); out, _ = p.communicate()
    windows = []
    for line in out.splitlines():
        m = re.search(r"\[alloc\] frames=(\d+) heap=(\d+) capacity=(\d+) external=(\d+) allocated=(\d+) gc=(\d+) new=(\d+) old=(\d+) pause_ms=" + NUM, line)
        if m:
            windows.append({"frames": int(m.group(1)), "heap": int(m.group(2)), "capacity": int(m.group(3)),
                            "external": int(m.group(4)), "allocated": int(m.group(5)), "gc": int(m.group(6)),
                            "new": int(m.group(7)), "old": int(m.group(8)), "pause_ms": float(m.group(9))})
    if not windows:
        return {"error": "no [alloc] lines (no VM service in this build?)", "tail": out[-500:]}
    w = windows[warmup:] or windows
    frames = sum(x["frames"] for x in w)
    return {
        "runtime": "flutter profile bundle (AOT)", "windows": len(w), "frames": frames,
        "heap_bytes": {"p50": statistics.median(x["heap"] for x in w), "max": max(x["heap"] for x in w)},
        "capacity_bytes_max": max(x["capacity"] for x in w),
        "external_bytes_max": max(x["external"] for x in w),
        "allocated_bytes_per_frame": (sum(x["allocated"] for x in w) / frames) if frames else None,
        "method": "heap growth between the known points (sample, before/after each GC from the GC timeline stream)",
        "gc_count": sum(x["gc"] for x in w), "gc_new": sum(x["new"] for x in w), "gc_old": sum(x["old"] for x in w),
        "gc_total_ms": sum(x["pause_ms"] for x in w),
    }


def capture_headless():
    """`fynkel test --headless --json`: the Rust host loop, determinism checksum, step timing."""
    print("headless host…", flush=True)
    out = sh(["cargo", "run", "-q", "--release", "-p", "fynkel_cli", "--", "test", "--headless", "--steps", "3000", "--json"], check=False)
    start = out.find("{")
    try:
        return json.loads(out[start:]) if start >= 0 else {"error": out[-500:]}
    except json.JSONDecodeError:
        return {"error": out[-500:]}


def capture_leaks():
    """The M1 teardown leak tests, with their measured numbers."""
    print("leak tests…", flush=True)
    r = subprocess.run(["cargo", "test", "--release", "-p", "fynkel_native", "-p", "fynkel_platform_apple", "--test", "leak", "--test", "lifetime", "--", "--nocapture"],
                       cwd=ROOT, text=True, capture_output=True)
    lines = [l.strip() for l in (r.stdout + r.stderr).splitlines() if l.strip().startswith("[leak]")]
    return {"passed": r.returncode == 0, "lines": lines}


def fmt(a, unit="ms"):
    if not a:
        return "—"
    return f"{a['p50']:.2f} / {a['p95']:.2f} / {a['p99']:.2f} {unit}"


def markdown(r):
    s, fv, fc, fn, b = r["standalone"], r["flutter_path_a"], r["flutter_path_c"], r["flutter_no_viewport"], r["bridge"]
    lines = [
        f"# M0/M1 capture {r['environment']['captured_at']} — commit {r['environment']['commit']}{' (dirty)' if r['environment']['dirty'] else ''}",
        "", f"Profile `{r['profile']['profile']}`; warm-up {r['warmup_seconds']} s, sample {r['sample_seconds']} s; "
        "values are p50 / p95 / p99 (median of per-second p50/p95, max of per-second p99).", "",
        "| Metric | Standalone | Flutter Path A | Flutter Path C | Flutter, no viewport |", "| --- | --- | --- | --- | --- |",
        f"| frames or ticks per second | {s.get('frames_per_s')} | {fv.get('ticks_per_s')} | {fc.get('frames_per_s')} | {fn.get('flutter_frames_per_s')} |",
        f"| presented interval / Dart tick interval | {fmt(s.get('interval_ms'))} | {fmt(fv.get('tick_interval_ms'))} | {fmt(fc.get('interval_ms'))} | — |",
        f"| engine CPU encode+submit | {fmt(s.get('cpu_encode_submit_ms'))} | {fmt(fv.get('rust_cpu_ms'))} | {fmt(fc.get('rust_cpu_ms'))} | — |",
        f"| submit → GPU complete | — | {fmt(fv.get('gpu_complete_ms'))} | — | — |",
        f"| present lag (frames) | 0 | {fmt(fv.get('present_lag_frames'), '')} | 0 (own swapchain) | — |",
        f"| raster-thread acquire wait | — | {fmt(fv.get('raster_acquire_wait_ms'))} | — | — |",
        f"| Flutter UI build | — | {fmt(fv.get('ui_build_ms'))} | {fmt(fc.get('ui_build_ms'))} | {fmt(fn.get('ui_build_ms'))} |",
        f"| Flutter raster | — | {fmt(fv.get('raster_ms'))} | {fmt(fc.get('raster_ms'))} | {fmt(fn.get('raster_ms'))} |",
        f"| dropped / stale presents (sample) | {'—'} | {fv.get('dropped_total')} / {fv.get('stale_presents_total')} | {fc.get('skipped_total')} skipped / — | — |",
        f"| viewport (physical px) | 2560×1440 | {'×'.join(map(str, fv.get('viewport_physical', ['?'])))} | {'×'.join(map(str, fc.get('viewport_physical', ['?'])))} | — |",
        "", f"Bridge @ {b.get('entities')} entities: encode+submit {b.get('encode_submit_us')} µs, step {b.get('step_us')} µs, "
        f"read {b.get('read_us')} µs per tick; {b.get('in_kib')} KiB in / {b.get('out_kib')} KiB out ({b.get('runtime')}).",
    ]
    al, aot, hl, lk = r.get("alloc", {}), r.get("alloc_aot", {}), r.get("headless", {}), r.get("leaks", {})
    if "heap_bytes_per_frame" in al:
        gcs = ", ".join(f"{k}×{v}" for k, v in al.get("gc_reasons", {}).items()) or "none"
        lines += ["", f"Dart allocation on the M1 scene ({al['entities']} entities, {al['frames']} frames, {al['runtime']}): "
                  f"{al['heap_bytes_per_frame']:.0f} B/frame; GC in window: {al['gc_count']} ({gcs}), {al['gc_total_ms']:.2f} ms total."]
    if "allocated_bytes_per_frame" in aot:
        lines += ["", f"Dart allocation in the Flutter host ({aot['runtime']}, {aot['frames']} frames over {aot['windows']} s): "
                  f"{aot['allocated_bytes_per_frame']:.0f} B/frame, heap p50 {aot['heap_bytes']['p50'] / 2**20:.1f} MB "
                  f"(max {aot['heap_bytes']['max'] / 2**20:.1f} MB, capacity {aot['capacity_bytes_max'] / 2**20:.1f} MB); "
                  f"GC in window: {aot['gc_count']} ({aot['gc_new']} new, {aot['gc_old']} old), {aot['gc_total_ms']:.2f} ms total."]
    elif aot:
        lines += ["", f"Dart allocation in the Flutter host: {aot.get('error')}"]
    if "step_time" in hl:
        st = hl["step_time"]
        lines += ["", f"Headless host: {hl['steps']} steps, {hl['live_entities']} entities, step p50/p95/max {st['p50_us']}/{st['p95_us']}/{st['max_us']} µs, "
                  f"checksum {hl['checksum']}, missed deadlines {hl['missed_deadlines']}."]
    if lk:
        lines += ["", f"Leak tests {'passed' if lk.get('passed') else 'FAILED'}:"] + [f"- {l}" for l in lk.get("lines", [])]
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--warmup", type=int, default=None)
    ap.add_argument("--sample", type=int, default=None)
    a = ap.parse_args()
    profile = json.loads(PROFILE.read_text())
    warmup = a.warmup or profile["warm_up_seconds"]
    sample = a.sample or profile["sample_seconds"]
    if not a.skip_build:
        build()
    result = {
        "profile": profile, "environment": environment(), "warmup_seconds": warmup, "sample_seconds": sample,
        "standalone": capture_standalone(warmup, sample),
        "flutter_path_a": capture_flutter(FLUTTER_APP, "viewport", warmup, sample),
        # Path C prototype retired with M1-07 (see benchmarks/m0/results for its M0 numbers).
        "flutter_path_c": {},
        "flutter_no_viewport": capture_flutter(FLUTTER_APP, "none", warmup, sample),
        "bridge": capture_bridge(),
        # M1 additions (blueprint §19.2b item 10).
        "alloc": capture_alloc(),
        # M3-16 (C3): the same counters from the AOT profile bundle of the real host.
        "alloc_aot": capture_alloc_aot(warmup, sample),
        "headless": capture_headless(),
        "leaks": capture_leaks(),
    }
    stem = f"{dt.date.today().isoformat()}-{result['environment']['commit']}"
    out_dir = ROOT / "benchmarks/m0/results"
    (out_dir / f"{stem}.json").write_text(json.dumps(result, indent=2))
    md = markdown(result)
    (out_dir / f"{stem}.md").write_text(md)
    print(md)
    print(f"wrote {out_dir / stem}.{{json,md}}")


if __name__ == "__main__":
    sys.exit(main())
