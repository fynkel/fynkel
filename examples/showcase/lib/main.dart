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
// 1–4 (or the buttons at the bottom, on a phone) switch between the
// showcase and the example shaders' scenes (dissolve, water, rim light:
// assets/shaders/); Space pauses and resumes
// the orbit. The `move` action (A/D, the left
// stick, the touch overlay's stick on a phone) scrubs the orbit.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel/gameplay.dart' show FynkelLoadError, Vec3;
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'fynkel_game.dart';
import 'game/game.dart';
import 'game/orbit.dart';
import 'game/spinner.dart';

/// A build: the project and start scene from the environment
/// (`FynkelLaunch.fromEnvironment`); the editor's Play calls [buildGame]
/// itself with the open project and scene (ADR-0021).
void main() => runApp(buildGame(FynkelLaunch.fromEnvironment()));

class GameApp extends StatelessWidget {
  const GameApp({super.key, this.launch = const FynkelLaunch()});

  /// Where to start (see [GamePage.launch]).
  final FynkelLaunch launch;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Fynkel Showcase',
      theme: ThemeData.dark(useMaterial3: true),
      home: GamePage(launch: launch),
    );
  }
}

class GamePage extends StatefulWidget {
  const GamePage({super.key, this.launch = const FynkelLaunch()});

  /// The project to load and the scene to start in: from the environment
  /// in a build, the open project and scene in the editor's Play.
  final FynkelLaunch launch;

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
  final _input = FynkelInputCapture();

  /// Whether a tap folded the HUD to its first lines.
  bool _hudFolded = false;

  /// The orbit's authored speed while paused (0 = paused).
  double _pausedSpeed = 0;

