#!/usr/bin/env python3
"""Bounded iPhone diagnostic; stops only the PID returned by this launch."""
import argparse
import hashlib
import json
import plistlib
import signal
from pathlib import Path
import subprocess
import time
import uuid


def launched_pid(doc):
    pid = doc['result']['process']['processIdentifier']
    if isinstance(pid, bool) or not isinstance(pid, int) or pid <= 0:
        raise ValueError('launch did not return a positive process identifier')
    return pid


def capture(device, bundle, app, out, trace_gpu=False, trace_frames=False, freeze_scene=False, acquire_wait_us=3000, buffer_count=4, resolution_scale=1.0, cpu_ticks=False, quality='medium', metal_trace_device=None, trace_physics=False, pipelined=False, frame_queue=1):
    with (app / 'Info.plist').open('rb') as file:
        actual_bundle = plistlib.load(file)['CFBundleIdentifier']
    if actual_bundle != bundle:
        raise ValueError(f'bundle mismatch: app is {actual_bundle}, requested {bundle}')
    out.mkdir(parents=True, exist_ok=False)
    capture_id = uuid.uuid4().hex
    trace_path = f'Documents/screen-space-{capture_id}.log'
    frame_path = f'Documents/frames-{capture_id}.csv'
    physics_path = f'Documents/physics-{capture_id}.csv'
    evidence = {'captureId': capture_id, 'device': device, 'bundle': bundle, 'app': str(app.resolve()),
                'status': 'running', 'sustainedQualification': False, 'traceGpu': trace_gpu, 'traceFrames': trace_frames, 'tracePhysics': trace_physics, 'metalTraceDevice': metal_trace_device, 'commands': []}
    evidence['executableSha256'] = hashlib.sha256((app / 'Runner').read_bytes()).hexdigest()
    evidence['frameworkSha256'] = {str(binary.relative_to(app)): hashlib.sha256(binary.read_bytes()).hexdigest()
        for framework in sorted((app / 'Frameworks').glob('*.framework'))
        if (binary := framework / framework.stem).is_file()}

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
    profiler = None
    profiler_log = None
    try:
        run(['device', 'install', 'app', '--device', device, app], timeout=120)
        request = out / 'capture-request.json'
        request.write_text(json.dumps({'captureId': capture_id, 'awaitProfiler': bool(metal_trace_device), 'timeline': trace_frames, 'freezeScene': freeze_scene, 'acquireWaitUs': acquire_wait_us, 'bufferCount': buffer_count, 'resolutionScale': resolution_scale, 'cpuTicks': cpu_ticks, 'quality': quality, 'pipelined': pipelined, 'frameQueue': frame_queue}) + '\n')
        run(['device', 'copy', 'to', '--device', device, '--domain-type', 'appDataContainer',
             '--domain-identifier', bundle, '--source', request,
             '--destination', 'Documents/capture-request.json'])
        launch = out / 'launch.json'
        variables = {}
        if trace_gpu:
            variables['FYNKEL_TRACE_SCREEN_SPACE_FILE'] = trace_path
        # devicectl rejects {}; omit the option when tracing needs no variables.
        if trace_frames:
            variables['FYNKEL_TRACE_FRAME_FILE'] = frame_path
        if trace_physics:
            variables['FYNKEL_TRACE_PHYSICS_FILE'] = physics_path
        environment = ['--environment-variables', json.dumps(variables)] if variables else []
        run(['device', 'process', 'launch', '--device', device, '--terminate-existing',
             '--json-output', launch, *environment, bundle])
        pid = launched_pid(json.loads(launch.read_text()))
        evidence['pid'] = pid
        save()
        if metal_trace_device:
            profiler_log = (out / 'metal-trace.log').open('w')
            command = ['xcrun', 'xctrace', 'record', '--template', 'Metal System Trace',
                       '--device', metal_trace_device, '--attach', str(pid),
                       '--time-limit', '8s', '--output', str(out / 'metal.trace'), '--no-prompt']
            evidence['metalTraceCommand'] = command
            profiler = subprocess.Popen(command, stdout=profiler_log, stderr=subprocess.STDOUT)
            save()
            wait_profiler_ready(profiler, out / 'metal-trace.log')
            signal_file = out / 'capture-go.json'
            signal_file.write_text(json.dumps({'captureId': capture_id}) + '\n')
            run(['device', 'copy', 'to', '--device', device, '--domain-type', 'appDataContainer',
                 '--domain-identifier', bundle, '--source', signal_file,
                 '--destination', f'Documents/capture-go-{capture_id}.json'])
        time.sleep(12)  # app's eight-second deadline plus logging/startup grace
        # Stop before copying evidence, so a slow copy cannot extend GPU load.
        run(['device', 'process', 'terminate', '--device', device, '--pid', pid], timeout=15)
        evidence['terminatedExactPid'] = pid
        pid = None
        if profiler is not None:
            evidence['metalTraceExitCode'] = profiler.wait(timeout=45)
            if profiler.returncode != 0:
                raise RuntimeError('Metal trace failed; see metal-trace.log')
        run(['device', 'copy', 'from', '--device', device, '--domain-type', 'appDataContainer',
             '--domain-identifier', bundle, '--source', 'Documents/mobile-capture.jsonl',
             '--destination', out / 'mobile-capture.jsonl'])
        rows = [json.loads(line) for line in (out / 'mobile-capture.jsonl').read_text().splitlines()]
        evidence['records'] = rows
        validate_capture_rows(rows, capture_id)
        expected_options = {'awaitProfiler': bool(metal_trace_device), 'timeline': trace_frames, 'frozenScene': freeze_scene,
                            'acquireWaitUs': acquire_wait_us, 'bufferCount': buffer_count,
                            'resolutionScale': resolution_scale, 'cpuTickReadback': cpu_ticks, 'quality': quality,
                            'pipelined': pipelined, 'frameQueue': frame_queue}
        if any(rows[0].get(key) != value for key, value in expected_options.items()):
            raise RuntimeError('app did not acknowledge the requested diagnostic options')
        if any(row.get('quality') != quality or row.get('qualityVerified') is not True
               for row in rows if row.get('event') == 'stats'):
            raise RuntimeError('quality profile was not verified against the attached asset store')
        if trace_gpu:
            run(['device', 'copy', 'from', '--device', device, '--domain-type', 'appDataContainer',
                 '--domain-identifier', bundle, '--source', trace_path,
                 '--destination', out / 'screen-space.log'])
            evidence['screenSpaceSamples'] = parse_screen_space((out / 'screen-space.log').read_text())
        if trace_frames:
            run(['device', 'copy', 'from', '--device', device, '--domain-type', 'appDataContainer',
                 '--domain-identifier', bundle, '--source', frame_path,
                 '--destination', out / 'frames.csv'])
        if trace_physics:
            run(['device', 'copy', 'from', '--device', device, '--domain-type', 'appDataContainer',
                 '--domain-identifier', bundle, '--source', physics_path,
                 '--destination', out / 'physics.csv'])
            evidence['physicsSamples'] = parse_physics((out / 'physics.csv').read_text())
        if metal_trace_device and not any(row.get('event') == 'profiler-ready' and row.get('captureId') == capture_id for row in rows):
            raise RuntimeError('app did not acknowledge the profiler start signal')
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
        try:
            if profiler is not None:
                stop_profiler(profiler)
                evidence['metalTraceExitCode'] = profiler.returncode
        except Exception as error:
            evidence['profilerTerminationError'] = str(error)
            evidence['status'] = 'failed'
            raise
        finally:
            if profiler_log is not None:
                profiler_log.close()
            save()
    print(out / 'capture.json')


