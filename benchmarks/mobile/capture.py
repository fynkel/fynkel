#!/usr/bin/env python3
"""Bounded iPhone diagnostic; stops only the PID returned by this launch."""
import argparse
import hashlib
import json
import plistlib
from pathlib import Path
import subprocess
import time


def launched_pid(doc):
    pid = doc['result']['process']['processIdentifier']
    if isinstance(pid, bool) or not isinstance(pid, int) or pid <= 0:
        raise ValueError('launch did not return a positive process identifier')
    return pid


def capture(device, bundle, app, out):
    with (app / 'Info.plist').open('rb') as file:
        actual_bundle = plistlib.load(file)['CFBundleIdentifier']
    if actual_bundle != bundle:
        raise ValueError(f'bundle mismatch: app is {actual_bundle}, requested {bundle}')
    out.mkdir(parents=True, exist_ok=False)
    evidence = {'device': device, 'bundle': bundle, 'app': str(app.resolve()),
                'status': 'running', 'sustainedQualification': False, 'commands': []}
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
        launch = out / 'launch.json'
        run(['device', 'process', 'launch', '--device', device, '--terminate-existing',
             '--json-output', launch, bundle])
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
        if not rows or rows[-1].get('event') != 'stop':
            raise RuntimeError('missing terminal stop record; capture is incomplete')
        if not any(row.get('event') == 'stats' for row in rows):
            raise RuntimeError('capture has no frame measurements')
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


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--device', required=True)
    parser.add_argument('--bundle', required=True)
    parser.add_argument('--app', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    capture(args.device, args.bundle, args.app, args.out.resolve())
