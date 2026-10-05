#!/usr/bin/env python3
"""Summarize top-level GPU execution intervals for the exact captured app PID.

Encoder durations can overlap across channels. They are not additive frame time
and profiler captures must not be compared directly with unprofiled FPS runs.
"""
import argparse
from collections import defaultdict
import json
import math
from pathlib import Path
import statistics
import subprocess
import xml.etree.ElementTree as ET


def read_table(path):
    root = ET.parse(path).getroot()
    schema = root.find('.//schema')
    if schema is None:
        raise ValueError(f'{path}: missing exported schema')
    columns = [col.findtext('mnemonic') for col in schema.findall('col')]
    ids = {element.get('id'): element for element in root.iter() if element.get('id')}

    def resolve(element):
        if element.get('ref'):
            try:
                return ids[element.get('ref')]
            except KeyError as error:
                raise ValueError('unresolved Instruments XML reference') from error
        return element

    rows = []
    for row in root.iter('row'):
        if len(row) != len(columns):
            raise ValueError('Instruments row does not match schema')
        rows.append({key: resolve(value) for key, value in zip(columns, row)})
    return rows, resolve


def analyze(directory, start_ms=None, end_ms=None):
    for value in [start_ms, end_ms]:
        if value is not None and (not math.isfinite(value) or value < 0):
            raise ValueError('execution window bounds must be finite and nonnegative')
    if start_ms is not None and end_ms is not None and start_ms >= end_ms:
        raise ValueError('execution window start must precede end')
    capture = json.loads((directory / 'capture.json').read_text())
    if capture.get('status') != 'diagnostic-complete' or capture.get('metalTraceExitCode') != 0:
        raise ValueError('requires a completed app capture and successful Metal trace')
    pid = capture['pid']
    if capture.get('terminatedExactPid') != pid:
        raise ValueError('captured process was not confirmed stopped')
    encoders, resolve = read_table(directory / 'metal-application-encoders-list.xml')
    labels = {}
    for row in encoders:
        process_pid = row['process'].find('pid')
        if process_pid is not None and int(resolve(process_pid).text) == pid:
            labels[row['encoder-id'].text] = row['encoder-label'].get('fmt') or row['encoder-label'].text or '(unnamed)'
    intervals, _ = read_table(directory / 'metal-gpu-intervals.xml')
    pieces = defaultdict(list)
    for row in intervals:
        encoder = row['encoder-id'].text
        if encoder not in labels or row['event-depth'].text != '0' or row['state'].text != 'Active':
            continue
        start, duration = int(row['start'].text), int(row['duration'].text)
        if duration < 0:
            raise ValueError('negative GPU interval')
        pieces[(labels[encoder], row['channel-name'].text, encoder)].append((start, start + duration))
    # Keep complete encoder/channel executions, rather than shortening a long
    # execution at a window boundary and incorrectly reporting it as faster.
    durations = {}
    windows = []
    for key, spans in pieces.items():
        if start_ms is not None and min(start for start, _ in spans) < start_ms * 1e6:
            continue
        if end_ms is not None and max(end for _, end in spans) > end_ms * 1e6:
            continue
        durations[key] = sum(end - start for start, end in spans)
        windows.extend(spans)
    if not windows:
        raise ValueError('no top-level GPU execution matched the captured process')
    grouped = defaultdict(list)
    for (label, channel, _), duration in durations.items():
        grouped[(label, channel)].append(duration / 1e6)
    merged = []
    for start, end in sorted(windows):
        if merged and start <= merged[-1][1]:
            merged[-1][1] = max(end, merged[-1][1])
        else:
            merged.append([start, end])
    return {
        'captureId': capture['captureId'], 'pid': pid,
        'scope': 'Profiled top-level active intervals only; excludes nested intervals and other processes. Channel durations overlap; do not add them as frame time. Not an unprofiled FPS or hardware-stall measurement.',
        'executionWindowMs': {'start': start_ms, 'end': end_ms},
        'excludedEncoderChannels': len(pieces) - len(durations),
        'matchedEncoders': len({encoder for _, _, encoder in durations}),
        'observedSpanMs': (max(end for _, end in windows) - min(start for start, _ in windows)) / 1e6,
        'activeIntervalUnionMs': sum(end - start for start, end in merged) / 1e6,
        'stages': [dict(label=label, channel=channel, encoders=len(values),
                        medianActiveMs=statistics.median(values), totalActiveMs=sum(values))
                   for (label, channel), values in sorted(grouped.items(), key=lambda item: sum(item[1]), reverse=True)],
    }


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--export', action='store_true', help='Export just the two required tables from metal.trace')
    parser.add_argument('--start-ms', type=float, help='Trace-relative execution window start; exclude crossing executions')
    parser.add_argument('--end-ms', type=float, help='Trace-relative execution window end; exclude crossing executions')
    args = parser.parse_args()
    if args.export:
        for name in ['metal-application-encoders-list', 'metal-gpu-intervals']:
            result = subprocess.run(['xcrun', 'xctrace', 'export', '--input', str(args.directory / 'metal.trace'),
                                     '--xpath', f'/trace-toc/run[@number="1"]/data/table[@schema="{name}"]'],
                                    capture_output=True, text=True, timeout=60, check=True)
            (args.directory / f'{name}.xml').write_text(result.stdout)
    print(json.dumps(analyze(args.directory, args.start_ms, args.end_ms), indent=2))
