// Measures a Fynkel app as the stage reports do (docs/stage-1.md §4,
// docs/stage-2.md §3.5): builds and runs the profile build of the app in
// the current directory and reports the fresh frame rate, the Dart allocation per
// frame, and the memory over the cycles of the app's lifecycle check.
// The app runs `tool/measured.dart` when it has one (or, as before,
// `lib/measured.dart`): the game wrapped in this package's MeasuredApp
// (lib/src/measured_app.dart), so the game's own code knows nothing of
// measuring, and the app has this package only as a dev_dependency.
// Otherwise it runs lib/main.dart, which uses this package's Measurement
// itself.
//
//   dart run fynkel_measure:measure rate [--seconds 60] [--fullscreen]
//   dart run fynkel_measure:measure lifecycle [--cycles 10]
//   dart run fynkel_measure:measure bench
//
// The lifecycle check's points are the app's: MeasuredApp's make the
// game's own change (such as the next level) if it has one, destroy and
// add the root, show a new view, and resize it. Points of a kind (the name
// without its number) should have the same memory. What the app reports (Measurement.report), such
// as load times, is printed at the end as it is.
//
// `--define KEY=VALUE`, as often as needed, builds the app with that
// `--dart-define`, for an app's own settings (such as how many cubes).
//
// `--baseline` runs either with a spinning Flutter box in place of the
// engine's view: what Flutter alone does, to judge the engine against.
//
// With `--ios <device id>` (from `flutter devices`) it measures on that
// iPhone instead: it starts the app there with `flutter run --profile`,
// passes the settings as `--dart-define`s, and reads the app's output
// through its VM service, since an iOS app's output doesn't reach
// `flutter run`. Nothing outside an iPhone can read an app's footprint, so
// there the app reports its own (fynkel_flutter's lib/src/footprint.dart). Add `--debug` for
// the iOS Simulator, which has no profile mode: its memory points still
// mean something, its frame rate and Dart allocation don't.
//
// With `--android <device id>` it measures on that Android device or
// emulator the same way. There memory is read from out here, with
// `dumpsys meminfo`: PSS plus swapped PSS (the emulator swaps, and a leak
// must not hide there), and the "Graphics" row, the GPU memory the driver
// reports. On the emulator that row is always 0: the GPU memory lives in
// the emulator's process on the Mac. The lifecycle check adds what only
// Android has: rotations and trips to the background, where Flutter takes
// the view's surface away.
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
import 'dart:math';

// ignore: implementation_imports
import 'package:fynkel_flutter/src/thermal.dart' show thermalStates;

/// The app's name, from the pubspec.yaml here.
final String _name = () {
  final pubspec = File('pubspec.yaml');
  final name = pubspec.existsSync()
      ? RegExp(r'^name:\s*(\S+)', multiLine: true).firstMatch(pubspec.readAsStringSync())?.group(1)
      : null;
  if (name == null) {
    stderr.writeln("Run this from the app's directory (no pubspec.yaml with a name here).");
    exit(2);
  }
  return name;
}();

String get _app => 'build/macos/Build/Products/Profile/$_name.app/Contents/MacOS/$_name';

/// The entry point measured: the game wrapped in MeasuredApp, if the app
/// has one.
String get _target => _attaching
    ? 'lib/main.dart'
    : [
        'tool/measured.dart',
        'lib/measured.dart',
      ].firstWhere((path) => File(path).existsSync(), orElse: () => 'lib/main.dart');

/// `attach`: the app's own lib/main.dart, as a player gets it.
var _attaching = false;

/// Seconds of a run that aren't measured: startup and the first frames.
/// `--warmup N` for more, such as to let a profiler attach first: attaching
/// Xcode's Power Profiler stops an iPhone app for several seconds.
var _warmUp = 5;

/// The iPhone to measure on, or null.
String? _ios;

/// The Android device or emulator to measure on, or null.
String? _android;

/// The phone to measure on, or null for this Mac.
String? get _device => _ios ?? _android;

/// The app's Android package: its `applicationId`, or for the examples,
/// whose bundle ids are all `dev.fynkel.<name>`, that.
final String _package = () {
  for (final path in ['android/app/build.gradle.kts', 'android/app/build.gradle']) {
    final file = File(path);
    if (!file.existsSync()) continue;
    final id = RegExp(r'''applicationId\s*=?\s*["']([^"']+)["']''').firstMatch(file.readAsStringSync());
    if (id != null) return id.group(1)!;
  }
  return 'dev.fynkel.$_name';
}();

/// `flutter run --debug` instead of `--profile`, for the iOS Simulator.
var _debug = false;

/// A spinning Flutter box instead of the engine's view: what Flutter alone
/// does in the same check.
var _baseline = false;

/// The app's own `--dart-define`s, from `--define KEY=VALUE`.
final _defines = <String>[];

