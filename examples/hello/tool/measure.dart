// The step 5 and 6 measurements (docs/stage-1.md §4): runs the profile
// build of hello and reports the fresh frame rate, the Dart allocation per
// frame, and the memory over create/destroy and resize cycles.
//
//   flutter build macos --profile
//   dart tool/measure.dart rate [--seconds 60] [--fullscreen]
//   dart tool/measure.dart lifecycle [--cycles 10]
//
// With `--ios <device id>` (from `flutter devices`) it measures on that
// iPhone instead: it starts hello there with `flutter run --profile`,
// passes the settings as `--dart-define`s, and reads the app's output
// through its VM service, since an iOS app's output doesn't reach
// `flutter run`. Nothing outside an iPhone can read an app's footprint, so
// there the app reports its own (lib/footprint.dart). Add `--debug` for
// the iOS Simulator, which has no profile mode: its memory points still
// mean something, its frame rate and Dart allocation don't.
//
// The Dart heap is read from out here, through the app's VM service: the
// app's own isolate does nothing for the measurement, so it can't distort
// it. Only the VM's GC timeline is recorded while it runs; each collection
// event carries the heap before and after it, which gives the bytes
// allocated between any two samples.
//
// Memory is the process's physical footprint (`footprint`), which counts
// the GPU memory and IOSurfaces the process owns, not just its heap. Each
// point is the lowest of several readings while the app stands still: the
// Metal driver keeps a 128 MB pool dirty for about a second after any blit
// (a GPU copy), and both a new view (wgpu clears a buffer of its own) and
// Flutter (text updates) blit now and then. Leaked memory is there in every
// reading, so the lowest one can't hide it.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

const _app = 'build/macos/Build/Products/Profile/hello.app/Contents/MacOS/hello';

/// Seconds of a run that aren't measured: startup and the first frames.
const _warmUp = 5;

/// The iPhone to measure on, or null for this Mac.
String? _ios;

/// `flutter run --debug` instead of `--profile`, for the iOS Simulator.
var _debug = false;

/// A spinning Flutter box instead of the engine's view: what Flutter alone
/// does in the same check.
var _baseline = false;

Future<void> main(List<String> args) async {
  final iosAt = args.indexOf('--ios');
  if (iosAt >= 0) _ios = args[iosAt + 1];
  _debug = args.contains('--debug');
  _baseline = args.contains('--baseline');
  if (_ios == null && !File(_app).existsSync()) {
    stderr.writeln('No profile build. Run: flutter build macos --profile');
    exit(2);
  }
  final mode = args.isEmpty ? 'rate' : args.first;
  final fullscreen = args.contains('--fullscreen');
  final secondsAt = args.indexOf('--seconds');
  final seconds = secondsAt < 0 ? 60 : int.parse(args[secondsAt + 1]);
  switch (mode) {
    case 'rate':
      await _rate(seconds, fullscreen: fullscreen);
    case 'lifecycle':
      final cyclesAt = args.indexOf('--cycles');
      await _lifecycle(cyclesAt < 0 ? 10 : int.parse(args[cyclesAt + 1]));
    default:
      stderr.writeln(
        'usage: dart tool/measure.dart rate [--seconds N] [--fullscreen] | lifecycle [--cycles N] [--baseline]  [--ios <device>] [--debug]',
      );
      exit(2);
  }
}

