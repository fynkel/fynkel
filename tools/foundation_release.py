#!/usr/bin/env python3
"""F8 repeatable local qualification. Success is not a platform release verdict.

Runs sequentially, retains every failure, pins the local release CLI, captures
source/toolchain/log hashes and raw timing records. A baseline comparison must
use the same machine and suite. Never substitutes an old result for a failed run.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import sys

from foundation_baseline import run_command

ROOT = Path(__file__).resolve().parent.parent


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def capture(args):
    try:
        p = subprocess.run(args, cwd=ROOT, text=True, capture_output=True)
        return {'exit_code': p.returncode, 'output': (p.stdout + p.stderr).strip()}
    except OSError as error:
        return {'exit_code': None, 'output': str(error)}


def sources():
    names = subprocess.check_output(
        ['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'], cwd=ROOT
    ).decode().split('\0')
    return {n: digest(ROOT / n) for n in sorted(set(names))
            if n and (ROOT / n).is_file() and not n.startswith('benchmarks/foundation/results/')}


def json_records(text):
    """Decode complete objects, including multiline JSON after tool progress text."""
    decoder = json.JSONDecoder()
    records = []
    offset = 0
    while (start := text.find('{', offset)) >= 0:
        try:
            value, offset = decoder.raw_decode(text, start)
            records.append(value)
        except ValueError:
            offset = start + 1
    return records


def percentiles(values):
    if not values or any(not isinstance(v, (int, float)) or not math.isfinite(v) or v < 0 for v in values):
        raise ValueError('invalid or missing timing samples')
    s = sorted(values)
    return {f'p{p}': s[max(0, math.ceil(len(s) * p / 100) - 1)] for p in (50, 95, 99)}


def metrics(records):
    result = {}
    for record in records:
        rows = record.get('samples', [record])
        if not isinstance(rows, list) or (rows and not isinstance(rows[0], dict)):
            rows = [record]
        for row in rows:
            samples = row.get('samples_us', row.get('samples_ns'))
            if samples is None:
                continue
            if len(samples) < 40:
                raise ValueError('qualification needs at least 40 samples')
            factor = 0.001 if 'samples_ns' in row else 1
            population = row.get("entities", row.get("assets"))
            if population is None:
                raise ValueError("missing population")
            key = f"{row.get('workload', 'registered')}/{population}"
            if key in result:
                raise ValueError('duplicate workload measurements')
            result[key] = percentiles([v * factor for v in samples])
    return result


def compare(current, baseline, ratio=1.5):
    if not current or set(current) != set(baseline):
        return ['workload keys are missing or differ from baseline']
    return [f'{key} p99 {value["p99"]:.1f}us exceeds regression ceiling'
            for key, value in current.items()
            if value['p99'] > max(baseline[key]['p99'] * ratio, baseline[key]['p99'] + 100)]


def suites():
    cargo = ['cargo', 'test', '--release', '--locked']
    def package(path, tool):
        return [(path, [tool, 'analyze']), (path, [tool, 'test'])]
    return {
        'correctness': [
            ('.', ['cargo', 'build', '--release', '--locked', '-p', 'fynkel_cli', '-p', 'fynkel_platform_apple', '-p', 'fynkel_native']),
            ('.', cargo + ['--workspace', '--', '--test-threads=1']),
            ('.', ['cargo', 'clippy', '--release', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings']),
            ('.', ['cargo', 'fmt', '--all', '--check']),
            ('.', [str(ROOT / 'target/release/fynkel'), 'codegen', '--check']),
            ('.', ['bash', 'tools/check_runtime_deps.sh']),
            *package('packages/fynkel', 'dart'),
            *package('packages/fynkel_authoring', 'dart'),
            *package('packages/fynkel_generator', 'dart'),
            *package('packages/fynkel_flutter', 'flutter'),
            *package('packages/fynkel_editor_api', 'flutter'),
            *package('examples/registered_gameplay', 'dart'),
            *package('apps/editor', 'flutter'),
        ],
        'native-scale': [('.', ['cargo', 'run', '--release', '--locked', '-p', 'fynkel_core', '--example', example])
                         for example in ('registered_bulk_bench', 'f4_sync_bench')],
        'dart-scale': [('packages/fynkel', ['dart', 'run', 'tool/f3_registry_bench.dart'])],
        'dart-aot': [
            ('packages/fynkel', ['dart', 'build', 'cli', '-t', 'tool/f3_registry_bench.dart', '-o', '../../target/f8-registry-aot']),
            ('.', [str(ROOT / 'target/f8-registry-aot/bundle/bin/f3_registry_bench'), '--aot']),
        ],
        'render': [
            ('.', cargo + ['-p', 'fynkel_core', '--test', 'f6_extract', '--', '--ignored', '--nocapture']),
            ('.', cargo + ['-p', 'fynkel_render', '--test', 'f6_render', '--', '--ignored', '--nocapture']),
            ('.', cargo + ['-p', 'fynkel_render', '--lib', 'benchmark_animated_bounds', '--', '--ignored', '--nocapture']),
        ],
        'transparency': [('.', cargo + ['-p', 'fynkel_render', '--test', 'shading', 'foundation_transparency_qualification', '--', '--ignored', '--nocapture'])],
        'editor-scale': [
            ('.', ['python3', 'benchmarks/scale/generate.py', '--out', str(ROOT / 'target/f8-scale-qualified')]),
            ('.', [str(ROOT / 'target/release/fynkel'), 'validate', '--project', str(ROOT / 'target/f8-scale-qualified'), '--json']),
            ('.', ['cargo', 'run', '--release', '--locked', '-p', 'fynkel_authoring', '--example', 'scale', '--', str(ROOT / 'target/f8-scale-qualified'), 'cook']),
            ('.', ['python3', 'benchmarks/scale/capture.py', str(ROOT / 'target/f8-scale-qualified')]),
        ],
        'editor-history': [
            ('.', ['cargo', 'build', '--release', '--locked', '-p', 'fynkel_authoring', '--example', 'foundation_history']),
            ('.', ['/usr/bin/time', '-l', str(ROOT / 'target/release/examples/foundation_history'), str(ROOT / 'target/f8-scale-qualified')]),
        ],
        'lifecycle': [('packages/fynkel', ['dart', 'run', 'tool/foundation_lifecycle_bench.dart'])],
        'packages': [('.', ['python3', 'tools/package_acceptance.py', '--work', str(ROOT / 'target/f8-packages'), '--mode', 'both'])],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--suite', choices=[*suites(), 'all'], default='correctness')
    parser.add_argument('--out', type=Path)
    parser.add_argument('--baseline', type=Path)
    parser.add_argument('--timeout', type=int, default=1800)
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error('--timeout must be positive')
    stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    out = args.out or ROOT / 'target/foundation-release' / stamp
    out.mkdir(parents=True, exist_ok=False)
    env = dict(os.environ, FYNKEL_CLI=str(ROOT / 'target/release/fynkel'))
    start = sources()
    report = dict(date=stamp, base_commit=capture(['git', 'rev-parse', 'HEAD'])['output'],
                  suite=args.suite, machine={'system': platform.platform(), 'cpu': capture(['sysctl', '-n', 'machdep.cpu.brand_string'])['output']},
                  toolchains={t: capture(c) for t, c in [('rust', ['rustc', '-Vv']), ('dart', ['dart', '--version']), ('flutter', ['flutter', '--version'])]},
                  source_sha256=start, checks=[], metrics_us={}, failures=[],
                  scope='Local automated qualification only; physical-device/second-machine release acceptance is separate.')
    selected = suites() if args.suite == 'all' else {args.suite: suites()[args.suite]}
    for suite, commands in selected.items():
        for cwd, command in commands:
            index = len(report['checks'])
            log = out / f'{index:02d}-{suite}.log'
            print(f'{suite}: {" ".join(command)}', flush=True)
            result = run_command(command, ROOT / cwd, log, args.timeout, env)
            report['checks'].append(dict(suite=suite, cwd=cwd, command=command, result=result, log=log.name, sha256=digest(log)))
            if result['exit_code'] != 0 or result.get('timed_out'):
                report['failures'].append(f'command {index} failed')
            records = json_records(log.read_text(errors='replace'))
            if records:
                (out / f'{index:02d}-records.json').write_text(json.dumps(records, indent=2)+'\n')
            if (suite in ('native-scale', 'dart-scale')
                    or (suite == 'dart-aot' and '--aot' in command)
                    or (suite == 'editor-scale' and 'benchmarks/scale/capture.py' in command)
                    or (suite == 'editor-history' and command[0] == '/usr/bin/time')):
                try:
                    values = metrics(records)
                    if not values:
                        raise ValueError('missing measured workloads')
                    prefix = f'{suite}/{command[-1]}'
                    report['metrics_us'].update({f'{prefix}/{k}': v for k,v in values.items()})
                except (ValueError, KeyError, TypeError) as e:
                    report['failures'].append(f'command {index}: {e}')
            (out / 'report.json').write_text(json.dumps(report, indent=2)+'\n')
    if sources() != start:
        report['failures'].append('source changed during qualification; rerun against stable source')
    if args.baseline:
        base = json.loads(args.baseline.read_text())
        if base['machine'] != report['machine'] or base['suite'] != report['suite'] or base.get('failures'):
            report['failures'].append('baseline machine/suite mismatch or unsuccessful baseline')
        else:
            report['failures'].extend(compare(report['metrics_us'], base['metrics_us']))
        report['baseline'] = {'path': str(args.baseline), 'sha256': digest(args.baseline)}
    report['checks_passed'] = not report['failures']
    (out / 'report.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps({'report': str(out / 'report.json'), 'failures': report['failures']}))
    return int(bool(report['failures']))


if __name__ == '__main__':
    sys.exit(main())