Future<void> main(List<String> args) async {
  final iosAt = args.indexOf('--ios');
  if (iosAt >= 0) _ios = args[iosAt + 1];
  final androidAt = args.indexOf('--android');
  if (androidAt >= 0) _android = args[androidAt + 1];
  _debug = args.contains('--debug');
  _baseline = args.contains('--baseline');
  for (var i = 0; i < args.length - 1; i++) {
    if (args[i] == '--define') _defines.add('--dart-define=${args[i + 1]}');
    if (args[i] == '--define-file') _defines.add('--dart-define-from-file=${args[i + 1]}');
  }
  final mode = args.isEmpty ? 'rate' : args.first;
  if (mode == 'attach') {
    if (_device == null) {
      stderr.writeln('attach measures on a phone: give --ios <device> or --android <device>');
      exit(2);
    }
    _attaching = true;
  }
  if (_device == null) await _buildMac();
  final fullscreen = args.contains('--fullscreen');
  final secondsAt = args.indexOf('--seconds');
  final seconds = secondsAt < 0 ? 60 : int.parse(args[secondsAt + 1]);
  final warmUpAt = args.indexOf('--warmup');
  if (warmUpAt >= 0) _warmUp = int.parse(args[warmUpAt + 1]);
  switch (mode) {
    case 'rate':
      await _rate(seconds, fullscreen: fullscreen);
    case 'lifecycle':
      final cyclesAt = args.indexOf('--cycles');
      await _lifecycle(cyclesAt < 0 ? 10 : int.parse(args[cyclesAt + 1]));
    case 'bench':
      await _bench();
    case 'attach':
      await _attach(secondsAt < 0 ? 600 : seconds);
    default:
      stderr.writeln(
        'usage: dart run fynkel_measure:measure rate [--seconds N] [--warmup N] [--fullscreen] | lifecycle [--cycles N] | bench | attach [--seconds N]  [--define KEY=VALUE] [--define-file FILE] [--baseline] [--ios <device> | --android <device>] [--debug]',
      );
      exit(2);
  }
}

/// The Mac's profile build of [_target], so the run is of the code as it
/// is now. A phone's is built by `flutter run`.
Future<void> _buildMac() async {
  stderr.writeln('Building $_target for macOS (profile)…');
  final build = await Process.run('flutter', ['build', 'macos', '--profile', '-t', _target, ..._defines]);
  if (build.exitCode != 0) {
    stderr
      ..write(build.stdout)
      ..write(build.stderr)
      ..writeln('The build failed.');
    exit(2);
  }
}

/// Waits [seconds], asking the VM service something each half minute, and
/// says each five minutes what the app reports, so a run that dies later
/// still leaves its memory. Returns when the service stopped answering, if
/// it did.
Future<int?> _watch(_Heap heap, _App app, int from, int seconds) async {
  final clock = Stopwatch()..start();
  int? silent;
  var said = 0;
  while (clock.elapsed.inSeconds < seconds) {
    final left = seconds - clock.elapsed.inSeconds;
    await Future<void>.delayed(Duration(seconds: left < 30 ? left : 30));
    if (silent == null) {
      try {
        await heap.alive();
      } on StateError {
        silent = clock.elapsed.inSeconds;
      }
    }
    if (clock.elapsed.inMinutes >= said + 5 && app.windows.length > from) {
      said = clock.elapsed.inMinutes;
      print(
        'at ${said}m           ${_mb((app.windows.last['footprint'] as num).toInt())}, ${app.windows.length - from} windows',
      );
    }
  }
  return silent;
}

