#!/usr/bin/env python3
"""Generate and preflight deterministic mobile workloads; never a device FPS gate."""
import argparse
import hashlib
import json
import os
import shutil
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
# Counts describe authored populations, NOT simultaneously visible objects.
PRESETS = {
    "dense-500": dict(static=500, bodies=50, behaviours=30, rigs=20, lights=8),
    "dense-2000": dict(static=2000, bodies=150, behaviours=100, rigs=60, lights=16),
    "dense-5000": dict(static=5000, bodies=500, behaviours=300, rigs=200, lights=32),
}


def prepare(preset, out, cli, app=False):
    out = out.resolve()
    # generate.py --app can remove an existing directory. Never forward one.
    if out.exists():
        raise ValueError(f"output already exists: {out}; choose a fresh directory")
    out.parent.mkdir(parents=True, exist_ok=True)
    report = {"schemaVersion": 1, "workload": preset, "seed": 1,
              "populations": PRESETS[preset], "app": app,
              "dartBehavioursExecute": app,
              "status": "preparing", "deviceQualification": "not-run",
              "resolution": [1320, 2868], "quality": "medium", "commands": []}
    evidence = out.with_name(out.name + '-preflight.json')
    if evidence.exists():
        raise ValueError(f"evidence already exists: {evidence}")

    def save():
        evidence.write_text(json.dumps(report, indent=2) + '\n')

    def run(args, key, cwd=ROOT):
        entry = {"stage": key, "argv": list(map(str, args))}
        report['commands'].append(entry)
        save()
        result = subprocess.run(entry['argv'], cwd=cwd, text=True, capture_output=True,
                                env={**os.environ, 'FYNKEL_CLI': str(cli)}, timeout=600)
        entry.update(exitCode=result.returncode, stdout=result.stdout, stderr=result.stderr)
        save()
        if result.returncode:
            raise RuntimeError(f"{key} failed; see {evidence}")
        return result.stdout

    save()
    try:
        args = [sys.executable, ROOT / 'benchmarks/stress/generate.py', '--out', out, '--seed', '1']
        for name, count in PRESETS[preset].items():
            args.extend(['--' + name, str(count)])
        if app:
            args.append('--app')
        run(args, 'generate')
        if app:
            shutil.copyfile(ROOT / 'benchmarks/mobile/main.dart', out / 'lib/main.dart')
            (out / 'tool').mkdir(exist_ok=True)
            shutil.copyfile(ROOT / 'benchmarks/mobile/cpu.dart', out / 'tool/cpu.dart')
            flutter = shutil.which('flutter')
            if flutter is None:
                raise RuntimeError('flutter must be on PATH')
            run([flutter, 'pub', 'add', 'path_provider:^2.1.5'], 'dependency', cwd=out)
        run([cli, 'project', 'lock', '--project', out], 'lock')
        run([cli, 'validate', '--project', out, '--json'], 'validate')
        run([cli, 'cook', '--project', out], 'cook')
        # Headless counts prove actual visibility and submission, not phone speed.
        raw = run([cli, 'render', '--project', out, '--no-cook', '--json', '--quality', 'medium',
                   '--size', '1320x2868', '--out', out / 'preflight.png'], 'render')
        report['render'] = json.loads(raw)
        raw = run([cli, 'test', '--headless', '--project', out, '--no-cook',
                   '--steps', '120', '--json'], 'simulation')
        report['simulation'] = json.loads(raw)
        scene = out / 'assets/scenes/stress.scene.json'
        report['sceneSha256'] = hashlib.sha256(scene.read_bytes()).hexdigest()
        report['generatorSha256'] = hashlib.sha256((ROOT / 'benchmarks/stress/generate.py').read_bytes()).hexdigest()
        report['status'] = 'preflight-complete'
    except Exception as error:
        report.update(status='failed', error=str(error))
        raise
    finally:
        save()
    print(evidence)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('preset', choices=PRESETS)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--cli', type=Path, default=ROOT / 'target/release/fynkel')
    parser.add_argument('--app', action='store_true')
    args = parser.parse_args()
    prepare(args.preset, args.out, args.cli.resolve(), args.app)
