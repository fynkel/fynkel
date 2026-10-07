// The engine comparison's Fynkel app (docs/engine-comparison.md, run C1):
// the scene of lib/game/game.dart in a `FynkelGameView`, configured from
// the environment like the Bevy and Unity apps —
//   CMP_LEVEL = plain | full | msaa   (the project's quality profiles)
//   CMP_SCALE = render scale (1.0, 0.75)
//   CMP_SHOT  = 1: camera frozen at t = 0, one PNG of the frame after 5 s
// — and printing one `[cmp] …` line a second.

import 'dart:async';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:fynkel/gameplay.dart' show FramePlan;
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:path_provider/path_provider.dart';

import 'fynkel_game.dart';
import 'game/game.dart';

final Map<String, String> env = Platform.environment;
// A `--dart-define` wins: devicectl's launch environment does not reach a
// Flutter app's Platform.environment on iOS (it does reach the Bevy app).
String _setting(String key, String fallback) {
  final defined = switch (key) {
    'CMP_LEVEL' => const String.fromEnvironment('CMP_LEVEL'),
    'CMP_SCALE' => const String.fromEnvironment('CMP_SCALE'),
    'CMP_SCENE' => const String.fromEnvironment('CMP_SCENE'),
    _ => '',
  };
  return defined.isNotEmpty ? defined : env[key] ?? fallback;
}

final String level = _setting('CMP_LEVEL', 'full');
final double scale = double.tryParse(_setting('CMP_SCALE', '')) ?? 1.0;
final bool shot = env['CMP_SHOT'] == '1';

/// `cubes`: the Bevy spike's scene (head-to-head); anything else: Sponza.
final String scene = _setting('CMP_SCENE', 'sponza');

void main() {
  // A release build's print is invisible from a wireless iPhone: every
  // `[cmp]` line also goes to `<app tmp>/cmp.log` (fetch it with
  // `xcrun devicectl device copy from`, tools/compare/run_ios.sh).
  final log = File('${Directory.systemTemp.path}/cmp.log')..writeAsStringSync('');
  final printLine = debugPrint;
  debugPrint = (String? message, {int? wrapWidth}) {
    printLine(message, wrapWidth: wrapWidth);
    if (message != null && message.startsWith('[cmp]')) {
      log.writeAsStringSync('$message\n', mode: FileMode.append, flush: true);
    }
  };
  runApp(buildGame(FynkelLaunch.fromEnvironment()));
}

class GameApp extends StatelessWidget {
  const GameApp({super.key, required this.launch});

  final FynkelLaunch launch;

  @override
  Widget build(BuildContext context) => MaterialApp(debugShowCheckedModeBanner: false, home: GamePage(launch: launch));
}

class GamePage extends StatefulWidget {
  const GamePage({super.key, required this.launch});

  final FynkelLaunch launch;

  @override
  State<GamePage> createState() => _GamePageState();
}

class _GamePageState extends State<GamePage> {
  Game? _game;
  FynkelViewportController? _viewport;
  String? _error;
  double _t = 0;
  bool _shotTaken = false;

  @override
  void initState() {
    super.initState();
    _start();
  }

  Future<void> _start() async {
    debugPrint('[cmp] start env CMP_LEVEL=${env['CMP_LEVEL']} CMP_SHOT=${env['CMP_SHOT']}');
    try {
      final dir = widget.launch.projectDir;
      if (dir != null && Directory(dir).existsSync()) {
        _game = scene == 'cubes'
            ? Game.loadCubes(dir, cacheDir: widget.launch.cacheDir)
            : Game.load(dir, cacheDir: widget.launch.cacheDir);
      } else {
        final cache = await unpackBundledCache();
        if (cache == null) throw StateError('no project directory and no bundled cache');
        _game = scene == 'cubes'
            ? Game.loadCubes(dir ?? '(bundled)', cacheDir: cache)
            : Game.load(dir ?? '(bundled)', cacheDir: cache);
      }
      _viewport = FynkelViewportController.game(quality: level);
      debugPrint('[cmp] loaded level=$level scale=$scale shot=$shot');
    } catch (e, st) {
      _error = '$e';
      debugPrint('[cmp] load failed: $e\n$st');
    }
    if (mounted) setState(() {});
  }