/// [seconds] of the app running: the frame rate the player sees, every
/// frame that wasn't fresh, and the Dart allocation per frame. With
/// `--baseline`, Flutter's frames and allocation alone.
Future<void> _rate(int seconds, {required bool fullscreen}) async {
  final app = await _App.launch({
    'FYNKEL_STATS': '1',
    if (fullscreen) 'FYNKEL_FULLSCREEN': '1',
    if (_baseline) 'FYNKEL_BASELINE': '1',
  });
  try {
    await Future<void>.delayed(Duration(seconds: _warmUp));
    final heap = await _Heap.connect(app.service!);
    final from = app.windows.length;
    final gpu = _android != null ? _GpuClock.sample() : null;
    final silent = await _watch(heap, app, from, seconds);
    final clocks = await gpu?.stop();
    // The memory and the frames came on another connection: a VM service
    // that stopped answering costs this line, not a long run's report.
    final allocated = silent == null ? await heap.allocatedSinceStart() : null;
    if (silent == null) {
      await heap.printReports();
      await heap.close();
    } else {
      print('vm service      stopped answering after ${silent}s');
    }
    final windows = app.windows.sublist(from);
    if (windows.isEmpty) throw StateError('the app printed no stats');

    int sum(String key) => windows.fold(0, (total, w) => total + ((w[key] ?? 0) as num).toInt());
    final presented = sum('presented');
    print(
      'mode            ${_ios != null
          ? 'iPhone'
          : _android != null
          ? 'Android'
          : fullscreen
          ? 'full screen'
          : 'window'}${_baseline ? ', the baseline (no engine view)' : ', ${windows.last['size']} pixels'}',
    );
    print('windows         ${windows.length} × ~1 s');
    if (!_baseline) {
      final fresh = windows.map((w) => w['fresh'] as double).toList()..sort();
      print(
        'fresh fps       median ${fresh[fresh.length ~/ 2].toStringAsFixed(1)}, '
        'min ${fresh.first.toStringAsFixed(1)}, max ${fresh.last.toStringAsFixed(1)}',
      );
      // Which seconds were slowest, counted from 1: a hitch at the start
      // isn't one in the middle of play.
      final slowest = [for (final (i, w) in windows.indexed) (i + 1, w['fresh'] as double)]
        ..sort((a, b) => a.$2.compareTo(b.$2));
      print('slowest seconds ${slowest.take(3).map((s) => '${s.$1}: ${s.$2.toStringAsFixed(1)}').join(', ')}');
      // Over a long run, each minute's median: whether the rate holds as
      // the device warms (docs/iphone-heat.md).
      if (windows.length >= 120) {
        final minutes = [
          for (var at = 0; at + 60 <= windows.length; at += 60)
            (windows.sublist(at, at + 60).map((w) => w['fresh'] as double).toList()..sort())[30],
        ];
        print('by minute       ${minutes.map((m) => m.toStringAsFixed(0)).join(', ')}');
      }
      print('frames          posted ${sum('posted')}, rendered ${sum('rendered')}, presented $presented');
      final ticks = windows.map((w) => w['ticks'] as num).toList()..sort();
      print(
        'dart ticks/s    median ${ticks[ticks.length ~/ 2].toStringAsFixed(1)}, min ${ticks.first.toStringAsFixed(1)}',
      );
      // Microseconds busy in each ~1 s window, over the window's length.
      final busy = sum('busyMicros') / 1e6 / windows.length;
      print('render thread   busy ${(busy * 100).toStringAsFixed(0)} % of the time');
      print('not shown       coalesced ${sum('coalesced')}, skipped ${sum('skipped')}, dropped ${sum('dropped')}');
    }
    final budget = ((windows.last['budget'] ?? 8333) as num) / 1000;
    print(
      'flutter frames  ${sum('flutterFrames')}, slow ${sum('slow')} '
      '(build or raster over one frame, ${budget.toStringAsFixed(2)} ms): '
      'build ${sum('slowBuild')}, raster ${sum('slowRaster')}',
    );
    final longest = windows.fold(0, (most, w) => max(most, ((w['longestRaster'] ?? 0) as num).toInt()));
    print('longest raster  ${(longest / 1000).toStringAsFixed(2)} ms');
    if (clocks != null) print('gpu             $clocks');
    // The device's thermal state, where it says (iOS, macOS): each change,
    // by the window it was first seen in.
    if (windows.first['thermal'] != null) {
      final changes = <String>[];
      Object? last;
      for (final (i, w) in windows.indexed) {
        if (w['thermal'] == last) continue;
        last = w['thermal'];
        changes.add('${thermalStates[(last! as num).toInt()]} from ${i}s');
      }
      print('thermal         ${changes.join(', ')}');
    }
    print('mouse in view   ${sum('mouse')} of ${windows.length} windows (Flutter hit-tests it after every frame)');
    if (allocated != null) {
      print(
        'dart heap       ${allocated.bytes} B allocated in ${allocated.seconds.toStringAsFixed(1)} s, '
        '${allocated.collections} collections',
      );
      // The baseline has no engine frames: per Flutter frame instead.
      final frames = _baseline ? sum('flutterFrames') : presented;
      print('per frame       ${(allocated.bytes / frames).toStringAsFixed(0)} B');
    }
    // The app's own footprint, once a second: whether it grows while
    // nothing but frames happens.
    final footprints = [for (final w in windows) (w['footprint'] as num).toInt()];
    final quarter = footprints.length ~/ 4;
    print(
      'footprint       ${_mb(footprints.first)} at the start, ${_mb(footprints[quarter])} after a quarter, '
      '${_mb(footprints.last)} at the end',
    );
    // What it's made of, from the same windows: the footprint's GPU part,
    // and what the engine knows it's holding there.
    String each(String key) {
      final values = [for (final w in windows) ((w[key] ?? 0) as num).toInt()];
      return '${_mb(values.first)}, ${_mb(values[quarter])}, ${_mb(values.last)}';
    }

    print(
      'by minute MB    ${[for (var m = 0; m + 60 <= footprints.length; m += 60) (footprints[m] / (1 << 20)).round()].join(', ')}',
    );
    print('gpu memory      ${each('graphics')} (start, a quarter, end)');
    print('  textures      ${each('textureBytes')}');
    print('  meshes        ${each('meshBytes')}');
    print('  sky           ${each('skyBytes')}');
    print('  shadows       ${each('shadowBytes')}');
    print('  scene, AO     ${each('targetBytes')}');
    print('  frames        ${each('frameBytes')}');
    print('  behind        ${each('behindBytes')}');
    print('  bloom         ${each('bloomBytes')}');
    print('  effects       ${each('effectBytes')}');
  } finally {
    await app.stop();
  }
}

