// UI Demo — entry point (created by `fynkel create`; edit freely).
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
// Input: the view captures keys, the mouse and gamepads into a
// `FynkelInputCapture` and hands each frame's events to the world, which
// turns them into the project's actions (`inputActions`; behaviours read
// `Input.action2D('move')` and friends). On a phone a `TouchOverlay` puts
// the same actions on screen. Q cycles the project's quality profiles
// (fynkel.project.json `quality`); Escape (the `pause` action) pauses.
// Screens (lib/ui/menus.dart; ADR-0062: game UI is plain Flutter): the
// title screen first, then the game, the pause menu on Escape, and the
// settings panel over either menu. Which one shows is `HudModel.screen`;
// the world runs only while it is `Screen.playing` (the title shows over
// the scene's first frame, so keys on a menu never move the cube). A menu
// is driven by mouse, keyboard or controller alike: `setMenuOpen` hands it
// the controller (D-pad or stick to move, A to press, B or Start back).
// HUD: the mover's readout is bound to the game's
// `HudModel` fields (`ModelBuilder`; docs/hud.md) — behaviours write them,
// the bound widgets rebuild on change only; the stats block refreshes once
// a second from `onStats` (`FynkelFrameStats`: where the frame goes —
// behaviours, native ticks, render(), the GPU's pass times, the Dart heap
// when the build carries a VM service; docs/instrumentation.md). Run with
// FYNKEL_CAPTURE=1 to get the same as one `[stats] key=value …` line a
// second on stdout (`FynkelFrameStats.summary()`, greppable from a script).
// World labels: a `ProjectionLayer` pins `WorldLabel`s to the Cube and the
// Crate through the world's bounded projection view (one call per frame,
// no rebuilds).
// One entry point (ADR-0021): `main` runs lib/fynkel_game.dart's
// `buildGame` with the launch from the environment; the editor's Play
// mounts the same `buildGame` in its Game view with the open project and
// scene. Quit with `FynkelApplication.quit()`, never `exit()`: in the
// editor it ends Play instead of closing the editor.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel/gameplay.dart' show FramePlan, FynkelLoadError, SettingsStore, SystemKind, SystemTime, Vec2, Vec3;
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'fynkel_game.dart';
import 'game/game.dart';
import 'game/hud_model.dart';
import 'game/showcase.dart';
import 'game/spinner.dart';
import 'ui/menus.dart';

/// `FYNKEL_CAPTURE=1` prints the stats line once a second (benchmarks).
final bool capturing = (Platform.environment['FYNKEL_CAPTURE'] ?? '').isNotEmpty;

void main() => runApp(buildGame(FynkelLaunch.fromEnvironment()));

class GameApp extends StatelessWidget {
  const GameApp({super.key, this.launch = const FynkelLaunch()});

  /// Where to start (see [GamePage.launch]).
  final FynkelLaunch launch;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'UI Demo',
      debugShowCheckedModeBanner: false,
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
  Game? _game;
  String? _error;
  FynkelViewportController? _viewport;
  FynkelFrameStats? _stats;
  final _input = FynkelInputCapture();
  final _view = GlobalKey<FynkelGameViewState>();

  /// The demo's crowd with health bars, minimap and cube camera.
  Showcase? _showcase;
  final _clock = Stopwatch()..start();

  /// The scene Main menu returns to (a fresh copy of it).
  String? _startScene;

  /// Back on a menu: Escape, or a controller's east (B) or menu (Start)
  /// button — the view hands a menu the controller as these keys
  /// (`setMenuOpen`).
  static final _back = {
    LogicalKeyboardKey.escape,
    LogicalKeyboardKey.gameButtonB,
    LogicalKeyboardKey.gameButtonStart,
  };