/// [seconds] of the spinning cube: the frame rate the player sees, every
/// frame that wasn't fresh, and the Dart allocation per frame.
Future<void> _rate(int seconds, {required bool fullscreen}) async {
  final app = await _App.start({'FYNKEL_STATS': '1', if (fullscreen) 'FYNKEL_FULLSCREEN': '1'});
  try {
    await app.serviceReady;
    await Future<void>.delayed(const Duration(seconds: _warmUp));
    final heap = await _Heap.connect(app.service!);
    final from = app.windows.length;
    await Future<void>.delayed(Duration(seconds: seconds));
    final allocated = await heap.allocatedSinceStart();
    await heap.close();
    final windows = app.windows.sublist(from);
    if (windows.isEmpty) throw StateError('the app printed no stats');

    int sum(String key) => windows.fold(0, (total, w) => total + (w[key] as num).toInt());
    final fresh = windows.map((w) => w['fresh'] as double).toList()..sort();
    final presented = sum('presented');
    print(
      'mode            ${_ios != null
          ? 'iPhone'
          : fullscreen
          ? 'full screen'
          : 'window'}, ${windows.last['size']} pixels',
    );
    print('windows         ${windows.length} × ~1 s');
    print(
      'fresh fps       median ${fresh[fresh.length ~/ 2].toStringAsFixed(1)}, '
      'min ${fresh.first.toStringAsFixed(1)}, max ${fresh.last.toStringAsFixed(1)}',
    );
    print('frames          posted ${sum('posted')}, rendered ${sum('rendered')}, presented $presented');
    print('not shown       coalesced ${sum('coalesced')}, skipped ${sum('skipped')}, dropped ${sum('dropped')}');
    print('flutter frames  ${sum('flutterFrames')}, slow ${sum('slow')} (build or raster over 8.33 ms)');
    print('mouse in view   ${sum('mouse')} of ${windows.length} windows (Flutter hit-tests it after every frame)');
    print(
      'dart heap       ${allocated.bytes} B allocated in ${allocated.seconds.toStringAsFixed(1)} s, '
      '${allocated.collections} collections',
    );
    print('per frame       ${(allocated.bytes / presented).toStringAsFixed(0)} B');
    // The app's own footprint, once a second: whether it grows while
    // nothing but frames happens.
    final footprints = [for (final w in windows) (w['footprint'] as num).toInt()];
    final quarter = footprints.length ~/ 4;
    print(
      'footprint       ${_mb(footprints.first)} at the start, ${_mb(footprints[quarter])} after a quarter, '
      '${_mb(footprints.last)} at the end',
    );
  } finally {
    await app.stop();
  }
}

/// [cycles] create/destroy cycles, then as many resizes between the full
/// window and 0.6 of it, with the footprint at each point the app reports.
/// Points of the same kind should match: the same view at the same size.
Future<void> _lifecycle(int cycles) async {
  final app = await _App.start({
    'FYNKEL_CHECK': 'lifecycle',
    'FYNKEL_CYCLES': '$cycles',
    'FYNKEL_STATS': '1',
    if (_baseline) 'FYNKEL_BASELINE': '1',
  });
  try {
    await app.serviceReady;
    // The Dart heap's share of each point, so a growing total can be told
    // apart: Dart's or native.
    final heap = await _Heap.open(app.service!);
    final marks = <String, _Footprint>{};
    final idle = <String>[];
    await for (final point in app.checks) {
      final from = app.windows.length;
      final footprint = marks[point] = _ios == null
          ? await _Footprint.lowest(app.pid)
          : await app.memory.firstWhere((m) => m.point == point).then((m) => _Footprint(m.bytes, m.graphics, null));
      final dart = await heap.usage();
      // A hidden window gets no frames, and an idle GPU frees its driver
      // memory, so such a reading says nothing about the view.
      // The baseline has no engine view, so no frame counts: its readings
      // can't be checked this way.
      final drawing = _baseline || app.windows.skip(from).any((w) => (w['presented'] as num) > 0);
      if (!drawing) idle.add(point);
      print(
        '${point.padRight(12)} $footprint   Dart heap ${_mb(dart.capacity)} (${_mb(dart.used)} used)'
        '${drawing ? '' : '   NOT DRAWING'}',
      );
      if (point == 'done') break;
    }
    if (!marks.containsKey('done')) throw StateError('the app stopped reporting before the check finished');
    await heap.close();
    void range(String label, bool Function(String point) which) {
      final totals = [
        for (final e in marks.entries)
          if (which(e.key)) e.value.total,
      ]..sort();
      print('${label.padRight(28)} ${_mb(totals.first)} to ${_mb(totals.last)} (${_mb(totals.last - totals.first)})');
    }

    int number(String point) => int.parse(point.split(' ').last);
    print('');
    range('created, full size', (p) => p.startsWith('created'));
    range('resized, back to full size', (p) => p.startsWith('resized') && number(p).isEven);
    range('resized, to 0.6', (p) => p.startsWith('resized') && number(p).isOdd);
    if (idle.isNotEmpty) {
      print(
        '\nNothing was drawn at ${idle.join(', ')}: was the window hidden or the phone locked? Those readings are void.',
      );
      exitCode = 1;
    }
  } finally {
    await app.stop();
  }
}

