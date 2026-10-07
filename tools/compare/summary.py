"""Summarises [cmp] lines: medians over the run after the first 15 lines
(loading), and the last thermal state. Usage: summary.py <log>..."""
import re, statistics, sys

for path in sys.argv[1:]:
    rows = []
    for line in open(path, errors="replace"):
        if "[cmp] engine=" not in line:
            continue
        kv = dict(re.findall(r"(\w+)=([^\s]+)", line.split("[cmp]", 1)[1]))
        rows.append(kv)
    steady = rows[15:] or rows
    def med(key):
        vals = [float(r[key]) for r in steady if r.get(key) not in (None, "null")]
        return statistics.median(vals) if vals else float("nan")
    thermal = [r.get("thermal") for r in rows]
    extra = " ".join(f"{k}={med(k):.2f}" for k in ("dart_ms", "render_cpu_ms") if any(k in r for r in steady))
    print(f"{path.split('/')[-1]}: n={len(steady)} fps={med('fps'):.1f} cpu={med('cpu_ms'):.2f} "
          f"gpu={med('gpu_ms'):.2f} gpu_p95={med('gpu_p95'):.2f} {extra} thermal {thermal[:1]}->{thermal[-1:]}")