/// [seconds] of the app as it is, played by hand: its own lib/main.dart,
/// with no code of its own for measuring. Each second the engine says
/// through `ext.fynkel.views` (fynkel_flutter's inspection.dart) what each
/// view has done so far and the app's footprint and heat, and the
/// differences make the windows. On Android, the memory from `dumpsys
/// meminfo`, the battery's temperature and the thermal status, each ten
/// seconds, and the GPU's clock and the app in front at the start and end.
Future<void> _attach(int seconds) async {
  final app = await _App.launch(const {});
  try {
    final heap = await _Heap.connect(app.service!);
    Future<Map<String, Object?>> ask() => heap._call('ext.fynkel.views', {'isolateId': heap._isolate});
    // The engine's extension comes with the first view: until then, the
    // app's own screens.
    stderr.writeln('Attached. Play for ${seconds}s.');
    if (_android != null) print('in front        ${await _inFront()} at the start');
    final gpu = _android != null ? _GpuClock.sample() : null;
    final android = <({int at, _Footprint memory, double? battery, int? status})>[];
    final clock = Stopwatch()..start();
    final androidLoop = _android == null
        ? null
        : () async {
            while (clock.elapsed.inSeconds < seconds) {
              final at = clock.elapsed.inSeconds;
              final front = await _inFront();
              if (!front.startsWith('$_package/')) stderr.writeln('${at}s: not in front, $front is');
              try {
                android.add((
                  at: at,
                  memory: await _Footprint.ofAndroid(),
                  battery: await _battery(),
                  status: await _thermalStatus(),
                ));
              } on StateError {
                // The app away or restarting: the next reading.
              }
              await Future<void>.delayed(const Duration(seconds: 10));
            }
          }();

    final windows = <Map<String, Object?>>[];
    Map<String, Object?>? last;
    var unanswered = 0, views = 0, idle = 0;
    while (clock.elapsed.inSeconds < seconds) {
      await Future<void>.delayed(const Duration(seconds: 1));
      final Map<String, Object?> now;
      try {
        now = await ask();
      } on StateError catch (error) {
        // Not registered yet (no view so far), or the app is away.
        if (unanswered++ == 0) stderr.writeln('no answer yet: $error');
        continue;
      }
      if (last != null) {
        final window = _window(last, now, at: clock.elapsed.inSeconds);
        windows.add(window);
        views = max(views, (now['views']! as List).length);
        // Said as it happens, so a run that isn't play can be stopped.
        idle = window['fresh'] == null ? idle + 1 : 0;
        if (idle == 10) stderr.writeln('${window['at']}s: no view has drawn for 10 s');
      }
      last = now;
      final minutes = clock.elapsed.inSeconds ~/ 60;
      if (clock.elapsed.inSeconds % 60 == 0 && minutes > 0 && windows.isNotEmpty) {
        stderr.writeln('${minutes}m: ${_mb((windows.last['footprint']! as num).toInt())}');
      }
    }
    await androidLoop;
    final clocks = await gpu?.stop();
    final allocated = await heap.allocatedSinceStart();
    await heap.close();
    if (_android != null) print('in front        ${await _inFront()} at the end');

    print('mode            ${_ios != null ? 'iOS' : 'Android'}, the app as it is (attach), $seconds s');
    print('seconds         ${windows.length} read, $unanswered without an answer (before the first view, or away)');
    print('views           at most $views at once');
    final drawing = [
      for (final w in windows)
        if (w['fresh'] != null) w,
    ];
    if (drawing.isEmpty) {
      print('no view drew in the run');
    } else {
      final fresh = [for (final w in drawing) w['fresh']! as double]..sort();
      double at(List<double> sorted, double q) => sorted[(q * (sorted.length - 1)).round()];
      print(
        'fresh fps       median ${at(fresh, 0.5).toStringAsFixed(1)}, 5th percentile ${at(fresh, 0.05).toStringAsFixed(1)}, '
        'min ${fresh.first.toStringAsFixed(1)}, max ${fresh.last.toStringAsFixed(1)}, over ${drawing.length} s with a view',
      );
      final slowest = [for (final w in drawing) (w['at']! as int, w['fresh']! as double)]
        ..sort((a, b) => a.$2.compareTo(b.$2));
      print('slowest seconds ${slowest.take(5).map((s) => '${s.$1}: ${s.$2.toStringAsFixed(1)}').join(', ')}');
      if (drawing.length >= 120) {
        final minutes = [
          for (var m = 0; m + 60 <= drawing.length; m += 60)
            (drawing.sublist(m, m + 60).map((w) => w['fresh']! as double).toList()..sort())[30],
        ];
        print('by minute       ${minutes.map((m) => m.toStringAsFixed(0)).join(', ')}');
      }
      final ticks = [for (final w in drawing) w['ticks']! as double]..sort();
      print('dart ticks/s    median ${at(ticks, 0.5).toStringAsFixed(1)}, min ${ticks.first.toStringAsFixed(1)}');
      int sum(String key) => drawing.fold(0, (total, w) => total + ((w[key] ?? 0) as num).toInt());
      print('frames          posted ${sum('posted')}, rendered ${sum('rendered')}, presented ${sum('presented')}');
      print('not shown       coalesced ${sum('coalesced')}, skipped ${sum('skipped')}, dropped ${sum('dropped')}');
      print(
        'render thread   busy ${(sum('busyMicros') / 1e6 / drawing.length * 100).toStringAsFixed(0)} % of the time',
      );
      print('sizes           ${{for (final w in drawing) w['size']}.join(', ')}');
      print(
        'dart heap       ${allocated.bytes} B allocated in ${allocated.seconds.toStringAsFixed(1)} s, '
        '${allocated.collections} collections, ${(allocated.bytes / max(1, sum('presented'))).toStringAsFixed(0)} B per frame shown',
      );
    }
    if (windows.isNotEmpty) {
      int sum(String key) => windows.fold(0, (total, w) => total + ((w[key] ?? 0) as num).toInt());
      final frames = sum('flutterFrames');
      print(
        'flutter frames  $frames, raster ${(sum('rasterMicros') / max(1, frames) / 1000).toStringAsFixed(2)} ms '
        'and build ${(sum('buildMicros') / max(1, frames) / 1000).toStringAsFixed(2)} ms on average, longest raster '
        '${(windows.fold(0, (most, w) => max(most, (w['longestRaster']! as num).toInt())) / 1000).toStringAsFixed(2)} ms',
      );
      final footprints = [for (final w in windows) (w['footprint']! as num).toInt()];
      print(
        'footprint       ${_mb(footprints.first)} at the start, ${_mb(footprints.reduce(max))} at most, '
        '${_mb(footprints.last)} at the end${_ios != null ? '' : ' (PSS, as the app reads it)'}',
      );
      print(
        'by minute MB    ${[for (var m = 0; m < footprints.length; m += 60) (footprints[m] / (1 << 20)).round()].join(', ')}',
      );
      if (_ios != null) {
        final graphics = [for (final w in windows) (w['graphics']! as num).toInt()];
        print(
          'metal           ${_mb(graphics.first)} at the start, ${_mb(graphics.reduce(max))} at most, ${_mb(graphics.last)} at the end',
        );
      }
      final engine = [
        for (final w in windows)
          if (w['textureBytes'] != null) w,
      ];
      if (engine.isNotEmpty) {
        String most(String key) => _mb(engine.fold(0, (m, w) => max(m, (w[key]! as num).toInt())));
        print(
          'engine, at most textures ${most('textureBytes')}, meshes ${most('meshBytes')}, sky ${most('skyBytes')}, '
          'shadows ${most('shadowBytes')}, scene ${most('targetBytes')}, frames ${most('frameBytes')}, '
          'behind ${most('behindBytes')}, bloom ${most('bloomBytes')}, effects ${most('effectBytes')}',
        );
      }
      if (windows.any((w) => w['thermal'] != null)) {
        final changes = <String>[];
        Object? previous;
        for (final w in windows) {
          if (w['thermal'] == null || w['thermal'] == previous) continue;
          previous = w['thermal'];
          changes.add('${thermalStates[(previous! as num).toInt()]} from ${w['at']}s');
        }
        print('thermal         ${changes.join(', ')}');
      }
    }
    if (android.isNotEmpty) {
      final first = android.first, top = android.reduce((a, b) => a.memory.total >= b.memory.total ? a : b);
      print('meminfo         ${first.memory} at the start');
      print('                ${top.memory} at most (${top.at}s)');
      print('                ${android.last.memory} at the end');
      final temps = [
        for (final r in android)
          if (r.battery != null) r.battery!,
      ];
      if (temps.isNotEmpty) {
        print(
          'battery         ${temps.first.toStringAsFixed(1)} °C at the start, ${temps.reduce(max).toStringAsFixed(1)} at most, ${temps.last.toStringAsFixed(1)} at the end',
        );
      }
      final statuses = [
        for (final r in android)
          if (r.status != null) r.status!,
      ];
      if (statuses.isNotEmpty) print('thermal status  ${statuses.reduce(max)} at most (0 none … 6 shutdown)');
    }
    if (clocks != null) print('gpu             $clocks');
  } finally {
    await app.stop();
  }
}

