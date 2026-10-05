#!/usr/bin/env python3
"""Correlate bounded host/native/Flutter frame traces; never a sustained FPS gate."""
import argparse
import csv
import json
import statistics
from collections import Counter, defaultdict
from pathlib import Path


def distribution(values):
    values = sorted(values)
    if not values:
        return None
    def percentile(p):
        return values[min(len(values) - 1, int((len(values) - 1) * p))]
    return dict(count=len(values), mean=statistics.mean(values), p50=statistics.median(values),
                p95=percentile(.95), p99=percentile(.99), maximum=values[-1])


def read_trace(path):
    with path.open() as file:
        reader = csv.DictReader(file)
        expected = 'time_us,event,generation,seq,slot,free,rendering,ready,presented,retiring,a,b,c'.split(',')
        if reader.fieldnames != expected:
            raise ValueError('unknown frame trace schema')
        rows = []
        allowed = {'reserve', 'submit_begin', 'submitted', 'ready', 'notified', 'drop',
                   'acquire_call', 'acquire_begin', 'acquire_new', 'acquire_repeat', 'stages',
                   'prepare_0', 'prepare_1', 'prepare_2', 'visibility', 'visibility_update'}
        for row in reader:
            if None in row or any(v is None for v in row.values()) or row['event'] not in allowed:
                raise ValueError('malformed frame trace row')
            rows.append({k: v if k == 'event' else int(v) for k, v in row.items()})
        if not rows or len(rows) >= 16384:
            raise ValueError('empty or potentially truncated frame trace')
        return sorted(rows, key=lambda r: r['time_us'])


