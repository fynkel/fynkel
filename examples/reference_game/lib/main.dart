// Fynkel Reference Game — entry point.
//
// `FynkelGameView` (package:fynkel_flutter) owns the loop (ADR-0007): once
// per Flutter frame it hands the frame time to the world, which runs every
// behaviour's fixedUpdate for the planned ticks, steps the native runtime and
// runs update/lateUpdate; then it renders the world into the viewport
// texture. Flutter widgets drawn over it are the HUD. The scene is
// fynkel.project.json's startupScene (the Playground); meshes come from the
// project's cooked cache (`fynkel cook`, which `fynkel run` does for you):
// by path when this process can see the project directory, else from the
// copy the app bundles (fynkel_cache/, unpacked at start-up) — which is
// how the same build runs on an iPhone.
//
// Input: the view captures keys, the mouse and gamepads into a
// `FynkelInputCapture` and hands each frame's events to the world, which
// turns them into the project's actions (`inputActions`; behaviours read
// `Input.action2D('move')` and friends; the camera rig reads `look`). A
// click captures the mouse for looking around (the pointer hides and stops
// at the edges); Escape (the `pause` action) pauses and releases it. On a
// phone a `TouchOverlay` puts the same actions on screen.
//
// HUD: the coin counter, the clock, the deaths, the switch's hint, the win
// and pause banners are bound to the game's `GameState` fields
// (`ModelBuilder`; docs/hud.md) — behaviours write them, the bound widgets
// rebuild on change only. A `ProjectionLayer` pins a `WorldLabel` to the
// Switch (`ProjectedAnchor`). The stats block refreshes once a second from `onStats`; F1 toggles
// it, Q cycles the quality profiles.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel/fynkel.dart' show FramePlan, FynkelLoadError, SettingsStore, Vec3;
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'game/game.dart';
import 'game/game_state.dart';

void main() => runApp(const GameApp());

class GameApp extends StatelessWidget {
  const GameApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Fynkel Reference Game',
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
  final _input = FynkelInputCapture();
  final _view = GlobalKey<FynkelGameViewState>();
  bool _paused = false;
  bool _showStats = false;