  /// Q: the next quality profile (the project default, `medium`, first).
  /// 1–4: the showcase, then the dissolve, water and rim light scenes.
  /// Space: pause / resume the camera orbit.
  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    const digits = [
      LogicalKeyboardKey.digit1,
      LogicalKeyboardKey.digit2,
      LogicalKeyboardKey.digit3,
      LogicalKeyboardKey.digit4,
    ];
    final scene = digits.indexOf(event.logicalKey);
    if (scene >= 0) {
      setState(() => _game!.loadScene(Game.scenes.values.elementAt(scene)));
      return KeyEventResult.handled;
    }
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
      final launch = widget.launch;
      final dir = launch.projectDir;
      if (dir != null && Directory(dir).existsSync()) {
        _game = Game.load(dir, cacheDir: launch.cacheDir);
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
      // The editor's Play (or FYNKEL_SCENE): the open scene, when it is
      // not the startup scene the project loaded.
      final start = launch.startScene;
      if (start != null && start != Game.scenes['Showcase']) _game!.loadScene(start);
      // The viewport draws from the world's asset store (the cooked cache
      // the scene came from); scene entities without a mesh stay invisible
      // (placeholders off) as they will in the shipped game.
      _viewport = FynkelViewportController.game();
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
        input: _input,
        onKeyEvent: _onKey,
        onStats: (s) => setState(() => _stats = s),
        overlay: Stack(
          fit: StackFit.expand,
          children: [
            // World labels (docs/hud.md): pinned to scene objects through the
            // world's projection view — one call per frame, paint transforms,
            // no rebuilds. The crate labels opt into occlusion by colliders
            // on the `default` layer (this scene has none yet, so they stay
            // visible; a collider on a pillar would hide the label behind
            // it); the robot's label is never hidden by geometry.
            ProjectionLayer(
              world: game.world,
              children: [
                if (game.world.find('Robot') case final robot?)
                  WorldLabel(target: robot, offset: const Vec3(0, 2.4, 0), text: 'Robot'),
                for (final name in const ['Crate A', 'Crate B', 'Crate C'])
                  if (game.world.find(name) case final crate?)
                    WorldLabel(
                      target: crate,
                      offset: const Vec3(0, 0.8, 0),
                      text: name,
                      occlusionMask: game.world.layerMask(const ['default']),
                    ),
                if (game.world.find('Gold Block') case final gold?)
                  WorldLabel(target: gold, offset: const Vec3(0, 0.8, 0), text: 'Gold', background: const Color(0xB3806000)),
              ],
            ),
            TouchOverlay.fromActions(game.report.inputActions, input: _input),
            // The scenes as buttons too, for a phone (no number keys there).
            SafeArea(
              child: Align(
                alignment: Alignment.bottomCenter,
                child: Padding(
                  padding: const EdgeInsets.only(bottom: 170),
                  child: Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    alignment: WrapAlignment.center,
                    children: [
                      for (final MapEntry(key: name, value: id) in Game.scenes.entries)
                        FilledButton.tonal(
                          key: Key('scene.$name'),
                          onPressed: () => setState(() => game.loadScene(id)),
                          child: Text(name),
                        ),
                    ],
                  ),
                ),
              ),
            ),
            Padding(
              padding: const EdgeInsets.all(16),
              child: Align(
                alignment: Alignment.topLeft,
                // A tap folds the HUD to its first lines (a phone's screen).
                child: GestureDetector(
                  onTap: () => setState(() => _hudFolded = !_hudFolded),
                  child: DecoratedBox(
                    decoration: BoxDecoration(
                      color: Colors.black.withValues(alpha: 0.55),
                      borderRadius: BorderRadius.circular(8),
                    ),
                    child: Padding(
                      padding: const EdgeInsets.all(12),
                      child: Hud(game: game, stats: _stats, viewport: _viewport!, folded: _hudFolded),
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// The HUD: ordinary Flutter widgets composited over the 3D viewport.
class Hud extends StatelessWidget {
  const Hud({super.key, required this.game, required this.stats, required this.viewport, this.folded = false});

  final Game game;
  final FynkelFrameStats? stats;
  final FynkelViewportController viewport;

  /// Only the title and the scene line (a tap on the HUD toggles it).
  final bool folded;

  @override
  Widget build(BuildContext context) {
    final style = Theme.of(context).textTheme.bodyMedium?.copyWith(fontFamily: 'monospace');
    final s = stats;
    final lines = <String>[
      'Fynkel Showcase',
      'scene      ${game.scene.sceneName} (${game.scene.entityCount} entities)  '
          '[${[for (final (i, name) in Game.scenes.keys.indexed) '${i + 1} $name'].join(', ')}]',
      'tick       ${game.world.tick}',
      'behaviours ${game.world.behaviours.length} attached',
      for (final sp in game.world.behaviours.whereType<Spinner>())
        'spinner    ${sp.angle.toStringAsFixed(2)} rad at ${sp.speed} rad/s',
      for (final o in game.world.behaviours.whereType<Orbit>())
        'orbit      ${o.angle.toStringAsFixed(2)} rad at ${o.speed} rad/s  [Space pauses, A/D scrub]',
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
          // M3-16: the frame's split, the GPU's pass times, memory.
          'work p95   behaviours ${s.behaviours.p95.toStringAsFixed(2)}  native ${s.nativeStep.p95.toStringAsFixed(2)}  '
              'extract ${v.extract.p95.toStringAsFixed(2)}  prepare ${v.prepare.p95.toStringAsFixed(2)}  '
              'encode ${v.encode.p95.toStringAsFixed(2)}  total ${s.totalWork.p95.toStringAsFixed(2)} ms',
          'gpu p95    frame ${v.gpuFrame.p95.toStringAsFixed(2)} ms  (shadow ${v.gpuShadow.p95.toStringAsFixed(2)}, '
              'ao ${v.gpuAo.p95.toStringAsFixed(2)}, '
              'scene ${v.gpuScene.p95.toStringAsFixed(2)}, post ${v.gpuPost.p95.toStringAsFixed(2)})  '
              '${(v.gpuBytes / 1048576).toStringAsFixed(0)} MB',
          'memory     native ${(s.world.nativeBytes / 1048576).toStringAsFixed(1)} MB  '
              'resident ${(s.world.residentBytes / 1048576).toStringAsFixed(0)} MB  '
              'assets ${(s.world.residencyBytes / 1048576).toStringAsFixed(1)} MB',
        ],
      ],
      for (final d in game.report.diagnostics) '$d',
    ];
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [for (final l in folded ? lines.take(2) : lines) Text(l, style: style)],
    );
  }
}
