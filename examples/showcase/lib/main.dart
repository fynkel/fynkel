// Fynkel Showcase — the renderer's example scene (examples/showcase).
//
// One scene that exercises what the renderer can do — the sun's shadow,
// spot and point lights, blend and mask materials, textured PBR, the sky —
// with a camera that orbits it (game.Orbit) so shadows and the transparent
// sort are seen moving. `fynkel render --project examples/showcase` writes
// the same view to a PNG; renders/ keeps one per renderer milestone.
//
// `FynkelGameView` (package:fynkel_flutter) owns the loop (ADR-0007): once
// per Flutter frame it hands the frame time to the world, which runs every
// behaviour's fixedUpdate for the planned ticks, steps the native runtime and
// runs update/lateUpdate; then it renders the world into the viewport
// texture. Flutter widgets drawn over it are the HUD. The scene is
// fynkel.project.json's startupScene; meshes and textures come from the
// project's cooked cache (`fynkel cook`, which `fynkel run` does for you):
// by path when this process can see the project directory, else from the
// copy the app bundles (fynkel_cache/, unpacked at start-up) — which is
// how the same build runs on an iPhone.
// Q cycles the project's quality profiles (fynkel.project.json `quality`);
// Space pauses and resumes the orbit.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel/fynkel.dart' show FynkelLoadError;
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'game/game.dart';
import 'game/orbit.dart';
import 'game/spinner.dart';

void main() => runApp(const GameApp());

class GameApp extends StatelessWidget {
  const GameApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Fynkel Showcase',
      theme: ThemeData.dark(useMaterial3: true),
      home: const GamePage(),
    );
  }
}

class GamePage extends StatefulWidget {
  const GamePage({super.key});

  @override
  State<GamePage> createState() => _GamePageState();
}

class _GamePageState extends State<GamePage> {
  /// The project's quality profiles, in the order Q cycles them.
  static const profiles = ['low', 'medium', 'high'];

  Game? _game;
  String? _error;
  FynkelViewportController? _viewport;
  FynkelFrameStats? _stats;

  /// The orbit's authored speed while paused (0 = paused).
  double _pausedSpeed = 0;

  /// Q: the next quality profile (the project default, `medium`, first).
  /// Space: pause / resume the camera orbit.
  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    if (event.logicalKey == LogicalKeyboardKey.keyQ) {
      final viewport = _viewport!;
      final current = profiles.indexOf(viewport.quality ?? 'medium');
      setState(() => viewport.quality = profiles[(current + 1) % profiles.length]);
      return KeyEventResult.handled;
    }
    if (event.logicalKey == LogicalKeyboardKey.space) {
      final orbit = _game!.world.behaviours.whereType<Orbit>().firstOrNull;
      if (orbit == null) return KeyEventResult.ignored;
      setState(() {
        if (orbit.speed == 0) {
          orbit.speed = _pausedSpeed;
        } else {
          _pausedSpeed = orbit.speed;
          orbit.speed = 0;
        }
      });
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  @override
  void initState() {
    super.initState();
    _start();
  }

  Future<void> _start() async {
    try {
      final dir = projectDirectory();
      if (dir != null && Directory(dir).existsSync()) {
        _game = Game.load(dir);
      } else {
        // Not on the machine that holds the project (a device): the cache
        // the app bundles, or nothing to show.
        final cache = await unpackBundledCache();
        if (cache == null) {
          throw StateError(
            dir == null
                ? 'No project directory and no bundled cache: run `fynkel run` from the project '
                    '(or pass --dart-define=FYNKEL_PROJECT=<dir> to flutter run).'
                : '$dir is not on this device and the app bundles no cache: run `fynkel cook`, then build again.',
          );
        }
        _game = Game.load(dir ?? '(bundled)', cacheDir: cache);
      }
      // The viewport draws from the world's asset store (the cooked cache
      // the scene came from); scene entities without a mesh stay invisible
      // (placeholders off) as they will in the shipped game.
      _viewport = FynkelViewportController(placeholders: false);
    } on FynkelLoadError catch (e) {
      _error = '$e\nrun `fynkel cook` in the project (or `fynkel run`, which cooks first)';
    } catch (e) {
      _error = '$e';
    }
    if (mounted) setState(() {});
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
    if (game == null) {
      return Scaffold(body: Center(child: Text(_error ?? '', textAlign: TextAlign.center)));
    }
    return Scaffold(
      body: FynkelGameView(
        world: game.world,
        controller: _viewport,
        onKeyEvent: _onKey,
        onStats: (s) => setState(() => _stats = s),
        overlay: Padding(
          padding: const EdgeInsets.all(16),
          child: Align(
            alignment: Alignment.topLeft,
            child: DecoratedBox(
              decoration: BoxDecoration(
                color: Colors.black.withValues(alpha: 0.55),
                borderRadius: BorderRadius.circular(8),
              ),
              child: Padding(
                padding: const EdgeInsets.all(12),
                child: Hud(game: game, stats: _stats, viewport: _viewport!),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// The HUD: ordinary Flutter widgets composited over the 3D viewport.
class Hud extends StatelessWidget {
  const Hud({super.key, required this.game, required this.stats, required this.viewport});

  final Game game;
  final FynkelFrameStats? stats;
  final FynkelViewportController viewport;

  @override
  Widget build(BuildContext context) {
    final style = Theme.of(context).textTheme.bodyMedium?.copyWith(fontFamily: 'monospace');
    final s = stats;
    final lines = <String>[
      'Fynkel Showcase',
      'scene      ${game.report.sceneName} (${game.report.entityCount} entities)',
      'tick       ${game.world.tick}',
      'behaviours ${game.world.behaviours.length} attached',
      for (final sp in game.world.behaviours.whereType<Spinner>())
        'spinner    ${sp.angle.toStringAsFixed(2)} rad at ${sp.speed} rad/s',
      for (final o in game.world.behaviours.whereType<Orbit>())
        'orbit      ${o.angle.toStringAsFixed(2)} rad at ${o.speed} rad/s  [Space pauses]',
      'assets     ${game.report.assetCount} retained from ${game.report.cacheDir}',
      'quality    ${viewport.quality ?? 'medium (project default)'}  [Q cycles]',
      if (s != null) ...[
        'fps        ${s.fps.toStringAsFixed(0)}  frame ${s.tickInterval.p50.toStringAsFixed(2)} ms '
            '(p99 ${s.tickInterval.p99.toStringAsFixed(2)})  render ${s.renderCall.p50.toStringAsFixed(2)} ms',
        'viewport   ${s.physicalWidth}x${s.physicalHeight}  ${s.fixedTicks} ticks/s  '
            '${s.missedDeadlines} missed, ${s.dropped} dropped',
        if (s.viewport case final v?) ...[
          'meshes     ${v.instances} drawn in ${v.draws} draws (${v.transparent} blended), '
              '${v.culled} culled, ${v.missing} missing, ${v.assetErrors} errors',
          'lights     ${v.lights} point/spot (${v.lightsDropped} dropped), '
              '${v.shadowMap == 0 ? 'no shadow' : '${v.shadowCasters} casters in ${v.shadowDraws} draws into a ${v.shadowMap}² shadow map'}',
          'render     ${v.renderWidth}x${v.renderHeight} at msaa ${v.msaa}  '
              'gpu ${v.gpuP50Ms.toStringAsFixed(2)} ms  cpu ${v.cpuP50Ms.toStringAsFixed(2)} ms',
        ],
      ],
      for (final d in game.report.diagnostics) '$d',
    ];
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [for (final l in lines) Text(l, style: style)],
    );
  }
}