  void _onFrame(double dt, FramePlan plan) {
    final game = _game!;
    if (!shot) _t += dt;
    final sw = Stopwatch()..start();
    game.place(_t);
    _placeUs.add(sw.elapsedMicroseconds);
    if (shot && !_shotTaken && _elapsed.elapsedMilliseconds > (int.tryParse(env['CMP_SHOT_DELAY'] ?? '') ?? 5) * 1000) {
      _shotTaken = true;
      unawaited(_saveShot());
    }
  }

  final Stopwatch _elapsed = Stopwatch()..start();

  /// The Dart side of each frame (`Game.place`: the camera, or 1 000 cube
  /// transforms in the cubes scene), in µs, for the head-to-head.
  final List<int> _placeUs = [];

  Future<void> _saveShot() async {
    final v = _viewport!;
    final slot = v.render(_game!.world);
    if (slot == null || slot < 0) {
      debugPrint('[cmp] shot failed: render answered $slot');
      return;
    }
    final pixels = v.readFrame(slot);
    if (pixels == null) return;
    final w = v.physicalWidth, h = v.physicalHeight;
    final buffer = await ui.ImmutableBuffer.fromUint8List(pixels);
    final descriptor = ui.ImageDescriptor.raw(buffer, width: w, height: h, pixelFormat: ui.PixelFormat.rgba8888);
    final image = await (await descriptor.instantiateCodec()).getNextFrame();
    final png = await image.image.toByteData(format: ui.ImageByteFormat.png);
    final dirPath = Platform.isIOS ? (await getApplicationDocumentsDirectory()).path : Directory.current.path;
    final file = File('$dirPath/cmp_shot_fynkel_${level}_$scale.png');
    await file.writeAsBytes(png!.buffer.asUint8List());
    debugPrint('[cmp] shot ${file.path}');
  }

  void _onStats(FynkelFrameStats s) {
    final v = s.viewport;
    final thermal = s.platform?.thermalState.index;
    // ViewportThermalState: unknown, nominal, fair, serious, critical.
    final t = thermal == null || thermal == 0 ? -1 : thermal - 1;
    debugPrint(
      '[cmp] engine=fynkel scene=$scene level=$level scale=$scale '
      'fps=${(v == null ? s.fps : v.framesPresented / s.windowSeconds).toStringAsFixed(1)} '
      'cpu_ms=${s.totalWork.p50.toStringAsFixed(2)} '
      'gpu_ms=${v?.gpuFrame.p50.toStringAsFixed(2)} gpu_p95=${v?.gpuFrame.p95.toStringAsFixed(2)} '
      'thermal=$t '
      'dart_ms=${_p50(_placeUs)} sim_ms=${s.simulate.p50.toStringAsFixed(2)} '
      'render_cpu_ms=${v?.cpuP50Ms.toStringAsFixed(2)} render_cpu_p95=${v?.cpuP95Ms.toStringAsFixed(2)} '
      'draws=${v?.draws}',
    );
    _placeUs.clear();
  }

  String _p50(List<int> us) {
    if (us.isEmpty) return 'null';
    final sorted = [...us]..sort();
    return (sorted[sorted.length ~/ 2] / 1000).toStringAsFixed(3);
  }

  @override
  void dispose() {
    _viewport?.dispose();
    _game?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final game = _game;
    if (_error != null) return Scaffold(body: Center(child: Text(_error!)));
    if (game == null) return const Scaffold(backgroundColor: Colors.black);
    return Scaffold(
      backgroundColor: Colors.black,
      body: FynkelGameView(
        world: game.world,
        controller: _viewport,
        resolutionScale: scale,
        onFrame: _onFrame,
        onStats: _onStats,
      ),
    );
  }
}
