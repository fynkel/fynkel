#!/usr/bin/env python3
"""Capture F0 validation with commands, toolchain, revision, logs, and outcomes.

python3 tools/foundation_baseline.py --only sdk,probe
python3 tools/foundation_baseline.py --list

Commands run sequentially. Failure does not hide later suite outcomes. Raw
artifacts default to ignored target/foundation-baseline/<UTC timestamp>/.
No source cleanup, reset, commit, or code generation writes are performed.
Package resolution and builds may update ignored tool caches/lockfiles.
"""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parent.parent


def capture(command, cwd=ROOT):
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True)
    return (result.stdout + result.stderr).strip()


def commands(dart, flutter):
    def package(path, tool, extra=()):
        return [(path, [tool, 'pub', 'get']),
                (path, [tool, 'analyze', '--fatal-infos']),
                (path, [tool, 'test', *extra])]

    suites = {
        'build-cli': [('.', ['cargo', 'build', '-p', 'fynkel_cli', '--locked'])],
        'clocks': [
            ('.', ['cargo', 'build', '--release', '-p', 'fynkel_cli', '--locked']),
            ('.', ['cargo', 'check', '--release', '--workspace', '--all-targets', '--locked']),
            ('.', ['cargo', 'test', '--release', '--workspace', '--locked']),
            ('.', ['cargo', 'fmt', '--all', '--check']),
            ('.', ['cargo', 'clippy', '--release', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings']),
            ('.', ['cargo', 'run', '--release', '-p', 'fynkel_cli', '--locked', '--', 'codegen', '--check']),
            ('packages/fynkel', [dart, 'analyze', '--fatal-infos']),
            ('packages/fynkel', [dart, 'test']),
            ('packages/fynkel_authoring', [dart, 'analyze', '--fatal-infos']),
            ('packages/fynkel_authoring', [dart, 'test']),
            ('packages/fynkel_flutter', [flutter, 'analyze', '--fatal-infos']),
            ('packages/fynkel_flutter', [flutter, 'test']),
            ('apps/editor', [flutter, 'analyze', '--fatal-infos']),
            ('apps/editor', [flutter, 'test', 'test/play_test.dart', 'test/play_document_test.dart', 'test/scene_view_test.dart', 'test/hierarchy_test.dart']),
            ('packages/fynkel', [dart, 'run', 'tool/foundation_probe.dart']),
        ],
        'identity-followup': [
            ('.', ['cargo', 'fmt', '--all', '--check']),
            ('.', ['cargo', 'clippy', '--release', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings']),
            ('.', ['cargo', 'run', '--release', '-p', 'fynkel_cli', '--locked', '--', 'codegen', '--check']),
            ('apps/editor', [flutter, 'test', 'test/play_test.dart', 'test/play_document_test.dart', 'test/scene_view_test.dart', 'test/hierarchy_test.dart']),
        ],
        'identity': [
            ('.', ['cargo', 'build', '--release', '-p', 'fynkel_cli', '--locked']),
            ('.', ['cargo', 'run', '--release', '-p', 'fynkel_cli', '--locked', '--', 'codegen', '--check']),
            ('.', ['cargo', 'check', '--release', '--workspace', '--all-targets', '--locked']),
            ('.', ['cargo', 'test', '--release', '--workspace', '--locked']),
            ('.', ['cargo', 'clippy', '--release', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings']),
            ('packages/fynkel', [dart, 'analyze', '--fatal-infos']),
            ('packages/fynkel', [dart, 'test']),
            ('packages/fynkel_authoring', [dart, 'analyze', '--fatal-infos']),
            ('packages/fynkel_authoring', [dart, 'test']),
            ('packages/fynkel_flutter', [flutter, 'analyze', '--fatal-infos']),
            ('packages/fynkel_flutter', [flutter, 'test']),
            ('apps/editor', [flutter, 'analyze', '--fatal-infos']),
            ('apps/editor', [flutter, 'test']),
            ('.', ['cargo', 'run', '--release', '-p', 'fynkel_host_headless', '--locked', '--', '--steps', '300', '--satellites', '1000', '--json']),
            ('.', ['cargo', 'run', '--release', '-p', 'fynkel_host_headless', '--locked', '--', '--steps', '300', '--satellites', '10000', '--json']),
        ],

        'rust': [('.', ['cargo', 'test', '--workspace', '--locked'])],
        'rust-lint': [('.', ['cargo', 'fmt', '--all', '--check']),
                      ('.', ['cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings'])],
        'stripped': [('.', ['cargo', 'test', '-p', 'fynkel_physics', '--no-default-features', '--locked']),
                     ('.', ['cargo', 'test', '-p', 'fynkel_core', '--no-default-features', '--test', 'modules', '--locked']),
                     ('.', ['sh', 'tools/check_runtime_deps.sh'])],
        'codegen': [('.', ['cargo', 'run', '-p', 'fynkel_cli', '--locked', '--', 'codegen', '--check'])],
        'sdk': package('packages/fynkel', dart),
        'generator': [('packages/fynkel_generator/example/spinner_game', [dart, 'pub', 'get']),
                      *package('packages/fynkel_generator', dart),
                      *package('packages/fynkel_generator/example/spinner_game', dart)],
        'authoring': package('packages/fynkel_authoring', dart),
        'flutter': package('packages/fynkel_flutter', flutter),
        'editor-api': package('packages/fynkel_editor_api', flutter),
        'editor': package('apps/editor', flutter),
        'hosts': [('hosts/flutter_apple', [flutter, 'pub', 'get']),
                  ('hosts/flutter_apple', [flutter, 'analyze', '--fatal-infos']),
                  ('.', ['cargo', 'test', '-p', 'fynkel_platform_apple', '--locked'])],
        'probe': [('packages/fynkel', [dart, 'run', 'tool/foundation_probe.dart'])],
        'prototype': [('packages/fynkel', [dart, 'pub', 'get']),
                      ('packages/fynkel', [dart, 'analyze', '--fatal-infos']),
                      ('packages/fynkel', [dart, 'test', 'test/foundation_prototype_test.dart']),
                      ('packages/fynkel', [dart, 'run', 'tool/foundation_prototype/run.dart'])],
        'prototype-native': [('.', ['cargo', 'run', '--release', '-p', 'fynkel_ecs', '--example', 'foundation_storage', '--locked', '--', str(count), '120'])
                             for count in (1000, 10000)],
        'prototype-bridge': [('.', ['cargo', 'build', '--release', '-p', 'fynkel_ecs', '--example', 'foundation_bridge', '--locked']),
                             ('packages/fynkel', [dart, 'pub', 'get']),
                             ('packages/fynkel', [dart, 'run', 'tool/foundation_prototype/native_storage.dart'])],
        'prototype-state': [('.', ['cargo', 'build', '--release', '-p', 'fynkel_core', '--no-default-features', '--example', 'foundation_state', '--locked']),
                            ('.', ['cargo', 'test', '--release', '-p', 'fynkel_core', '--no-default-features', '--example', 'foundation_state', '--locked']),
                            ('.', ['cargo', 'clippy', '--release', '-p', 'fynkel_core', '--no-default-features', '--example', 'foundation_state', '--example', 'foundation_state_bench', '--locked', '--', '-D', 'warnings']),
                            ('packages/fynkel', [dart, 'pub', 'get']),
                            ('packages/fynkel', [dart, 'analyze', '--fatal-infos']),
                            ('packages/fynkel', [dart, 'run', 'tool/foundation_prototype/mixed_contracts.dart']),
                            ('packages/fynkel', [dart, 'run', 'tool/foundation_prototype/mixed_bench.dart']),
                            ('.', ['cargo', 'run', '--release', '-p', 'fynkel_core', '--no-default-features', '--example', 'foundation_state_bench', '--locked'])],
        'foundation-contracts': [('.', ['cargo', 'test', '--release', '-p', 'fynkel_core', '--no-default-features', '--example', 'foundation_contracts', '--locked']),
                                 ('.', ['cargo', 'clippy', '--release', '-p', 'fynkel_core', '--no-default-features', '--example', 'foundation_contracts', '--locked', '--', '-D', 'warnings'])],
        'headless-release': [('.', ['cargo', 'run', '--release', '-p', 'fynkel_host_headless', '--locked', '--',
                                   '--steps', '3000', '--satellites', '24', '--json'])],
        'headless-scale': [('.', ['cargo', 'run', '--release', '-p', 'fynkel_host_headless', '--locked', '--',
                                 '--steps', '300', '--satellites', str(count), '--json'])
                           for count in (1000, 10000)],
    }

    suites['authority'] = [('.', ['cargo', 'build', '--release', '-p', 'fynkel_native', '--locked'])] + list(suites['clocks']) + [('.', ['cargo', 'test', '--release', '-p', 'fynkel_native', '--test', 'identity_libraries', '--', '--ignored', '--nocapture'])]
    suites['transforms'] = list(suites['clocks'])
    suites['gameplay'] = [
        ('.', ['cargo', 'check', '--release', '--workspace', '--all-targets', '--locked']),
        ('.', ['cargo', 'test', '--release', '--workspace', '--locked']),
        ('.', ['cargo', 'test', '--release', '-p', 'fynkel_core', '--no-default-features', '--test', 'gameplay', '--test', 'gameplay_structure', '--test', 'gameplay_components', '--test', 'gameplay_references', '--test', 'gameplay_prefab', '--test', 'gameplay_effects', '--test', 'gameplay_audio_bus', '--test', 'gameplay_input', '--test', 'gameplay_lifetime', '--test', 'gameplay_emitter', '--test', 'gameplay_native_poses', '--test', 'gameplay_native_stages', '--test', 'apply_now', '--test', 'gameplay_live', '--test', 'gameplay_clone', '--test', 'gameplay_presentation', '--test', 'modules', '--locked']),
        ('.', ['cargo', 'fmt', '--all', '--check']),
        ('.', ['cargo', 'clippy', '--release', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings']),
        ('.', ['cargo', 'run', '--release', '-p', 'fynkel_cli', '--locked', '--', 'codegen', '--check']),
    ]
    suites['gameplay-bridge'] = list(suites['gameplay']) + [
        ('.', ['cargo', 'test', '--release', '-p', 'fynkel_native', '--no-default-features', '--lib', 'gameplay', '--locked']),
        ('packages/fynkel', [dart, 'analyze', '--fatal-infos']),
        ('packages/fynkel', [dart, 'test']),
    ]
    suites['gameplay-sdk'] = list(suites['gameplay-bridge']) + [
        ('packages/fynkel_authoring', [dart, 'analyze', '--fatal-infos']),
        ('packages/fynkel_authoring', [dart, 'test']),
        ('packages/fynkel_flutter', [flutter, 'analyze', '--fatal-infos']),
        ('packages/fynkel_flutter', [flutter, 'test']),
        ('apps/editor', [flutter, 'analyze', '--fatal-infos']),
        ('apps/editor', [flutter, 'test', 'test/play_test.dart', 'test/play_document_test.dart', 'test/scene_view_test.dart', 'test/hierarchy_test.dart']),
    ]
    suites['gameplay-sdk'] += [
        ('.', ['cargo', 'test', '--release', '-p', 'fynkel_assets', '--no-default-features', '--test', 'prepared_prefab', '--locked']),
        ('.', ['cargo', 'test', '--release', '-p', 'fynkel_physics', '--no-default-features', '--test', 'prepared_mesh', '--locked']),
    ]
    suites['bounds'] = list(suites['clocks']) + [('.', ['cargo', 'test', '--release', '-p', 'fynkel_render', '--lib', 'benchmark_animated_bounds', '--', '--ignored', '--nocapture'])]
    return suites


def run_command(command, cwd, logfile, timeout, env):
    started = time.monotonic()
    try:
        with logfile.open('w') as log:
            process = subprocess.Popen(command, cwd=cwd, env=env, stdout=log,
                                       stderr=subprocess.STDOUT, start_new_session=True)
            try:
                code = process.wait(timeout=timeout)
                state = 'passed' if code == 0 else 'failed'
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
                code, state = process.returncode, 'timed-out'
            except KeyboardInterrupt:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
                code, state = process.returncode, 'interrupted'
    except OSError as error:
        logfile.write_text(str(error) + '\n')
        code, state = None, 'could-not-start'
    return {'state': state, 'exit_code': code, 'seconds': round(time.monotonic() - started, 3)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--only', help='Comma-separated suite names; default all')
    parser.add_argument('--list', action='store_true')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--timeout', type=int, default=1800, help='Per-command seconds')
    parser.add_argument('--flutter', default=shutil.which('flutter'))
    args = parser.parse_args()
    if not args.flutter:
        parser.error('Flutter is required; pass --flutter /path/to/flutter')
    flutter = str(Path(args.flutter).resolve())
    dart = str(Path(flutter).with_name('dart'))
    suites = commands(dart, flutter)
    if args.list:
        print('\n'.join(suites))
        return 0
    selected = list(suites) if args.only is None else args.only.split(',')
    if len(selected) != len(set(selected)) or any(name not in suites for name in selected):
        parser.error('--only must contain unique listed suite names')
    if args.timeout <= 0:
        parser.error('--timeout must be positive')
    # Package tests invoke the CLI. Resolve that dependency even for --only.
    if any(name in selected for name in ('sdk', 'generator', 'authoring', 'editor', 'probe')):
        selected = ['build-cli', *[name for name in selected if name != 'build-cli']]
    timestamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
    output = args.output or ROOT / 'target/foundation-baseline' / timestamp
    output.mkdir(parents=True, exist_ok=False)
    env = os.environ.copy()
    env['PATH'] = str(Path(flutter).parent) + os.pathsep + env.get('PATH', '')
    # Tests which cook must use a CLI from this checkout. SDK native hooks
    # still build and check their own matching native code asset.
    env['FYNKEL_CLI'] = str(ROOT / ('target/release/fynkel' if any(name.startswith(('identity', 'clocks', 'transforms', 'bounds', 'authority', 'gameplay')) for name in selected) else 'target/debug/fynkel'))
    metadata = {
        'started_utc': timestamp,
        'revision': capture(['git', 'rev-parse', 'HEAD']),
        'working_tree': capture(['git', 'status', '--short']),
        'tracked_diff_sha256': hashlib.sha256(
            subprocess.check_output(['git', 'diff', '--binary', 'HEAD'], cwd=ROOT)).hexdigest(),
        'untracked_sha256': {
            name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
            for name in subprocess.check_output(
                ['git', 'ls-files', '--others', '--exclude-standard', '-z'], cwd=ROOT
            ).decode().split('\0') if name and (ROOT / name).is_file()
        },
        'machine': {'platform': platform.platform(), 'architecture': platform.machine(),
                    'cpu': capture(['sysctl', '-n', 'machdep.cpu.brand_string']) if platform.system() == 'Darwin' else platform.processor()},
        'tools': {'cargo': capture(['cargo', '--version']), 'dart': capture([dart, '--version']),
                  'flutter': capture([flutter, '--version'])},
        'selected_suites': selected,
        'results': [],
    }
    report = output / 'report.json'
    report.write_text(json.dumps(metadata, indent=2) + '\n')
    print(f'Artifacts: {output}', flush=True)
    interrupted = False
    for suite in selected:
        for index, (cwd, command) in enumerate(suites[suite]):
            log = output / f'{suite}-{index}.log'
            print(f'[{suite}] {" ".join(command)}', flush=True)
            result = run_command(command, ROOT / cwd, log, args.timeout, env)
            metadata['results'].append({'suite': suite, 'cwd': cwd, 'command': command,
                                        'log': log.name, **result})
            report.write_text(json.dumps(metadata, indent=2) + '\n')
            print(f'[{suite}] {result["state"]} ({result["seconds"]}s)', flush=True)
            if result['state'] != 'passed' and not (suite in ('identity', 'clocks', 'transforms', 'bounds', 'authority', 'gameplay', 'gameplay-bridge', 'gameplay-sdk') and index > 0 and result['state'] != 'interrupted'):
                # Remaining commands in this suite may rely on setup/build.
                interrupted = result['state'] == 'interrupted'
                break
        if interrupted:
            break
    metadata['finished_utc'] = datetime.now(timezone.utc).isoformat()
    report.write_text(json.dumps(metadata, indent=2) + '\n')
    return int(any(item['state'] != 'passed' for item in metadata['results']))


if __name__ == '__main__':
    raise SystemExit(main())
