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
final String level = env['CMP_LEVEL'] ?? 'full';
final double scale = double.tryParse(env['CMP_SCALE'] ?? '') ?? 1.0;
final bool shot = env['CMP_SHOT'] == '1';

void main() => runApp(buildGame(FynkelLaunch.fromEnvironment()));

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
        _game = Game.load(dir, cacheDir: widget.launch.cacheDir);
      } else {
        final cache = await unpackBundledCache();
        if (cache == null) throw StateError('no project directory and no bundled cache');
        _game = Game.load(dir ?? '(bundled)', cacheDir: cache);
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
    game.place(_t);
    if (shot && !_shotTaken && _elapsed.elapsedMilliseconds > (int.tryParse(env['CMP_SHOT_DELAY'] ?? '') ?? 5) * 1000) {
      _shotTaken = true;
      unawaited(_saveShot());
    }
  }

  final Stopwatch _elapsed = Stopwatch()..start();

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
      '[cmp] engine=fynkel level=$level scale=$scale '
      'fps=${(v == null ? s.fps : v.framesPresented / s.windowSeconds).toStringAsFixed(1)} '
      'cpu_ms=${s.totalWork.p50.toStringAsFixed(2)} '
      'gpu_ms=${v?.gpuFrame.p50.toStringAsFixed(2)} gpu_p95=${v?.gpuFrame.p95.toStringAsFixed(2)} '
      'thermal=$t',
    );
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