  /// Escape while paused: resume (the loop does not run while paused, so
  /// the `pause` action cannot be read then). F1: the stats block. Q: the
  /// next quality profile (the project default, `medium`, first). Every
  /// other key goes to the input capture (WASD, Space, E, R, …).
  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    if (_paused && event.logicalKey == LogicalKeyboardKey.escape) {
      _setPaused(false);
      return KeyEventResult.handled;
    }
    if (event.logicalKey == LogicalKeyboardKey.f1) {
      setState(() => _showStats = !_showStats);
      return KeyEventResult.handled;
    }
    if (event.logicalKey != LogicalKeyboardKey.keyQ) return KeyEventResult.ignored;
    final viewport = _viewport!;
    final current = profiles.indexOf(viewport.quality ?? 'medium');
    final next = profiles[(current + 1) % profiles.length];
    setState(() => viewport.quality = next);
    // Remembered in the user's settings.json for the next launch.
    _game!.settings.update(_game!.settings.current.copyWith(quality: next));
    return KeyEventResult.handled;
  }

  /// The `pause` action (Escape, the gamepad's menu button), read after
  /// each frame: the app owns pausing, so it reads the action here rather
  /// than in a behaviour.
  void _onFrame(double dt, FramePlan plan) {
    if (_game!.world.input.pressed('pause')) _setPaused(true);
  }

  void _setPaused(bool v) {
    setState(() => _paused = v);
    _game!.state.paused.value = v;
    final view = _view.currentState;
    view?.setPaused(v);
    // The pointer is free while paused; a click takes it back.
    if (v) view?.capturePointer(false);
  }

  @override
  void initState() {
    super.initState();
    _start();
  }

  Future<void> _start() async {
    try {
      final dir = projectDirectory();
      final settings = SettingsStore.defaultPath('Fynkel Reference Game');
      if (dir != null && Directory(dir).existsSync()) {
        _game = Game.load(dir, settingsPath: settings);
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
      // The viewport draws from the world's asset store (the cooked cache
      // the scene came from); scene entities without a mesh stay invisible
      // (placeholders off) as they will in the shipped game.
      _viewport = FynkelViewportController(placeholders: false);
      // The profile the user picked last time (empty: the project's default).
      final quality = _game!.settings.current.quality;
      if (profiles.contains(quality)) _viewport!.quality = quality;
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
      return Scaffold(
        body: Center(child: Text(_error ?? '', textAlign: TextAlign.center)),
      );
    }
    return Scaffold(
      body: FynkelGameView(
        key: _view,
        world: game.world,
        controller: _viewport,
        input: _input,
        onKeyEvent: _onKey,
        onFrame: _onFrame,
        onStats: (s) => setState(() => _stats = s),
        overlay: ModelBinding<GameState>(
          model: game.state,
          child: Stack(
            fit: StackFit.expand,
            children: [
              // A click takes the mouse for the `look` action (desktop);
              // Escape gives it back.
              Listener(
                behavior: HitTestBehavior.translucent,
                onPointerDown: (_) {
                  if (!_paused && !(_view.currentState?.pointerCaptured ?? false)) {
                    _view.currentState?.capturePointer(true);
                  }
                },
              ),
              // The switch's prompt, pinned to the pad through the world's
              // bounded projection view (one call per frame, no rebuilds).
              ProjectionLayer(
                world: game.world,
                children: [
                  if (game.world.find('Switch') case final pad?)
                    ProjectedAnchor(target: pad, offset: const Vec3(0, 1.2, 0), child: const SwitchPrompt()),
                ],
              ),
              // The project's `touch:overlay/*` bindings as a stick and
              // buttons, on touch platforms only (`visible:` to force it).
              TouchOverlay.fromActions(game.report.inputActions, input: _input),
              const Padding(
                padding: EdgeInsets.all(16),
                child: Align(alignment: Alignment.topLeft, child: Counter()),
              ),
              if (_showStats)
                Padding(
                  padding: const EdgeInsets.all(16),
                  child: Align(
                    alignment: Alignment.topRight,
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        color: Colors.black.withValues(alpha: 0.55),
                        borderRadius: BorderRadius.circular(8),
                      ),
                      child: Padding(
                        padding: const EdgeInsets.all(12),
                        child: StatsBlock(game: game, stats: _stats, viewport: _viewport!),
                      ),
                    ),
                  ),
                ),
              const Align(
                alignment: Alignment.bottomCenter,
                child: Padding(padding: EdgeInsets.all(24), child: Hint()),
              ),
              WinBanner(onRestart: game.restart),
              const PauseBanner(),
            ],
          ),
        ),
      ),
    );
  }
}

/// The HUD counter: `Coins n / 10`, the clock and the deaths, one widget per
/// model field — `collected` rebuilds when a coin is taken, the clock
/// every tenth, `deaths` on a death, never on a frame where nothing
/// changed.
class Counter extends StatelessWidget {
  const Counter({super.key});

  @override
  Widget build(BuildContext context) {
    final state = ModelBinding.of<GameState>(context);
    final theme = Theme.of(context).textTheme;
    final big = theme.headlineSmall?.copyWith(fontWeight: FontWeight.bold, shadows: const [Shadow(blurRadius: 6)]);
    final small = theme.bodyLarge?.copyWith(shadows: const [Shadow(blurRadius: 6)]);
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text('Coins ', style: big),
            ModelBuilder<int>(
              value: state.collected,
              builder: (_, n, _) => Text('$n', style: big),
            ),
            Text(' / ', style: big),
            ModelBuilder<int>(
              value: state.total,
              builder: (_, n, _) => Text('$n', style: big),
            ),
          ],
        ),
        ModelBuilder<int>(
          value: state.elapsedTenths,
          builder: (_, t, _) => Text(
            '${(t ~/ 600).toString().padLeft(2, '0')}:${((t ~/ 10) % 60).toString().padLeft(2, '0')}.${t % 10}',
            style: small,
          ),
        ),
        ModelBuilder<int>(
          value: state.deaths,
          builder: (_, n, _) => Text(n == 0 ? '' : 'deaths $n', style: small),
        ),
      ],
    );
  }
}

/// The prompt over the switch (`E — open the gate`), visible while the
/// player stands on it.
class SwitchPrompt extends StatelessWidget {
  const SwitchPrompt({super.key});

