#!/usr/bin/env python3
"""Measure an already generated/validated scale project with the release example.

Run cargo build --release --locked -p fynkel_authoring --example scale first.
Forty fresh sessions measure open/rescan; forty warm cooks measure incremental
cost. Retains all samples. Stage times have the example's 0.1ms resolution.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('project', type=Path)
    args = parser.parse_args()
    samples = {}
    count = None
    for mode in ('session', 'cook-warm'):
        for iteration in range(43):
            p = subprocess.run([str(ROOT / 'target/release/examples/scale'), str(args.project.resolve()), mode],
                               check=True, text=True, capture_output=True)
            found = re.search(r'\((\d+) assets,', p.stdout)
            if not found:
                raise RuntimeError('missing asset population')
            population = int(found[1])
            if count is not None and population != count:
                raise RuntimeError('asset population changed during capture')
            count = population
            rows = re.findall(r'^(.*?): ([\d.]+) ms', p.stdout, flags=re.M)
            expected = 4 if mode == 'session' else 2
            if len(rows) != expected:
                raise RuntimeError(f'missing stage timings: {p.stdout}')
            if iteration >= 3:
                for stage, value in rows:
                    samples.setdefault(f'{mode}/{stage}', []).append(float(value) * 1000)
    for stage, values in samples.items():
        print(json.dumps({'workload': stage, 'assets': count, 'samples_us': values,
                          'warmup_runs': 3, 'scope': 'native editor session/cook stages; excludes Flutter UI layout; 100us timing resolution'}))


if __name__ == '__main__':
    main()
