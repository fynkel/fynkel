#!/usr/bin/env python3
"""Summarize bounded CPU readback without treating a host frame as one tick."""
import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path
from statistics import mean, median


def analyze_ticks(samples, ticks):
    by_frame = defaultdict(list)
    previous = None
    for tick in ticks:
        index = tick['frameIndex']
        if not isinstance(index, int) or index < 0 or index >= len(samples):
            raise ValueError('tick refers to an unknown host frame')
        if previous is not None and tick['tick'] <= previous:
            raise ValueError('tick identifiers must increase')
        previous = tick['tick']
        by_frame[index].append(tick)
    windows = defaultdict(list)
    for index, frame in enumerate(samples):
        frame_ticks = by_frame[index]
        if len(frame_ticks) != frame['ticks']:
            raise ValueError('tick probe count does not match host frame')
        if frame_ticks:
            costs = [next(t['micros'] for t in tick['systems'] if t['system'] == 'step') for tick in frame_ticks]
            windows[frame['elapsedUs'] // 1_000_000].append((frame, frame_ticks, costs))
    result = []
    for second, frames in sorted(windows.items()):
        all_ticks = [tick for _, ticks, _ in frames for tick in ticks]
        systems = defaultdict(list)
        for tick in all_ticks:
            for system in tick['systems']:
                systems[system['system']].append(system['micros'])
        stages = defaultdict(list)
        for tick in all_ticks:
            for stage, micros in tick.get('stagesUs', {}).items():
                if micros < 0:
                    raise ValueError('negative SDK stage time')
                stages[stage].append(micros)
        if stages and any(len(values) != len(all_ticks) for values in stages.values()):
            raise ValueError('incomplete SDK stage timings')
        multi = [costs for _, _, costs in frames if len(costs) > 1]
        result.append({
            'second': second,
            'ticks': len(all_ticks),
            'sdkStageMediansUs': {k: median(v) for k, v in stages.items()},
            'sdkStageMeansUs': {k: mean(v) for k, v in stages.items()},
            **({'bridgeOutsideStepMeanUs': mean(
                tick['stagesUs']['bridgeStep'] - next(s['micros'] for s in tick['systems'] if s['system'] == 'step')
                for tick in all_ticks)} if 'bridgeStep' in stages else {}),
            'systemMediansUs': {k: median(v) for k, v in systems.items()},
            'firstOfMultiTickStepMedianUs': median(c[0] for c in multi) if multi else None,
            'lastOfMultiTickStepMedianUs': median(c[-1] for c in multi) if multi else None,
            'nativeOutsideStepsPerTickMedianUs': median((f['nativeUs'] - sum(c)) / len(c) for f, _, c in frames),
            'probeReadbackMedianUs': median(t['readbackUs'] for t in all_ticks),
        })
    return result


def analyze(capture):
    batches = [r for r in capture['records'] if r.get('event') == 'cpuFrames']
    if len(batches) != 1:
        raise ValueError('expected exactly one CPU sample batch')
    samples = batches[0]['samples']
    windows = defaultdict(list)
    for sample in samples:
        if sample['ticks'] < 0 or sample['elapsedUs'] < 0:
            raise ValueError('negative tick count or sample time')
        windows[sample['elapsedUs'] // 1_000_000].append(sample)
    result = []
    for second, frames in sorted(windows.items()):
        ticking = [f for f in frames if f['ticks'] > 0]
        systems = defaultdict(list)
        for frame in ticking:
            for system in frame['lastTickSystems']:
                systems[system['system']].append(system['micros'])
        result.append({
            'second': second,
            'frames': len(frames),
            'ticks': sum(f['ticks'] for f in frames),
            'ticksPerFrameCounts': dict(sorted(Counter(f['ticks'] for f in frames).items())),
            'frameNativePerTickMedianUs': median(f['nativeUs'] / f['ticks'] for f in ticking) if ticking else None,
            'frameBehavioursPerTickMedianUs': median(f['behavioursUs'] / f['ticks'] for f in ticking) if ticking else None,
            'lastTickSystemMediansUs': {k: median(v) for k, v in systems.items()},
            'readbackMedianUs': median(f['readbackUs'] for f in frames),
        })
    tick_batches = [r for r in capture['records'] if r.get('event') == 'cpuTicks']
    if len(tick_batches) > 1:
        raise ValueError('multiple tick probe batches')
    return {
        **({'allTickWindows': analyze_ticks(samples, tick_batches[0]['samples'])} if tick_batches else {}),
        'captureStatus': capture['status'],
        'sustainedQualification': False,
        'sampleCount': len(samples),
        'interpretation': 'Frame costs divided by ticks include frame overhead; system timings sample only the last tick of each ticking frame. Step overlaps other systems. Zero-tick frames do not contribute stale system data.',
        'windows': result,
    }


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('capture', type=Path)
    args = parser.parse_args()
    print(json.dumps(analyze(json.loads(args.capture.read_text())), indent=2))