/// The process's physical footprint: in total, the GPU memory it owns
/// (render targets, buffers) and its IOSurfaces (ours and Flutter's).
class _Footprint {
  _Footprint(this.total, this.gpu, this.surfaces);

  final int total;
  final int? gpu, surfaces;

  /// The lowest of five readings over two seconds, after a moment for the
  /// app to settle (it stands still for three seconds after each mark).
  static Future<_Footprint> lowest(int pid) async {
    _Footprint? lowest;
    for (var i = 0; i < 5; i++) {
      await Future<void>.delayed(const Duration(milliseconds: 400));
      final reading = await of(pid);
      if (lowest == null || reading.total < lowest.total) lowest = reading;
    }
    return lowest!;
  }

  static Future<_Footprint> of(int pid) async {
    final result = await Process.run('footprint', ['-p', '$pid', '-f', 'bytes']);
    final out = '${result.stdout}';
    int category(String name) => RegExp(
      '^\\s*(\\d+) B.*\\s$name',
      multiLine: true,
    ).allMatches(out).fold(0, (sum, m) => sum + int.parse(m.group(1)!));
    final total = RegExp(r'Footprint: (\d+) B').firstMatch(out);
    if (total == null) throw StateError('footprint said: $out${result.stderr}');
    return _Footprint(int.parse(total.group(1)!), category(r'Owned physical footprint'), category('IOSurface'));
  }

  @override
  String toString() {
    final (gpu, surfaces) = (this.gpu, this.surfaces);
    final all = _mb(total).padLeft(9);
    if (gpu == null) return all;
    // On iOS the app reports Metal's part itself (the graphics ledger:
    // buffers, textures, the driver's pools). IOSurfaces aren't in it on
    // the iPhone; they are in the rest, with the CPU's memory.
    if (surfaces == null) return '$all   Metal ${_mb(gpu)}, rest ${_mb(total - gpu)}';
    return '$all   GPU ${_mb(gpu)}, IOSurface ${_mb(surfaces)}';
  }
}

String _mb(int bytes) => '${(bytes / (1 << 20)).toStringAsFixed(1)} MB';

/// The running app, its output read line by line.
class _App {
  _App._(this._process) {
    _process.stderr.transform(utf8.decoder).transform(const LineSplitter()).listen(_line);
    _process.stdout.transform(utf8.decoder).transform(const LineSplitter()).listen(_line);
  }

  /// hello with [settings]: on this Mac as environment variables, or on
  /// [_ios] through `flutter run`.
  static Future<_App> start(Map<String, String> settings) async {
    final ios = _ios;
    if (ios == null) {
      // Without saved window state: a run after a full-screen one would
      // otherwise start full screen too.
      final app = _App._(await Process.start(_app, ['-ApplePersistenceIgnoreState', 'YES'], environment: settings));
      unawaited(app._process.exitCode.then((code) => app._ended('the app ended ($code)')));
      return app;
    }
    final app = _App._(
      await Process.start('flutter', [
        'run',
        if (_debug) '--debug' else '--profile',
        '-d',
        ios,
        for (final MapEntry(:key, :value) in settings.entries) '--dart-define=$key=$value',
      ]),
    );
    // flutter run ending before the app is up (no such device, a locked
    // phone, a failed build) must fail the measurement, with its reasons.
    unawaited(
      app._process.exitCode.then((code) {
        if (!app._serviceReady.isCompleted) {
          app._serviceReady.completeError(
            StateError('flutter run ended ($code) before the app was up:\n${app._log.join('\n')}'),
          );
        }
        app._ended('flutter run ended ($code)');
      }),
    );
    return app;
  }

  /// The app went away before [stop]: a check still waiting for a point
  /// fails, with flutter run's last words. Without this, nothing would be
  /// left to wait for and the tool would end quietly, with exit code 0.
  void _ended(String why) {
    if (_stopping || _checks.isClosed) return;
    final last = _log.length > 20 ? _log.sublist(_log.length - 20) : _log;
    final error = StateError('$why before the check finished. flutter run said, last:\n${last.join('\n')}');
    // Both streams a check may be waiting on.
    _checks.addError(error);
    unawaited(_checks.close());
    if (!_memory.isClosed) {
      _memory.addError(error);
      unawaited(_memory.close());
    }
  }

