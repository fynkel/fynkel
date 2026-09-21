// The live Dart heap for the host's diagnostics (blueprint C3, M3-16):
// heap in use, bytes allocated, GC counts and pauses, sampled once per
// stats window over the VM service the running app already exposes.
// Nothing here touches the frame: the probe talks JSON-RPC on a WebSocket
// between reports, and what it reads is small — the VM's GC timeline
// stream (one event per collection with the heap before and after it)
// and `getMemoryUsage`. `getAllocationProfile` would give the same
// number and is not used: its answer lists every class (5–6 MB of JSON
// per call), and decoding it allocated more than the app it measured.

import 'dart:async';
import 'dart:convert';
import 'dart:developer' as dev;
import 'dart:io';
import 'dart:isolate';

/// One stats window of the Dart heap ([DartHeapProbe.sample]).
class DartHeapStats {
  const DartHeapStats({
    required this.heapUsage,
    required this.heapCapacity,
    required this.externalUsage,
    required this.allocatedBytes,
    required this.gcCollections,
    required this.gcPauseMs,
    required this.newSpaceCollections,
    required this.oldSpaceCollections,
    this.gcReasons = const {},
  });

  /// The isolate's heap: live bytes, the space reserved for it, and the
  /// native memory its objects hold (`getMemoryUsage`).
  final int heapUsage, heapCapacity, externalUsage;

  /// Bytes allocated since the previous sample: the heap's growth between
  /// the points where it is known — the previous sample, the heap before
  /// and after each collection in the window (the VM's `GC` timeline
  /// events), this sample. Exact to the kilobyte each event rounds to;
  /// counts every allocation, inlined or not, new and old space.
  final int allocatedBytes;

  /// Collections in the window (new + old space) and their total pause on
  /// the mutator, from the same events.
  final int gcCollections;
  final double gcPauseMs;
  final int newSpaceCollections, oldSpaceCollections;

  /// Collections by the VM's reason (`new space`, `old space`, `idle`,
  /// `external`, `finalize`, …).
  final Map<String, int> gcReasons;

  @override
  String toString() =>
      'DartHeapStats(heap ${heapUsage ~/ 1024} KB of ${heapCapacity ~/ 1024}, external ${externalUsage ~/ 1024} KB, '
      'allocated ${allocatedBytes ~/ 1024} KB, gc $gcCollections (${gcPauseMs.toStringAsFixed(2)} ms))';
}

/// A connection to the running app's own VM service for the heap numbers
/// (blueprint C3). Debug and profile builds carry the service; a release
/// build has none, so [connect] returns null there and the stats stay
/// absent — say so in a HUD rather than showing zeros. The AOT numbers
/// come from a `--profile` build, which is what `benchmarks/m0/capture.py`
/// runs (`hosts/flutter_apple` prints one `[alloc]` line per window).
///
/// While a probe is open the VM records only its `GC` timeline stream (so
/// a window's `getVMTimeline` answer is a few kilobytes, not the frame
/// timeline); [close] restores the streams that were recorded before.
class DartHeapProbe {
  DartHeapProbe._(this._socket, this._isolateId) {
    _socket.listen(_onMessage, onDone: _onDone, onError: (_) => _onDone());
  }

  final WebSocket _socket;
  final String _isolateId;
  final _pending = <int, Completer<Map<String, Object?>>>{};
  var _nextId = 1;
  var _closed = false;

  // Where the previous sample left off: the timeline clock and the heap
  // in use, the start of the next window's accounting.
  int _lastMicros = 0, _lastUsed = 0;
  List<Object?>? _previousStreams;

  bool get isOpen => !_closed;

  /// Connects to the service the app exposes; null when there is none
  /// (a release build) or it could not be reached.
  static Future<DartHeapProbe?> connect() async {
    Uri? uri;
    try {
      uri = (await dev.Service.getInfo()).serverUri;
      // A debug/profile app whose service is not serving yet.
      uri ??= (await dev.Service.controlWebServer(enable: true, silenceOutput: true)).serverUri;
    } catch (_) {
      return null;
    }
    if (uri == null) return null;
    final isolateId = dev.Service.getIsolateId(Isolate.current);
    if (isolateId == null) return null;
    final ws = uri.replace(scheme: 'ws', path: '${uri.path}ws');
    try {
      final socket = await WebSocket.connect(ws.toString()).timeout(const Duration(seconds: 2));
      final probe = DartHeapProbe._(socket, isolateId);
      final flags = await probe._call('getVMTimelineFlags', {});
      probe._previousStreams = (flags['recordedStreams'] as List?)?.toList();
      await probe._call('setVMTimelineFlags', {
        'recordedStreams': ['GC'],
      });
      await probe._mark();
      return probe;
    } catch (_) {
      return null;
    }
  }