  /// Q (in the game): the next quality profile (the project default,
  /// `medium`, first). Back on a menu: close the settings panel, or resume
  /// from the pause menu (the loop does not run on a menu, so the `pause`
  /// action cannot be read then). Every other key goes to the input
  /// capture (WASD, Space, …) in the game, and moves through a menu's
  /// buttons on a menu (the arrows; Enter or Space press).
  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    final hud = _game!.hud;
    if (_back.contains(event.logicalKey)) {
      if (hud.settingsOpen.value) {
        hud.settingsOpen.value = false;
        return KeyEventResult.handled;
      }
      if (hud.screen.value == Screen.paused) {
        _show(Screen.playing);
        return KeyEventResult.handled;
      }
      return KeyEventResult.ignored;
    }
    if (event.logicalKey != LogicalKeyboardKey.keyQ || hud.screen.value != Screen.playing) {
      return KeyEventResult.ignored;
    }
    final current = qualityProfiles.indexOf(_viewport!.quality ?? 'medium');
    _setQuality(qualityProfiles[(current + 1) % qualityProfiles.length]);
    // Remembered in the user's settings.json for the next launch.
    _game!.settings.update(_game!.settings.current.copyWith(quality: _viewport!.quality));
    return KeyEventResult.handled;
  }

  void _setQuality(String profile) => setState(() => _viewport!.quality = profile);

  /// Read after each frame. On a menu the world stops here, after the
  /// frame it drew (the title's first frame shows the scene behind it). In
  /// the game, the `pause` action (Escape, the gamepad's menu button) opens
  /// the pause menu: the app owns pausing, so it reads the action here
  /// rather than in a behaviour.
  void _onFrame(double dt, FramePlan plan) {
    final showcase = _showcase!;
    if (showcase.ensure()) setState(() {});
    showcase.tick(_clock.elapsedMicroseconds / 1e6);
    final screen = _game!.hud.screen.value;
    if (screen != Screen.playing) {
      _view.currentState
        ?..setPaused(true)
        ..setMenuOpen(true);
    } else if (_game!.world.input.pressed('pause')) {
      _show(Screen.paused);
    }
  }

  /// Switch screens; the world runs only while playing, and a menu has
  /// the keys and the controller (the D-pad or the stick move between
  /// buttons, A presses, B goes back).
  void _show(Screen screen) {
    final hud = _game!.hud;
    hud.settingsOpen.value = false;
    hud.screen.value = screen;
    final menu = screen != Screen.playing;
    _view.currentState
      ?..setPaused(menu)
      ..setMenuOpen(menu);
  }

  /// Main menu: back to the title, and a fresh start scene for the next
  /// Play (the transition runs at the first frame the world runs).
  void _toTitle() {
    if (_startScene case final scene?) _game!.scenes.transition(scene);
    _show(Screen.title);
  }

  /// Quit on desktop; phones close apps themselves. In the editor's Play
  /// this ends Play.
  VoidCallback? get _quit =>
      Platform.isMacOS || Platform.isWindows || Platform.isLinux ? FynkelApplication.quit : null;

  @override
  void initState() {
    super.initState();
    _start();
  }

  Future<void> _start() async {
    try {
      final launch = widget.launch;
      final dir = launch.projectDir;
      final settings = SettingsStore.defaultPath('UI Demo');
      if (dir != null && Directory(dir).existsSync()) {
        _game = Game.load(dir, cacheDir: launch.cacheDir, settingsPath: settings);
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
        _game = Game.load(dir ?? '(bundled)', cacheDir: cache, settingsPath: settings);
      }
      // The editor's Play (or FYNKEL_SCENE): the open scene instead of the
      // startup scene, loaded at the first frame.
      final start = launch.startScene;
      if (start != null && start != _game!.scenes.current) _game!.scenes.transition(start);
      _startScene = start ?? _game!.scenes.current;
      // The viewport draws from the world's asset store (the cooked cache
      // the scene came from); scene entities without a mesh stay invisible
      // (placeholders off) as they will in the shipped game.
      _viewport = FynkelViewportController(placeholders: false);
      _showcase = Showcase(_game!.world, crowdSize: int.tryParse(Platform.environment['FYNKEL_CROWD'] ?? '') ?? 300)
        ..ensure();
      // The profile the user picked last time (empty: the project's default).
      final quality = _game!.settings.current.quality;
      if (qualityProfiles.contains(quality)) _viewport!.quality = quality;
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
    _showcase?.dispose();
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
        key: _view,
        world: game.world,
        controller: _viewport,
        input: _input,
        // Opens the output device (docs/audio.md); the view suspends it
        // while the app is hidden and closes it when disposed. Without a
        // device (CI, a headless box) the game still runs, silently.
        audio: true,
        onKeyEvent: _onKey,
        onFrame: _onFrame,
        // Once a second. `profileDartHeap` adds the Dart heap's allocation
        // rate and GC pauses (C3) to the stats when the build has a VM
        // service (debug, profile); a release build reports none.
        profileDartHeap: true,
        onStats: (s) {
          setState(() => _stats = s);
          if (capturing) debugPrint('[stats] ${s.summary()}');
        },
        overlay: ModelBinding<HudModel>(
          model: game.hud,
          child: Stack(
            fit: StackFit.expand,
            children: [
              // Labels pinned to scene objects: the layer sizes the
              // projection view from its own layout and moves the labels
              // with paint transforms as the objects move.
              ProjectionLayer(
                world: game.world,
                children: [
                  if (game.world.find('Cube') case final cube?)
                    WorldLabel(target: cube, offset: const Vec3(0, 0.9, 0), text: 'Cube'),
                  if (game.world.find('Crate') case final crate?)
                    WorldLabel(target: crate, offset: const Vec3(0, 0.9, 0), text: 'Crate'),
                ],
              ),
              // A health bar over every crowd member: one painter, no
              // widget per bar (docs/hud.md § Many marks).
              ProjectedMarks(marks: _showcase!.bars),
              // Two more cameras of the same world, shown in panels.
              if (_showcase!.minimap case final cam?)
                Positioned(
                  right: 16,
                  top: 16,
                  width: 180,
                  height: 180,
                  child: _Panel(label: 'Minimap', circle: true, child: CameraView(world: game.world, camera: cam)),
                ),
              if (_showcase!.cubeCam case final cam?)
                Positioned(
                  right: 16,
                  bottom: 16,
                  width: 260,
                  height: 160,
                  child: _Panel(label: 'Cube camera', child: CameraView(world: game.world, camera: cam)),
                ),
              // The project's `touch:overlay/*` bindings as a stick and
              // buttons, on touch platforms only (`visible:` to force it).
              TouchOverlay.fromActions(game.report.inputActions, input: _input),
              Padding(
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
              // The screen on top: nothing in the game, a menu otherwise,
              // the settings panel over either menu.
              ModelBuilder<Screen>(
                value: game.hud.screen,
                builder: (context, screen, _) => ModelBuilder<bool>(
                  value: game.hud.settingsOpen,
                  builder: (context, settingsOpen, _) => switch ((screen, settingsOpen)) {
                    (Screen.playing, _) => const SizedBox.shrink(),
                    (_, true) => SettingsPanel(
                        settings: game.settings,
                        onBack: () => game.hud.settingsOpen.value = false,
                        onQuality: _setQuality,
                      ),
                    (Screen.title, false) => TitleScreen(
                        title: 'UI Demo',
                        onPlay: () => _show(Screen.playing),
                        onSettings: () => game.hud.settingsOpen.value = true,
                        onQuit: _quit,
                      ),
                    (Screen.paused, false) => PauseMenu(
                        onResume: () => _show(Screen.playing),
                        onSettings: () => game.hud.settingsOpen.value = true,
                        onMainMenu: _toTitle,
                        onQuit: _quit,
                      ),
                  },
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// The HUD: ordinary Flutter widgets composited over the 3D viewport. The
/// stats lines rebuild once a second (`onStats`); the mover's readout is
/// bound to the `HudModel` fields the Mover writes ([MoverReadout]).
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
      'UI Demo',
      'ui demo    ${_showcaseLine(game)}',
      'scene      ${game.report.sceneName} (${game.report.entityCount} entities)',
      'tick       ${game.world.tick}',
      'behaviours ${game.world.behaviours.length} attached',
      for (final sp in game.world.behaviours.whereType<Spinner>())
        'spinner    ${sp.angle.toStringAsFixed(2)} rad at ${sp.speed} rad/s',
      'input      move ${_xy(game.world.input.action2D('move'))}  jump ${game.world.input.held('jump') ? 'held' : '-'}  '
          '[WASD/stick, Space/A; Esc pauses]',
      'assets     ${game.report.assetCount} retained from ${game.report.cacheDir}',
      'quality    ${viewport.quality ?? 'medium (project default)'}  [Q cycles]',
      if (s != null) ...[
        'loop       ${s.fps.toStringAsFixed(0)}/s  scene ${s.textureAcquisitionFps?.toStringAsFixed(0) ?? '-'} new/s  frame ${s.tickInterval.p50.toStringAsFixed(2)} ms '
            '(p99 ${s.tickInterval.p99.toStringAsFixed(2)})  render ${s.renderCall.p50.toStringAsFixed(2)} ms',
        'simulate   ${s.simulate.p50.toStringAsFixed(2)} ms (p99 ${s.simulate.p99.toStringAsFixed(2)})  '
            'viewport ${s.physicalWidth}x${s.physicalHeight}  ${s.fixedTicks} ticks/s  '
            '${s.missedDeadlines} missed, ${s.dropped} dropped',
        // Where the frame goes (M3-16): the Dart behaviours, the native
        // ticks, the render call's extract/prepare/encode, the GPU's own
        // pass times (Metal timestamps; they trail the draw by a few
        // frames) — the p95 of the total work is the blueprint's 12 ms.
        'work       behaviours ${s.behaviours.p95.toStringAsFixed(2)}  native ${s.nativeStep.p95.toStringAsFixed(2)}  '
            'render ${s.renderCall.p95.toStringAsFixed(2)}  total ${s.totalWork.p95.toStringAsFixed(2)} ms (p95)',
        if (s.viewport case final v?) ...[
          'gpu        ${v.gpuFrame.p95.toStringAsFixed(2)} ms frame (p95; shadow ${v.gpuShadow.p95.toStringAsFixed(2)}, '
              'scene ${v.gpuScene.p95.toStringAsFixed(2)}, post ${v.gpuPost.p95.toStringAsFixed(2)})  '
              '${(v.gpuBytes / 1048576).toStringAsFixed(0)} MB',
          'memory     native ${(s.world.nativeBytes / 1048576).toStringAsFixed(1)} MB  '
              'resident ${(s.world.residentBytes / 1048576).toStringAsFixed(0)} MB  '
              'snapshot ${s.world.snapshotBytes} B',
        ],
        if (s.dartHeap case final h?)
          'dart heap  ${(h.heapUsage / 1048576).toStringAsFixed(1)} MB  '
              '${(h.allocatedBytes / 1024 / (s.frames == 0 ? 1 : s.frames)).toStringAsFixed(1)} KB/frame allocated  '
              '${h.gcCollections} gc (${h.gcPauseMs.toStringAsFixed(1)} ms)',
        // The last tick's phases (`World.systemTimes`, M3-10): where a
        // slow tick goes — physics, the native movers, the transforms.
        'systems    ${_systems(s.systems)}',
        if (s.viewport case final v?) 'meshes     ${v.instances} drawn in ${v.draws} draws, ${v.culled} culled, ${v.missing} missing, ${v.assetErrors} errors',
      ],
      for (final d in game.report.diagnostics) '$d',
    ];
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (final l in lines) Text(l, style: style),
        MoverReadout(style: style),
      ],
    );
  }

  /// The step's total, then every phase that touched something.
  static String _systems(List<SystemTime> times) => [
        for (final t in times)
          if (t.system == SystemKind.step || t.entities > 0) '${t.system.name} ${t.micros} µs${t.system == SystemKind.step ? '' : ' (${t.entities})'}',
      ].join('  ');

  static String _xy(Vec2 v) => '(${v.x.toStringAsFixed(2)}, ${v.y.toStringAsFixed(2)})';
}

/// The mover's line, one widget per model field: `position` rebuilds its
/// text when the cube moves, `jumps` when a jump starts, `airborne` at
/// take-off and landing — never on a frame where nothing changed.
class MoverReadout extends StatelessWidget {
  const MoverReadout({super.key, this.style});

  final TextStyle? style;

  @override
  Widget build(BuildContext context) {
    final hud = ModelBinding.of<HudModel>(context);
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Text('mover      ', style: style),
        ModelBuilder<Vec3>(value: hud.position, builder: (_, p, _) => Text(_xyz(p), style: style)),
        ModelBuilder<int>(value: hud.jumps, builder: (_, n, _) => Text('  $n jumps', style: style)),
        ModelBuilder<bool>(value: hud.airborne, builder: (_, air, _) => Text(air ? ' (airborne)' : '', style: style)),
      ],
    );
  }

  static String _xyz(Vec3 v) => '(${v.x.toStringAsFixed(2)}, ${v.y.toStringAsFixed(2)}, ${v.z.toStringAsFixed(2)})';
}

String _showcaseLine(Game game) =>
    '${game.world.projection.length} projected points (health bars), minimap + cube camera (CameraView)';

/// A framed panel with a caption.
class _Panel extends StatelessWidget {
  const _Panel({required this.label, required this.child, this.circle = false});

  final String label;
  final Widget child;
  final bool circle;

  @override
  Widget build(BuildContext context) {
    final picture = circle ? ClipOval(child: child) : ClipRRect(borderRadius: BorderRadius.circular(10), child: child);
    return Stack(
      fit: StackFit.expand,
      children: [
        DecoratedBox(
          position: DecorationPosition.foreground,
          decoration: BoxDecoration(
            shape: circle ? BoxShape.circle : BoxShape.rectangle,
            borderRadius: circle ? null : BorderRadius.circular(10),
            border: Border.all(color: Colors.white70, width: 2),
          ),
          child: picture,
        ),
        Align(
          alignment: circle ? Alignment.bottomCenter : Alignment.topLeft,
          child: Padding(
            padding: const EdgeInsets.all(6),
            child: DecoratedBox(
              decoration: BoxDecoration(color: Colors.black54, borderRadius: BorderRadius.circular(4)),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                child: Text(label, style: const TextStyle(fontSize: 12, color: Colors.white)),
              ),
            ),
          ),
        ),
      ],
    );
  }
}
