#!/usr/bin/env python3
"""Bounded iPhone diagnostic; stops only the PID returned by this launch."""
import argparse
import hashlib
import json
import plistlib
from pathlib import Path
import subprocess
import time
import uuid


def launched_pid(doc):
    pid = doc['result']['process']['processIdentifier']
    if isinstance(pid, bool) or not isinstance(pid, int) or pid <= 0:
        raise ValueError('launch did not return a positive process identifier')
    return pid


def capture(device, bundle, app, out, trace_gpu=False):
    with (app / 'Info.plist').open('rb') as file:
        actual_bundle = plistlib.load(file)['CFBundleIdentifier']
    if actual_bundle != bundle:
        raise ValueError(f'bundle mismatch: app is {actual_bundle}, requested {bundle}')
    out.mkdir(parents=True, exist_ok=False)
    capture_id = uuid.uuid4().hex
    trace_path = f'Documents/screen-space-{capture_id}.log'
    evidence = {'captureId': capture_id, 'device': device, 'bundle': bundle, 'app': str(app.resolve()),
                'status': 'running', 'sustainedQualification': False, 'traceGpu': trace_gpu, 'commands': []}
    evidence['executableSha256'] = hashlib.sha256((app / 'Runner').read_bytes()).hexdigest()

    def save():
        (out / 'capture.json').write_text(json.dumps(evidence, indent=2) + '\n')

    def run(args, timeout=60):
        cmd = ['xcrun', 'devicectl', *map(str, args)]
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        evidence['commands'].append({'argv': cmd, 'exitCode': result.returncode,
                                     'stdout': result.stdout, 'stderr': result.stderr})
        save()
        result.check_returncode()

    pid = None
    try:
        run(['device', 'install', 'app', '--device', device, app], timeout=120)
        request = out / 'capture-request.json'
        request.write_text(json.dumps({'captureId': capture_id}) + '\n')
        run(['device', 'copy', 'to', '--device', device, '--domain-type', 'appDataContainer',
             '--domain-identifier', bundle, '--source', request,
             '--destination', 'Documents/capture-request.json'])
        launch = out / 'launch.json'
        variables = {}
        if trace_gpu:
            variables['FYNKEL_TRACE_SCREEN_SPACE_FILE'] = trace_path
        # devicectl rejects {}; omit the option when tracing needs no variables.
        environment = ['--environment-variables', json.dumps(variables)] if variables else []
        run(['device', 'process', 'launch', '--device', device, '--terminate-existing',
             '--json-output', launch, *environment, bundle])
        pid = launched_pid(json.loads(launch.read_text()))
        evidence['pid'] = pid
        save()
        time.sleep(12)  # app's eight-second deadline plus logging/startup grace
        # Stop before copying evidence, so a slow copy cannot extend GPU load.
        run(['device', 'process', 'terminate', '--device', device, '--pid', pid], timeout=15)
        evidence['terminatedExactPid'] = pid
        pid = None
        run(['device', 'copy', 'from', '--device', device, '--domain-type', 'appDataContainer',
             '--domain-identifier', bundle, '--source', 'Documents/mobile-capture.jsonl',
             '--destination', out / 'mobile-capture.jsonl'])
        rows = [json.loads(line) for line in (out / 'mobile-capture.jsonl').read_text().splitlines()]
        evidence['records'] = rows
        validate_capture_rows(rows, capture_id)
        if trace_gpu:
            run(['device', 'copy', 'from', '--device', device, '--domain-type', 'appDataContainer',
                 '--domain-identifier', bundle, '--source', trace_path,
                 '--destination', out / 'screen-space.log'])
            evidence['screenSpaceSamples'] = parse_screen_space((out / 'screen-space.log').read_text())
        reason = rows[-1].get('reason')
        evidence['status'] = 'diagnostic-complete' if reason == 'deadline' else 'interrupted'
    except Exception as error:
        evidence.update(status='failed', error=str(error))
        raise
    finally:
        if pid is not None:
            try:
                run(['device', 'process', 'terminate', '--device', device, '--pid', pid])
                evidence['terminatedExactPid'] = pid
            except Exception as error:
                evidence['terminationError'] = str(error)
                evidence['status'] = 'failed'
        save()
    print(out / 'capture.json')


def validate_capture_rows(rows, capture_id):
    if not rows or rows[0].get('event') != 'start' or rows[0].get('captureId') != capture_id:
        raise RuntimeError('capture log does not belong to this launch')
    if rows[-1].get('event') != 'stop':
        raise RuntimeError('missing terminal stop record; capture is incomplete')
    if not any(row.get('event') == 'stats' for row in rows):
        raise RuntimeError('capture has no frame measurements')


def parse_screen_space(text):
    import re
    pattern = re.compile(r'screen_space_us sample=(\d+) depth=(\d+) resolve=(\d+) estimate=(\d+) blur=(\d+) total=(\d+)')
    rows = []
    previous = 0
    for line in text.splitlines():
        match = pattern.fullmatch(line)
        if not match:
            raise ValueError('malformed screen-space trace row')
        sample, depth, resolve, estimate, blur, total = map(int, match.groups())
        if sample <= previous:
            raise ValueError('screen-space trace samples must increase')
        previous = sample
        rows.append(dict(sample=sample, depthUs=depth, resolveUs=resolve, estimateUs=estimate, blurUs=blur, totalUs=total))
    if not rows:
        raise ValueError('no screen-space GPU samples')
    return rows


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--device', required=True)
    parser.add_argument('--bundle', required=True)
    parser.add_argument('--app', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    parser.add_argument('--trace-gpu', action='store_true')
    args = parser.parse_args()
    capture(args.device, args.bundle, args.app, args.out.resolve(), trace_gpu=args.trace_gpu)