def wait_profiler_ready(process, log, timeout=30):
    """Bound attachment while the app waits idle, before its eight-second load."""
    deadline = time.monotonic() + timeout
    while True:
        if process.poll() is not None:
            raise RuntimeError('Metal recorder exited before workload start; see metal-trace.log')
        if 'Ctrl-C to stop the recording' in log.read_text():
            return
        if time.monotonic() >= deadline:
            raise TimeoutError('Metal recorder did not become ready before workload start')
        time.sleep(0.1)


def stop_profiler(process):
    """Bound cleanup to this exact child, including recorder startup failures."""
    if process.poll() is not None:
        return
    try:
        process.send_signal(signal.SIGINT)
    except ProcessLookupError:
        process.wait(timeout=5)
        return
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


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


def parse_physics(text):
    import csv
    import io
    header = ['world', 'tick', 'ran', 'sync_ns', 'commands_ns', 'characters_ns',
              'solver_ns', 'writeback_ns', 'events_ns', 'total_ns',
              'collect_ns', 'hierarchy_ns', 'classify_ns', 'apply_ns',
              'propagation_ns', 'core_adapter_ns', 'publication_ns', 'core_total_ns']
    reader = csv.DictReader(io.StringIO(text))
    if reader.fieldnames != header or not text.endswith('\n'):
        raise ValueError('invalid or truncated physics trace header/record')
    rows = []
    ticks = {}
    for raw in reader:
        if set(raw) != set(header) or any(v is None or not v.isdigit() for v in raw.values()):
            raise ValueError('invalid physics trace row')
        row = {k: int(v) for k, v in raw.items()}
        world, tick = row['world'], row['tick']
        if world == 0 or tick <= ticks.get(world, 0) or row['ran'] not in [0, 1]:
            raise ValueError('invalid world, tick order or phase state')
        ticks[world] = tick
        if sum(row[k] for k in header[3:9]) > row['total_ns']:
            raise ValueError('physics phases exceed total')
        if sum(row[k] for k in header[10:14]) > row['sync_ns']:
            raise ValueError('synchronization subphases exceed sync total')
        if sum(row[k] for k in header[14:17]) > row['core_total_ns'] or row['total_ns'] > row['core_adapter_ns']:
            raise ValueError('invalid core physics phase containment')
        if not row['ran'] and any(row[k] for k in header[3:]):
            raise ValueError('skipped physics step has stale timing')
        rows.append(row)
    if not any(row['ran'] for row in rows) or len(rows) >= 8192:
        raise ValueError('missing physics samples or saturated trace')
    return rows


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--device', required=True)
    parser.add_argument('--bundle', required=True)
    parser.add_argument('--app', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    parser.add_argument('--quality', choices=['medium', 'medium-no-ao', 'medium-no-contact', 'medium-no-screen-space'], default='medium')
    parser.add_argument('--metal-trace-device', help='Physical-device UDID from xctrace list devices; adds an eight-second attached Metal trace, with profiler overhead')
    parser.add_argument('--trace-physics', action='store_true', help='Bounded per-tick native physics phase trace; adds diagnostic overhead')
    parser.add_argument('--trace-gpu', action='store_true')
    parser.add_argument('--cpu-ticks', action='store_true')
    parser.add_argument('--trace-frames', action='store_true')
    parser.add_argument('--freeze-scene', action='store_true')
    parser.add_argument('--acquire-wait-us', type=int, choices=[0, 3000], default=3000)
    parser.add_argument('--buffer-count', type=int, choices=[4, 5, 6, 8], default=4)
    parser.add_argument('--pipelined', action='store_true', help='Render on the native worker (the game view default)')
    parser.add_argument('--frame-queue', type=int, choices=[1, 2, 3], default=1, help='Frames the compositor shows in order before skipping to the newest; 2 needs --buffer-count 5')
    parser.add_argument('--resolution-scale', type=float, choices=[0.5, 0.75, 1.0], default=1.0)
    args = parser.parse_args()
    capture(args.device, args.bundle, args.app, args.out.resolve(), trace_gpu=args.trace_gpu, trace_frames=args.trace_frames, freeze_scene=args.freeze_scene, acquire_wait_us=args.acquire_wait_us, buffer_count=args.buffer_count, resolution_scale=args.resolution_scale, cpu_ticks=args.cpu_ticks, quality=args.quality, metal_trace_device=args.metal_trace_device, trace_physics=args.trace_physics, pipelined=args.pipelined, frame_queue=args.frame_queue)