  @override
  Widget build(BuildContext context) {
    final state = ModelBinding.of<GameState>(context);
    return ModelBuilder<String>(
      value: state.hint,
      builder: (context, hint, _) => hint.isEmpty
          ? const SizedBox.shrink()
          : DecoratedBox(
              decoration: BoxDecoration(
                color: Colors.cyan.withValues(alpha: 0.8),
                borderRadius: BorderRadius.circular(6),
              ),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                child: Text(hint, style: Theme.of(context).textTheme.bodyMedium?.copyWith(color: Colors.black)),
              ),
            ),
    );
  }
}

/// The bottom line: the controls until the first coin, then nothing.
class Hint extends StatelessWidget {
  const Hint({super.key});

  @override
  Widget build(BuildContext context) {
    final state = ModelBinding.of<GameState>(context);
    return ModelBuilder<int>(
      value: state.collected,
      builder: (context, n, _) => n > 0
          ? const SizedBox.shrink()
          : Text(
              'WASD / left stick: walk   mouse / right stick: look   Space / A: jump   Shift / LB: sprint   E / X: use   R / Y: restart',
              style: Theme.of(context).textTheme.bodyMedium?.copyWith(shadows: const [Shadow(blurRadius: 6)]),
              textAlign: TextAlign.center,
            ),
    );
  }
}

/// The win screen: shown while `GameState.won` is set.
class WinBanner extends StatelessWidget {
  const WinBanner({super.key, required this.onRestart});

  final VoidCallback onRestart;

  @override
  Widget build(BuildContext context) {
    final state = ModelBinding.of<GameState>(context);
    return ModelBuilder<bool>(
      value: state.won,
      builder: (context, won, _) => won
          ? ColoredBox(
              color: Colors.black.withValues(alpha: 0.5),
              child: Center(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text('All coins collected!', style: Theme.of(context).textTheme.headlineMedium),
                    const SizedBox(height: 8),
                    ModelBuilder<int>(
                      value: state.elapsedTenths,
                      builder: (_, t, _) =>
                          Text('${(t / 10).toStringAsFixed(1)} s', style: Theme.of(context).textTheme.titleLarge),
                    ),
                    const SizedBox(height: 16),
                    FilledButton(onPressed: onRestart, child: const Text('Play again (R)')),
                  ],
                ),
              ),
            )
          : const SizedBox.shrink(),
    );
  }
}

/// The pause screen: shown while `GameState.paused` is set (the pattern for
/// any screen — menu, settings, results — is a model field the widgets
/// switch on; docs/hud.md).
class PauseBanner extends StatelessWidget {
  const PauseBanner({super.key});

  @override
  Widget build(BuildContext context) {
    final state = ModelBinding.of<GameState>(context);
    return ModelBuilder<bool>(
      value: state.paused,
      builder: (context, paused, _) => paused
          ? ColoredBox(
              color: Colors.black.withValues(alpha: 0.5),
              child: Center(child: Text('PAUSED — Esc resumes', style: Theme.of(context).textTheme.headlineMedium)),
            )
          : const SizedBox.shrink(),
    );
  }
}

/// The engine's numbers (F1): once a second from `onStats`.
class StatsBlock extends StatelessWidget {
  const StatsBlock({super.key, required this.game, required this.stats, required this.viewport});

  final Game game;
  final FynkelFrameStats? stats;
  final FynkelViewportController viewport;

  @override
  Widget build(BuildContext context) {
    final style = Theme.of(context).textTheme.bodyMedium?.copyWith(fontFamily: 'monospace');
    final s = stats;
    final lines = <String>[
      'scene      ${game.report.sceneName} (${game.report.entityCount} entities)',
      'tick       ${game.world.tick}',
      'behaviours ${game.world.behaviours.length} attached',
      'quality    ${viewport.quality ?? 'medium (project default)'}  [Q cycles]',
      if (s != null) ...[
        'fps        ${s.fps.toStringAsFixed(0)}  frame ${s.tickInterval.p50.toStringAsFixed(2)} ms '
            '(p99 ${s.tickInterval.p99.toStringAsFixed(2)})  render ${s.renderCall.p50.toStringAsFixed(2)} ms',
        'simulate   ${s.simulate.p50.toStringAsFixed(2)} ms (p99 ${s.simulate.p99.toStringAsFixed(2)})  '
            '${s.fixedTicks} ticks/s  ${s.missedDeadlines} missed, ${s.dropped} dropped',
        if (s.viewport case final v?)
          'meshes     ${v.instances} drawn in ${v.draws} draws, ${v.culled} culled, ${v.missing} missing',
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