  /// flutter run's output on iOS, for the error when it fails.
  final _log = <String>[];

  final Process _process;
  final _serviceReady = Completer<void>();
  final _checks = StreamController<String>();
  var _stopping = false;
  final _memory = StreamController<({String point, int bytes, int graphics})>.broadcast();
  Uri? service;
  WebSocket? _appOutput;

  /// Each `[stats]` line, as its key=value pairs.
  final windows = <Map<String, Object>>[];

  int get pid => _process.pid;

  /// The VM service is up (and on iOS, the app's output is being read).
  /// An iOS build and install can take minutes.
  Future<void> get serviceReady =>
      _serviceReady.future.timeout(_ios == null ? const Duration(seconds: 30) : const Duration(minutes: 10));
  Stream<String> get checks => _checks.stream;

  /// The app's own footprint readings (`[memory]` lines).
  Stream<({String point, int bytes, int graphics})> get memory => _memory.stream;

  void _line(String line) {
    if (_ios != null) {
      // flutter run's own output: only the VM service address matters,
      // and the rest is kept for the error if it fails.
      _log.add(line);
      if (line.contains('Ensure the device is unlocked') || line.contains('Waiting for')) stderr.writeln(line);
      final uri = RegExp(r'Dart VM Service on .* is available at: (\S+)').firstMatch(line);
      if (uri != null && service == null) unawaited(_readAppOutput(Uri.parse(uri.group(1)!)));
      return;
    }
    final uri = RegExp(r'VM service is listening on (\S+)').firstMatch(line);
    if (uri != null && !_serviceReady.isCompleted) {
      service = Uri.parse(uri.group(1)!);
      _serviceReady.complete();
    }
    _appLine(line);
  }

  /// On iOS: the app's stdout, from the VM service's `Stdout` stream.
  Future<void> _readAppOutput(Uri uri) async {
    service = uri;
    final socket = _appOutput = await WebSocket.connect('${uri.replace(scheme: 'ws', path: '${uri.path}ws')}');
    var partial = '';
    socket.listen((data) {
      final event = ((jsonDecode(data as String) as Map)['params'] as Map?)?['event'] as Map?;
      if (event?['kind'] != 'WriteEvent') return;
      final lines = (partial + utf8.decode(base64.decode(event!['bytes'] as String))).split('\n');
      partial = lines.removeLast();
      lines.forEach(_appLine);
    }, onDone: () => _ended("the app's output ended"));
    socket.add(
      jsonEncode({
        'jsonrpc': '2.0',
        'id': 'stdout',
        'method': 'streamListen',
        'params': {'streamId': 'Stdout'},
      }),
    );
    _serviceReady.complete();
  }

  void _appLine(String line) {
    if (line.startsWith('[check] ')) _checks.add(line.substring(8));
    final memory = RegExp(r'^\[memory\] (.+) (\d+) (\d+)$').firstMatch(line);
    if (memory != null) {
      _memory.add((point: memory.group(1)!, bytes: int.parse(memory.group(2)!), graphics: int.parse(memory.group(3)!)));
    }
    if (line.startsWith('[stats] ')) {
      final window = <String, Object>{};
      for (final pair in line.substring(8).split(' ')) {
        final [key, value] = pair.split('=');
        window[key] = num.tryParse(value) ?? value;
      }
      windows.add(window);
    }
  }

  Future<void> stop() async {
    _stopping = true;
    await _appOutput?.close();
    if (_ios != null) {
      // Quits the app on the phone too.
      _process.stdin.writeln('q');
      await _process.exitCode.timeout(const Duration(seconds: 20), onTimeout: () => _process.kill() ? -1 : -1);
    } else {
      _process.kill();
      await _process.exitCode;
    }
    // Not awaited: a stream nobody listened to (rate never reads checks)
    // never finishes closing, and awaiting it would end the program
    // silently, with exit code 0, before an error in flight is reported.
    unawaited(_checks.close());
    unawaited(_memory.close());
  }
}

/// The app's Dart heap, through its VM service.
class _Heap {
  _Heap._(this._socket) {
    _socket.listen(_onMessage);
  }

  final WebSocket _socket;
  late final String _isolate;
  final _pending = <int, Completer<Map<String, Object?>>>{};
  int _nextId = 0;
  late int _startMicros, _startUsed;
  List<Object?>? _streams;