  /// The window since the previous [sample] (or [connect]).
  Future<DartHeapStats?> sample() async {
    if (_closed) return null;
    try {
      final now = await _micros();
      final timeline = await _call('getVMTimeline', {
        'timeOriginMicros': _lastMicros,
        'timeExtentMicros': now - _lastMicros,
      });
      final usage = await _call('getMemoryUsage', {'isolateId': _isolateId});
      final used = (usage['heapUsage'] as num?)?.toInt() ?? 0;

      // Every collection's end event carries the heap before and after it
      // (kB); its begin event on the same thread gives the pause.
      final begins = <String, int>{};
      final collections = <_Collection>[];
      for (final e in ((timeline['traceEvents'] as List?) ?? const []).cast<Map>()) {
        if (e['cat'] != 'GC') continue;
        final key = '${e['name']}/${e['tid']}';
        final ts = (e['ts'] as num?)?.toInt() ?? 0;
        if (e['ph'] == 'B') {
          begins[key] = ts;
          continue;
        }
        if (e['ph'] != 'E') continue;
        final args = e['args'] as Map?;
        final beforeNew = _kb(args, 'Before.New.Used (kB)');
        if (beforeNew == null) continue;
        final before = beforeNew + (_kb(args, 'Before.Old.Used (kB)') ?? 0);
        final after = (_kb(args, 'After.New.Used (kB)') ?? 0) + (_kb(args, 'After.Old.Used (kB)') ?? 0);
        final began = begins.remove(key);
        collections.add(_Collection(
          ts: ts,
          before: before,
          after: after,
          pauseMicros: began == null ? 0 : ts - began,
          newSpace: '${e['name']}'.startsWith('CollectNew'),
          reason: '${args?['Reason'] ?? '?'}',
        ));
      }
      collections.sort((a, b) => a.ts.compareTo(b.ts));

      var allocated = 0, known = _lastUsed, pauseMicros = 0, newSpace = 0, oldSpace = 0;
      final reasons = <String, int>{};
      for (final c in collections) {
        allocated += (c.before - known).clamp(0, 1 << 62);
        known = c.after;
        pauseMicros += c.pauseMicros;
        if (c.newSpace) {
          newSpace++;
        } else {
          oldSpace++;
        }
        reasons[c.reason] = (reasons[c.reason] ?? 0) + 1;
      }
      allocated += (used - known).clamp(0, 1 << 62);
      _lastMicros = now;
      _lastUsed = used;
      return DartHeapStats(
        heapUsage: used,
        heapCapacity: (usage['heapCapacity'] as num?)?.toInt() ?? 0,
        externalUsage: (usage['externalUsage'] as num?)?.toInt() ?? 0,
        allocatedBytes: allocated,
        gcCollections: collections.length,
        gcPauseMs: pauseMicros / 1000,
        newSpaceCollections: newSpace,
        oldSpaceCollections: oldSpace,
        gcReasons: reasons,
      );
    } catch (_) {
      return null;
    }
  }

  Future<void> close() async {
    if (_closed) return;
    try {
      final previous = _previousStreams;
      if (previous != null) await _call('setVMTimelineFlags', {'recordedStreams': previous});
    } catch (_) {
      // The service went away first; nothing to restore.
    }
    _closed = true;
    await _socket.close();
  }

  Future<void> _mark() async {
    _lastMicros = await _micros();
    final usage = await _call('getMemoryUsage', {'isolateId': _isolateId});
    _lastUsed = (usage['heapUsage'] as num?)?.toInt() ?? 0;
  }

  Future<int> _micros() async => ((await _call('getVMTimelineMicros', {}))['timestamp'] as num).toInt();

  static int? _kb(Map? args, String key) {
    final v = args?[key];
    final n = v is num ? v.toInt() : int.tryParse('$v');
    return n == null ? null : n * 1024;
  }

  Future<Map<String, Object?>> _call(String method, Map<String, Object?> params) {
    if (_closed) return Future.error(StateError('probe closed'));
    final id = _nextId++;
    final completer = Completer<Map<String, Object?>>();
    _pending[id] = completer;
    _socket.add(jsonEncode({'jsonrpc': '2.0', 'id': id, 'method': method, 'params': params}));
    return completer.future.timeout(const Duration(seconds: 3), onTimeout: () {
      _pending.remove(id);
      throw TimeoutException(method);
    });
  }

  void _onMessage(Object? data) {
    if (data is! String) return;
    final msg = jsonDecode(data);
    if (msg is! Map) return;
    final id = msg['id'];
    if (id == null) return;
    final c = _pending.remove(int.tryParse('$id'));
    if (c == null || c.isCompleted) return;
    final result = msg['result'];
    if (result is Map) {
      c.complete(result.cast<String, Object?>());
    } else {
      c.completeError(StateError('${msg['error']}'));
    }
  }

  void _onDone() {
    _closed = true;
    for (final c in _pending.values) {
      if (!c.isCompleted) c.completeError(StateError('VM service closed'));
    }
    _pending.clear();
  }
}

class _Collection {
  const _Collection({
    required this.ts,
    required this.before,
    required this.after,
    required this.pauseMicros,
    required this.newSpace,
    required this.reason,
  });

  final int ts, before, after, pauseMicros;
  final bool newSpace;
  final String reason;
}