/// One second of [attach]: the busiest view's counts between two reports,
/// and the memory and heat as of the second.
Map<String, Object?> _window(Map<String, Object?> before, Map<String, Object?> now, {required int at}) {
  final seconds = ((now['micros']! as num) - (before['micros']! as num)) / 1e6;
  int diff(String key) => (now[key]! as num).toInt() - (before[key]! as num).toInt();
  final window = <String, Object?>{
    'at': at,
    'footprint': now['footprint'],
    'graphics': now['graphics'],
    'thermal': now['thermal'],
    'flutterFrames': diff('flutterFrames'),
    'buildMicros': diff('buildMicros'),
    'rasterMicros': diff('rasterMicros'),
    'longestRaster': now['longestRasterMicros'],
  };
  final earlier = {for (final v in (before['views']! as List).cast<Map<String, Object?>>()) v['id']: v};
  Map<String, Object?>? busiest;
  var most = -1;
  for (final view in (now['views']! as List).cast<Map<String, Object?>>()) {
    final then = earlier[view['id']];
    // A new engine view starts its counts again: this second can't count it.
    if (then == null) continue;
    final presented = (view['presented']! as int) - (then['presented']! as int);
    if (presented <= most) continue;
    most = presented;
    busiest = {
      for (final key in ['posted', 'rendered', 'coalesced', 'skipped', 'presented', 'dropped', 'busyMicros'])
        key: (view[key]! as int) - (then[key]! as int),
      for (final key in view.keys)
        if (key.endsWith('Bytes')) key: view[key],
      'fresh': presented / seconds,
      'ticks': ((view['ticks']! as int) - (then['ticks']! as int)) / seconds,
      'size': '${view['width']}x${view['height']}',
    };
  }
  // A view that drew nothing this second (the app away, the game paused)
  // isn't a frame rate.
  if (busiest != null && (busiest['posted']! as int) > 0) window.addAll(busiest);
  return window;
}

/// The app in front on the Android device: a run's frames mean little
/// with another app over it.
Future<String> _inFront() async {
  final out = await _adb(['shell', 'dumpsys', 'window']);
  return RegExp(r'mCurrentFocus=Window\{\S+ \S+ ([^}]+)\}').firstMatch(out)?.group(1) ?? 'unknown';
}