  /// Connected, and counting allocation from now on.
  static Future<_Heap> connect(Uri service) async => (await open(service))._start();

  /// Connected, for [usage] alone: the timeline is left as it is.
  static Future<_Heap> open(Uri service) async {
    final ws = service.replace(scheme: 'ws', path: '${service.path}ws');
    final heap = _Heap._(await WebSocket.connect('$ws'));
    final isolates = ((await heap._call('getVM'))['isolates'] as List).cast<Map<String, Object?>>();
    heap._isolate = isolates.firstWhere((i) => i['name'] == 'main', orElse: () => isolates.first)['id'] as String;
    return heap;
  }

  /// The memory the Dart heap holds now (its capacity, which is what the
  /// footprint sees; what it uses changes with every collection) and what
  /// it uses.
  Future<({int capacity, int used})> usage() async {
    final usage = await _call('getMemoryUsage', {'isolateId': _isolate});
    return (capacity: (usage['heapCapacity'] as num).toInt(), used: (usage['heapUsage'] as num).toInt());
  }

  Future<_Heap> _start() async {
    _streams = ((await _call('getVMTimelineFlags'))['recordedStreams'] as List?) ?? const [];
    await _call('setVMTimelineFlags', {
      'recordedStreams': ['GC'],
    });
    _startMicros = await _micros();
    _startUsed = await _used();
    return this;
  }

  /// Bytes allocated since [connect]: the heap's growth between the points
  /// where it is known (the start, before and after each collection, now).
  Future<({int bytes, double seconds, int collections})> allocatedSinceStart() async {
    final now = await _micros();
    final timeline = await _call('getVMTimeline', {
      'timeOriginMicros': _startMicros,
      'timeExtentMicros': now - _startMicros,
    });
    final used = await _used();
    final collections = <({int ts, int before, int after})>[];
    for (final event in (timeline['traceEvents'] as List).cast<Map<String, Object?>>()) {
      if (event['cat'] != 'GC' || event['ph'] != 'E') continue;
      final args = event['args'] as Map<String, Object?>?;
      int? kb(String key) {
        final value = args?[key];
        final n = value is num ? value.toInt() : int.tryParse('$value');
        return n == null ? null : n * 1024;
      }

      final beforeNew = kb('Before.New.Used (kB)');
      if (beforeNew == null) continue;
      collections.add((
        ts: (event['ts'] as num).toInt(),
        before: beforeNew + (kb('Before.Old.Used (kB)') ?? 0),
        after: (kb('After.New.Used (kB)') ?? 0) + (kb('After.Old.Used (kB)') ?? 0),
      ));
    }
    collections.sort((a, b) => a.ts.compareTo(b.ts));
    var bytes = 0, known = _startUsed;
    for (final c in collections) {
      if (c.before > known) bytes += c.before - known;
      known = c.after;
    }
    if (used > known) bytes += used - known;
    return (bytes: bytes, seconds: (now - _startMicros) / 1e6, collections: collections.length);
  }

  Future<void> close() async {
    // Only [connect] changed the timeline.
    if (_streams case final streams?) await _call('setVMTimelineFlags', {'recordedStreams': streams});
    await _socket.close();
  }

  Future<int> _micros() async => ((await _call('getVMTimelineMicros'))['timestamp'] as num).toInt();

  Future<int> _used() async => ((await _call('getMemoryUsage', {'isolateId': _isolate}))['heapUsage'] as num).toInt();

  Future<Map<String, Object?>> _call(String method, [Map<String, Object?> params = const {}]) {
    final id = _nextId++;
    final reply = _pending[id] = Completer();
    _socket.add(jsonEncode({'jsonrpc': '2.0', 'id': '$id', 'method': method, 'params': params}));
    return reply.future.timeout(const Duration(seconds: 10)).catchError((Object e) => throw StateError('$method: $e'));
  }

  void _onMessage(Object? data) {
    final message = jsonDecode(data as String) as Map<String, Object?>;
    final reply = _pending.remove(int.tryParse('${message['id']}'));
    if (reply == null) return;
    final result = message['result'];
    if (result is Map<String, Object?>) {
      reply.complete(result);
    } else {
      reply.completeError('${message['error']}');
    }
  }
}