def correlate_cpu(host, records):
    """Join every CPU frame/tick to its host frame, without averaging catch-up away."""
    frames = [r['samples'] for r in records if r['event'] == 'cpuFrames']
    ticks = [r['samples'] for r in records if r['event'] == 'cpuTicks']
    if not frames and not ticks:
        return None
    if len(frames) != 1 or len(ticks) != 1 or len(frames[0]) != len(host):
        raise ValueError('CPU/host trace batches do not match')
    by_frame = defaultdict(list)
    for tick in ticks[0]:
        index = tick['frameIndex']
        if type(index) is not int or not 0 <= index < len(host):
            raise ValueError('CPU tick refers to an unknown host frame')
        by_frame[index].append(tick)
    groups = defaultdict(list)
    windows = defaultdict(list)
    all_rows = []
    for index, (h, c) in enumerate(zip(host, frames[0])):
        if type(h['steps']) is not int or h['steps'] < 0 or h['frame'] != index or h['steps'] != c['ticks']:
            raise ValueError('CPU/host frame identity or step count mismatch')
        if index and h['tick'] - h['steps'] != host[index-1]['tick']:
            raise ValueError('completed host tick sequence is discontinuous')
        ts = by_frame[index]
        expected_ticks = list(range(h['tick'] - h['steps'], h['tick']))
        if [t['tick'] for t in ts] != expected_ticks:
            raise ValueError('CPU ticks do not match completed host tick range')
        native_steps = []
        for t in ts:
            step = [v['micros'] for v in t['systems'] if v['system'] == 'step']
            if len(step) != 1 or step[0] < 0 or t['readbackUs'] < 0:
                raise ValueError('missing, duplicate or negative native step timing')
            native_steps.append(step[0])
        row = dict(steps=h['steps'],
                   simulationUs=h['simulationEndUs']-h['startUs'],
                   renderUs=h['endUs']-h['renderStartUs'],
                   workUs=h['endUs']-h['startUs'],
                   nativeStepUs=sum(native_steps), nativeHostUs=c['nativeUs'],
                   behaviourHostUs=c['behavioursUs'],
                   probeUs=c['readbackUs']+sum(t['readbackUs'] for t in ts))
        groups[h['steps']].append(row)
        windows[(h['startUs']-host[0]['startUs'])//1_000_000].append(row)
        all_rows.append(row)
    fields = ['simulationUs','renderUs','workUs','nativeStepUs','nativeHostUs','behaviourHostUs','probeUs']
    def summarize(rows):
        return dict(frames=len(rows), ticks=sum(r['steps'] for r in rows),
                    ticksPerFrame=dict(Counter(r['steps'] for r in rows)),
                    durations={key:distribution([r[key] for r in rows]) for key in fields},
                    totalsMs={key:sum(r[key] for r in rows)/1000 for key in fields})
    return dict(scope='Same-ordinal host/CPU frames and exact completed tick ranges. Native steps are nested within native host and simulation time; totals are not additive. Probe cost is diagnostic overhead; clock/logger overhead is not fully isolated.',
                total=summarize(all_rows),
                bySteps={str(n):summarize(rows) for n,rows in sorted(groups.items())},
                windows=[dict(second=n,**summarize(rows)) for n,rows in sorted(windows.items())])


def correlate_prepare(host, native):
    """Require complete phase groups, each inside exactly one host render call."""
    names = ['setup', 'cameraEnvironment', 'visibility', 'lightPool', 'instances',
             'batchUploadsOutline', 'shadowPostUniforms', 'overlay', 'readinessAccounting']
    phase_events = {'prepare_0', 'prepare_1', 'prepare_2'}
    visibility_enabled = any(r['event'] == 'visibility' for r in native)
    update_enabled = any(r['event'] == 'visibility_update' for r in native)
    if update_enabled and not visibility_enabled:
        raise ValueError('update classification requires visibility timing')
    expected_events = phase_events | {'stages'} | ({'visibility'} if visibility_enabled else set()) | ({'visibility_update'} if update_enabled else set())
    if not visibility_enabled and not any(r['event'] in phase_events for r in native):
        return None  # Older captures have no phase instrumentation.
    grouped = defaultdict(dict)
    index = 0
    for row in native:
        if row['event'] not in expected_events:
            continue
        while index < len(host) and host[index]['endUs'] < row['time_us']:
            index += 1
        if index == len(host) or not host[index]['renderStartUs'] <= row['time_us'] <= host[index]['endUs']:
            raise ValueError('preparation timing has no matching host render interval')
        if row['event'] in grouped[index]:
            raise ValueError('duplicate preparation timing in host frame')
        grouped[index][row['event']] = row
    samples = []
    visibility_samples = []
    update_samples = defaultdict(list)
    update_names = ['unchanged', 'membershipRebuild', 'sparseRefit', 'denseRefit', 'costRebuild', 'membershipRepair', 'depthRebuild']
    remainder = []
    for group in grouped.values():
        if set(group) != expected_events:
            raise ValueError('incomplete preparation phase group')
        values = [group[f'prepare_{i}'][col] for i in range(3) for col in ['a','b','c']]
        if min(values) < 0 or sum(values) > group['stages']['b']:
            raise ValueError('invalid preparation phase durations')
        if visibility_enabled:
            detail = [group['visibility'][col] for col in ['a','b','c']]
            if min(detail) < 0 or sum(detail) > values[2]:
                raise ValueError('invalid visibility phase durations')
            visibility_samples.append(detail)
            if update_enabled:
                kind = group['visibility_update']['a']
                if kind not in range(len(update_names)) or group['visibility_update']['b'] or group['visibility_update']['c']:
                    raise ValueError('unknown visibility update classification')
                update_samples[update_names[kind]].append(detail[1])
        samples.append(values)
        remainder.append(group['stages']['b'] - sum(values))
    return dict(scope='Disjoint elapsed CPU phases, nested inside prepare; includes upload calls and waits, not GPU execution. Medians are not additive.',
                frames=len(samples),
                visibilityUpdateKinds={name:dict(durationsUs=distribution(values), totalMs=sum(values)/1000)
                    for name,values in update_samples.items()} if update_enabled else None,
                visibilityPhasesUs={name:distribution([v[i] for v in visibility_samples])
                    for i,name in enumerate(['bounds', 'indexUpdate', 'queries'])} if visibility_enabled else None,
                visibilityTotalsMs={name:sum(v[i] for v in visibility_samples)/1000
                    for i,name in enumerate(['bounds', 'indexUpdate', 'queries'])} if visibility_enabled else None,
                outerRemainderUs=distribution(remainder),
                outerRemainderTotalMs=sum(remainder)/1000,
                phasesUs={name:distribution([v[i] for v in samples]) for i,name in enumerate(names)},
                totalsMs={name:sum(v[i] for v in samples)/1000 for i,name in enumerate(names)})


def analyze(directory):
    capture = json.loads((directory / 'capture.json').read_text())
    if capture['status'] != 'diagnostic-complete':
        raise ValueError('capture did not complete its diagnostic deadline')
    records = capture['records']
    if records[0]['captureId'] != capture['captureId']:
        raise ValueError('capture ID mismatch')
    host = next(r['samples'] for r in records if r['event'] == 'hostFrames')
    flutter = next(r['samples'] for r in records if r['event'] == 'flutterFrames')
    if not host or len(host) >= 4096 or len(flutter) >= 4096:
        raise ValueError('missing or potentially truncated host trace')
    for frame in host:
        times = [frame[k] for k in ['startUs', 'simulationEndUs', 'renderStartUs', 'endUs']]
        if times != sorted(times):
            raise ValueError('host clock discontinuity')
    start, end = host[0]['startUs'], host[-1]['endUs']
    seconds = (end - start) / 1e6
    native_all = read_trace(directory / 'frames.csv')
    native = [r for r in native_all if start <= r['time_us'] <= end]
    events = defaultdict(list)
    for row in native:
        events[row['event']].append(row)
    if not events['submitted'] or not events['acquire_new']:
        raise ValueError('no correlated rendering/acquisition evidence')
    by_seq = defaultdict(dict)
    for row in native_all:
        if row['event'] in {'reserve', 'submit_begin', 'submitted', 'ready', 'notified', 'acquire_new'}:
            key = (row['generation'], row['seq'])
            if row['event'] in by_seq[key]:
                raise ValueError('duplicate per-frame event')
            by_seq[key][row['event']] = row
    def spans(a, b):
        result = []
        for row in events[a]:
            other = by_seq[(row['generation'], row['seq'])].get(b)
            if other:
                span = other['time_us'] - row['time_us']
                if span < 0:
                    raise ValueError('native clock/event order discontinuity')
                result.append(span)
        return result
    # Match every native attempt to its enclosing synchronous Dart render call.
    attempts = sorted(events['reserve'] + events['drop'], key=lambda r: r['time_us'])
    matched = []; index = 0
    for row in attempts:
        while index < len(host) and host[index]['endUs'] < row['time_us']:
            index += 1
        if index == len(host) or not host[index]['renderStartUs'] <= row['time_us'] <= host[index]['endUs']:
            raise ValueError('native attempt has no matching host render interval')
        matched.append((row, host[index]))
    request_to_acquisition = []
    for row, frame in matched:
        if row['event'] != 'reserve':
            continue
        acquired_frame = by_seq[(row['generation'], row['seq'])].get('acquire_new')
        if acquired_frame:
            age = acquired_frame['time_us'] - frame['renderStartUs']
            if age < 0:
                raise ValueError('acquisition predates its render request')
            request_to_acquisition.append(age)
    dropped = [f for r, f in matched if r['event'] == 'drop']
    acquired = events['acquire_new']
    intervals = [b['time_us'] - a['time_us'] for a, b in zip(acquired, acquired[1:])]
    wait_with_ready = []
    acquire_durations = []; pending = None; begin = None
    for row in native_all:
        if row['event'] == 'acquire_call': pending = row; begin = None
        elif row['event'] == 'acquire_begin': begin = row
        elif row['event'] in {'acquire_new', 'acquire_repeat'} and pending and start <= row['time_us'] <= end:
            acquire_durations.append(row['time_us'] - pending['time_us'])
            if begin and begin['ready'] > 0:
                wait_with_ready.append(row['a'])
            pending = None
    ff = [f for f in flutter if start <= f['buildStart'] <= f['rasterFinish'] <= end]
    render_results = Counter(str(f['slot']) for f in host)
    return dict(scope='eight-second diagnostic; acquisition is not physical display presentation',
        options=records[0], seconds=seconds, hostFrames=len(host),
        cpuCorrelation=correlate_cpu(host, records),
        prepareCorrelation=correlate_prepare(host, native),
        nativeEvents=dict(Counter(r['event'] for r in native)), matchedAttempts=len(matched),
        renderResults=dict(render_results), freshAcquisitionsPerSecond=len(acquired)/seconds,
        hostSimulationUs=distribution([f['simulationEndUs']-f['startUs'] for f in host]),
        hostRenderUs=distribution([f['endUs']-f['renderStartUs'] for f in host if f['slot'] is not None]),
        hostWorkUs=distribution([f['endUs']-f['startUs'] for f in host]),
        hostStartIntervalUs=distribution([b['startUs']-a['startUs'] for a,b in zip(host,host[1:])]),
        simulationSteps=dict(Counter(f['steps'] for f in host)),
        simulationTotalMs=sum(f['simulationEndUs']-f['startUs'] for f in host)/1000,
        renderTotalMs=sum(f['endUs']-f['renderStartUs'] for f in host if f['slot'] is not None)/1000,
        droppedRenderCpuMs=sum(f['endUs']-f['renderStartUs'] for f in dropped)/1000,
        dropOccupancy=dict(Counter(','.join(str(r[k]) for k in ['free','rendering','ready','presented','retiring']) for r in events['drop'])),
        renderRequestToAcquisitionUs=distribution(request_to_acquisition),
        submitToCompletionUs=distribution(spans('submitted','ready')),
        completionToNotifyReturnUs=distribution(spans('ready','notified')),
        completionToAcquisitionUs=distribution(spans('ready','acquire_new')),
        acquisitionIntervalUs=distribution(intervals), acquisitionCallUs=distribution(acquire_durations),
        waitingWithCompletedFrameUs=distribution(wait_with_ready),
        stagesUs={k:distribution([r[col] for r in events['stages']]) for k,col in [('extract','a'),('prepare','b'),('encode','c')]},
        flutterFramesCovered=len(ff), flutterLastRasterUs=max((f['rasterFinish'] for f in ff),default=None),
        flutterBuildUs=distribution([f['buildFinish']-f['buildStart'] for f in ff]),
        flutterRasterUs=distribution([f['rasterFinish']-f['rasterStart'] for f in ff]),
        flutterRasterQueueUs=distribution([f['rasterStart']-f['buildFinish'] for f in ff]),
        flutterVsyncToBuildUs=distribution([f['buildStart']-f['vsyncStart'] for f in ff]))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    print(json.dumps(analyze(args.directory), indent=2))