/// The battery's temperature on the Android device, in °C.
Future<double?> _battery() async {
  final out = await _adb(['shell', 'dumpsys', 'battery']);
  final tenths = RegExp(r'temperature: (\d+)').firstMatch(out);
  return tenths == null ? null : int.parse(tenths.group(1)!) / 10;
}

/// Android's thermal status (0 none … 6 shutdown).
Future<int?> _thermalStatus() async {
  final out = await _adb(['shell', 'dumpsys', 'thermalservice']);
  final status = RegExp(r'Thermal Status: (\d+)').firstMatch(out);
  return status == null ? null : int.parse(status.group(1)!);
}

/// The app's own timings: it runs them, reports each and marks `done`.
Future<void> _bench() async {
  final app = await _App.launch({'FYNKEL_CHECK': 'bench'});
  try {
    final heap = await _Heap.open(app.service!);
    await for (final point in app.checks) {
      await heap.read();
      if (point == 'done') break;
    }
    await heap.printReports();
    await heap.close();
  } finally {
    await app.stop();
  }
}

/// The app's lifecycle check, [cycles] times over, with the footprint at
/// each point the app reports. Points of a kind (`created 3`, `created 4`)
/// should match: the app is in the same state at each.
Future<void> _lifecycle(int cycles) async {
  final app = await _App.launch({
    'FYNKEL_CHECK': 'lifecycle',
    'FYNKEL_CYCLES': '$cycles',
    'FYNKEL_STATS': '1',
    if (_baseline) 'FYNKEL_BASELINE': '1',
  });
  try {
    // The Dart heap's share of each point, so a growing total can be told
    // apart: Dart's or native.
    final heap = await _Heap.open(app.service!);
    final marks = <String, _Footprint>{};
    final idle = <String>[];
    await for (final point in app.checks) {
      final from = app.windows.length;
      final footprint = marks[point] = _android != null
          ? await _Footprint.lowest()
          : _ios == null
          ? await _Footprint.lowest(app.pid)
          : await app.memory.firstWhere((m) => m.point == point).then((m) => _Footprint(m.bytes, m.graphics, null));
      final dart = await heap.usage();
      await heap.read();
      // A hidden window gets no frames, and an idle GPU frees its driver
      // memory, so such a reading says nothing about the view.
      // The baseline has no engine view, so no frame counts: its readings
      // can't be checked this way.
      final drawing = _baseline || app.windows.skip(from).any((w) => (w['presented'] as num) > 0);
      if (!drawing) idle.add(point);
      print(
        '${point.padRight(24)} $footprint   Dart heap ${_mb(dart.capacity)} (${_mb(dart.used)} used)'
        '${drawing ? '' : '   NOT DRAWING'}',
      );
      if (point == 'done') break;
    }
    if (!marks.containsKey('done')) throw StateError('the app stopped reporting before the check finished');
    if (_android != null) await _androidCycles(app, cycles, heap, marks, idle);
    await heap.printReports();
    await heap.close();
    void range(String label, bool Function(String point) which) {
      final totals = [
        for (final e in marks.entries)
          if (which(e.key)) e.value.total,
      ]..sort();
      print('${label.padRight(24)} ${_mb(totals.first)} to ${_mb(totals.last)} (${_mb(totals.last - totals.first)})');
    }

    print('');
    // Each kind of point that came more than once, in the order they came.
    final kinds = <String>{for (final point in marks.keys) _kind(point)};
    for (final kind in kinds) {
      if (marks.keys.where((p) => _kind(p) == kind).length > 1) range(kind, (p) => _kind(p) == kind);
    }
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

/// A point's kind: its name without the number at the end.
String _kind(String point) => point.replaceFirst(RegExp(r' \d+$'), '');

/// Android only: [cycles] rotations to landscape and back, then [cycles]
/// trips to the background, with the memory after each (from out here:
/// the app doesn't know these happen). The app stands still meanwhile.
/// The view keeps its surface in the background (docs/stage-1.md §3.5).
Future<void> _androidCycles(_App app, int cycles, _Heap heap, Map<String, _Footprint> marks, List<String> idle) async {
  Future<void> mark(String point) async {
    final from = app.windows.length;
    // A moment for the app to settle and for a few stats windows.
    await Future<void>.delayed(const Duration(seconds: 3));
    final footprint = marks[point] = await _Footprint.lowest();
    final dart = await heap.usage();
    final drawing = _baseline || app.windows.skip(from).any((w) => (w['presented'] as num) > 0);
    if (!drawing) idle.add(point);
    print(
      '${point.padRight(24)} $footprint   Dart heap ${_mb(dart.capacity)} (${_mb(dart.used)} used)'
      '${drawing ? '' : '   NOT DRAWING'}',
    );
  }

  // The device may be someone's phone: its rotation settings go back to
  // what they were.
  Future<String> setting(String name) async => (await _adb(['shell', 'settings', 'get', 'system', name])).trim();
  final autoRotate = await setting('accelerometer_rotation');
  final rotation = await setting('user_rotation');
  await _adb(['shell', 'settings', 'put', 'system', 'accelerometer_rotation', '0']);
  try {
    for (var i = 1; i <= cycles * 2; i++) {
      await _adb(['shell', 'settings', 'put', 'system', 'user_rotation', i.isOdd ? '1' : '0']);
      await mark(i.isOdd ? 'rotated to landscape ${(i + 1) ~/ 2}' : 'rotated to portrait ${i ~/ 2}');
    }
  } finally {
    await _adb(['shell', 'settings', 'put', 'system', 'user_rotation', rotation == 'null' ? '0' : rotation]);
    await _adb([
      'shell',
      'settings',
      'put',
      'system',
      'accelerometer_rotation',
      autoRotate == 'null' ? '1' : autoRotate,
    ]);
  }
  // Each trip must end with the view drawing again; a point where it
  // doesn't fails the check (`idle`).
  for (var i = 1; i <= cycles; i++) {
    await _adb(['shell', 'input', 'keyevent', 'KEYCODE_HOME']);
    await Future<void>.delayed(const Duration(seconds: 3));
    await _adb(['shell', 'am', 'start', '-n', '$_package/.MainActivity']);
    await mark('resumed $i');
  }
}

/// `adb -s <device> args`, failing loudly.
Future<String> _adb(List<String> args) async {
  final sdk = Platform.environment['ANDROID_HOME'] ?? '${Platform.environment['HOME']}/Library/Android/sdk';
  final result = await Process.run('$sdk/platform-tools/adb', ['-s', _android!, ...args]);
  if (result.exitCode != 0) throw StateError('adb ${args.join(' ')} failed: ${result.stderr}');
  return '${result.stdout}';
}

/// The process's physical footprint: in total, the GPU memory it owns
/// (render targets, buffers) and its IOSurfaces (ours and Flutter's). On
/// Android: PSS with swap, and the App Summary's Graphics and Native Heap
/// rows (malloc's: Rust's, the driver's and Flutter's own C++).
class _Footprint {
  _Footprint(this.total, this.gpu, this.surfaces, {this.native});

  final int total;
  final int? gpu, surfaces, native;
  bool get android => native != null;

  /// The lowest of five readings, 400 ms apart. The app stands still after
  /// each mark until [_Heap.read] says the readings are done (over adb they
  /// take 5 to 7 s). On Android without a [pid]: the app's, through `dumpsys`.
  static Future<_Footprint> lowest([int? pid]) async {
    _Footprint? lowest;
    for (var i = 0; i < 5; i++) {
      await Future<void>.delayed(const Duration(milliseconds: 400));
      final reading = pid == null ? await ofAndroid() : await of(pid);
      if (lowest == null || reading.total < lowest.total) lowest = reading;
    }
    return lowest!;
  }

  static Future<_Footprint> ofAndroid() async {
    final out = await _adb(['shell', 'dumpsys', 'meminfo', _package]);
    int kb(String pattern) {
      final match = RegExp(pattern).firstMatch(out);
      if (match == null) throw StateError('dumpsys meminfo said: $out');
      return int.parse(match.group(1)!) * 1024;
    }

    return _Footprint(
      kb(r'TOTAL PSS:\s+(\d+)') + kb(r'TOTAL SWAP PSS:\s+(\d+)'),
      kb(r'Graphics:\s+(\d+)'),
      null,
      native: kb(r'Native Heap:\s+(\d+)'),
    );
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
    if (android) return '$all   Graphics ${_mb(gpu)}   Native ${_mb(native!)}';
    // On iOS the app reports Metal's part itself (the graphics ledger:
    // buffers, textures, the driver's pools). IOSurfaces aren't in it on
    // the iPhone; they are in the rest, with the CPU's memory.
    if (surfaces == null) return '$all   Metal ${_mb(gpu)}, rest ${_mb(total - gpu)}';
    return '$all   GPU ${_mb(gpu)}, IOSurface ${_mb(surfaces)}';
  }
}

/// An Android GPU's clock and how busy it is, read once a second through
/// adb while a run goes on: whether the governor raised the clock. Exynos
/// phones (the Galaxy Note10+) have them in `/sys/kernel/gpu`; on a phone
/// without them, [stop] gives null.
class _GpuClock {
  _GpuClock._();

  final _mhz = <int>[], _busy = <int>[];
  int? _min, _max;
  var _stopped = false;
  late final Future<void> _loop;

  static _GpuClock sample() {
    final clock = _GpuClock._();
    clock._loop = clock._run();
    return clock;
  }

  Future<void> _run() async {
    try {
      final range = (await _adb([
        'shell',
        'cat /sys/kernel/gpu/gpu_min_clock /sys/kernel/gpu/gpu_max_clock',
      ])).split('\n');
      _min = int.parse(range[0].trim()) ~/ 1000;
      _max = int.parse(range[1].trim()) ~/ 1000;
    } on Object {
      return;
    }
    while (!_stopped) {
      final read = (await _adb(['shell', 'cat /sys/kernel/gpu/gpu_clock /sys/kernel/gpu/gpu_busy'])).split('\n');
      final mhz = int.tryParse(read[0].trim()), busy = int.tryParse(read[1].replaceAll('%', '').trim());
      if (mhz != null && busy != null) {
        // A clock of 0 is a GPU powered off between frames.
        _mhz.add(mhz ~/ (mhz > 10000 ? 1000 : 1));
        _busy.add(busy);
      }
      await Future<void>.delayed(const Duration(seconds: 1));
    }
  }

  /// What was read, or null if nothing could be.
  Future<String?> stop() async {
    _stopped = true;
    await _loop;
    if (_mhz.isEmpty) return null;
    final sorted = [..._mhz]..sort();
    final atMin = _mhz.where((m) => m <= _min!).length;
    final busy = _busy.reduce((a, b) => a + b) / _busy.length;
    return 'clock median ${sorted[sorted.length ~/ 2]} MHz (range $_min–$_max), '
        'max seen ${sorted.last}, at the minimum ${(atMin * 100 / _mhz.length).toStringAsFixed(0)} % of '
        '${_mhz.length} readings; busy ${busy.toStringAsFixed(0)} %';
  }
}

String _mb(int bytes) => '${(bytes / (1 << 20)).toStringAsFixed(1)} MB';

/// The running app, its output read line by line.
class _App {
  _App._(this._process) {
    _process.stderr.transform(utf8.decoder).transform(const LineSplitter()).listen(_line);
    _process.stdout.transform(utf8.decoder).transform(const LineSplitter()).listen(_line);
  }

  /// The app with [settings]: on this Mac as environment variables, or on
  /// [_device] through `flutter run`.
  /// Starts the app and waits for its VM service. On Android, flutter run
  /// finds the service in the device's log, and a busy log can lose that
  /// line: the app runs, and flutter run waits for good (seen on a Galaxy
  /// Note10+). So when the line is late after the install, the app is
  /// started once more.
  static Future<_App> launch(Map<String, String> settings) async {
    for (var attempt = 1; ; attempt++) {
      final app = await start(settings);
      try {
        await app.serviceReady;
        return app;
      } on TimeoutException {
        final last = app._log.length > 20 ? app._log.sublist(app._log.length - 20) : app._log;
        await app.stop();
        if (_device == null || attempt == 2) {
          throw StateError("the app's VM service never came up. flutter run said, last:\n${last.join('\n')}");
        }
        stderr.writeln('flutter run never saw the VM service; starting the app once more');
      }
    }
  }

  static Future<_App> start(Map<String, String> settings) async {
    final device = _device;
    if (device == null) {
      // Without saved window state: a run after a full-screen one would
      // otherwise start full screen too.
      final app = _App._(await Process.start(_app, ['-ApplePersistenceIgnoreState', 'YES'], environment: settings));
      unawaited(app._process.exitCode.then((code) => app._ended('the app ended ($code)')));
      return app;
    }
    // A phone's screen may have gone off since the last run; the app keeps
    // it on once it runs.
    if (_android != null) await _adb(['shell', 'input', 'keyevent', 'KEYCODE_WAKEUP']);
    final app = _App._(
      await Process.start('flutter', [
        'run',
        if (_debug) '--debug' else '--profile',
        '-d',
        device,
        '-t',
        _target,
        for (final MapEntry(:key, :value) in settings.entries) '--dart-define=$key=$value',
        ..._defines,
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

  /// flutter run's output on a phone, for the error when it fails.
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

  /// The VM service is up (and on a phone, the app's output is being read).
  /// An iOS build and install can take minutes.
  /// On Android, two minutes from the install on are plenty.
  Future<void> get serviceReady {
    final installed = _installing.future.then(
      (_) => Future<void>.delayed(const Duration(minutes: 2), () {
        throw TimeoutException('no VM service two minutes after the install');
      }),
    );
    return Future.any([
      _serviceReady.future,
      installed,
    ]).timeout(_device == null ? const Duration(seconds: 30) : const Duration(minutes: 10));
  }

  /// flutter run is installing the app on an Android device.
  final _installing = Completer<void>();

  Stream<String> get checks => _checks.stream;

  /// The app's own footprint readings (`[memory]` lines).
  Stream<({String point, int bytes, int graphics})> get memory => _memory.stream;

  void _line(String line) {
    if (_device != null) {
      // flutter run's own output: only the VM service address matters,
      // and the rest is kept for the error if it fails.
      _log.add(line);
      if (line.contains('Ensure the device is unlocked') || line.contains('Waiting for')) stderr.writeln(line);
      if (_android != null && line.startsWith('Installing ') && !_installing.isCompleted) _installing.complete();
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

  /// On a phone: the app's stdout, from the VM service's `Stdout` stream
  /// (on Android dart:io's stdout reaches nothing else).
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
    if (_device != null) {
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

  /// Tells the app this point's memory is read, so its check goes on
  /// (Measurement.mark holds still until then).
  Future<void> read() => _call('ext.fynkel.read', {'isolateId': _isolate});

  /// Everything the app reported (Measurement.report), in order.
  Future<void> printReports() async {
    final reports = (await _call('ext.fynkel.reports', {'isolateId': _isolate}))['reports'] as List;
    if (reports.isEmpty) return;
    print('');
    reports.forEach(print);
    print('');
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

  /// Throws if the service doesn't answer.
  Future<void> alive() => _micros();

  Future<int> _micros() async => ((await _call('getVMTimelineMicros'))['timestamp'] as num).toInt();

  Future<int> _used() async => ((await _call('getMemoryUsage', {'isolateId': _isolate}))['heapUsage'] as num).toInt();

  Future<Map<String, Object?>> _call(String method, [Map<String, Object?> params = const {}]) {
    final id = _nextId++;
    final reply = _pending[id] = Completer();
    _socket.add(jsonEncode({'jsonrpc': '2.0', 'id': '$id', 'method': method, 'params': params}));
    // Long enough for an app a profiler stopped while attaching.
    return reply.future.timeout(const Duration(seconds: 60)).catchError((Object e) => throw StateError('$method: $e'));
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
