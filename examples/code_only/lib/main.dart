// Fynkel Code Only — the app. The game (lib/game/) is made from Dart
// alone; this file is the Flutter around it: `FynkelGameView` owns the
// loop and draws the world, and the overlay — the title, the HUD, the
// results — is ordinary widgets bound to the game's `GameState`
// (docs/hud.md), switching on `GameState.screen`.
//
// How the places are made: built live from the specs (the default), or
// loaded from the scene documents tool/scenes.dart wrote from them —
// `CODE_ONLY_MODE=scenes` in the environment (or
// `--dart-define=CODE_ONLY_MODE=scenes`). The editor's Play starts in the
// scene it has open, so it uses the documents.
//
// One entry point (ADR-0021): `main` runs lib/fynkel_game.dart's
// `buildGame` with the launch from the environment; the editor's Play
// mounts the same `buildGame`.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:fynkel/fynkel.dart' show FramePlan, FynkelLoadError, SettingsStore;
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'fynkel_game.dart';
import 'game/game.dart';
import 'game/game_state.dart';
import 'generated/assets.g.dart';

void main() => runApp(buildGame(FynkelLaunch.fromEnvironment()));

/// [BuildMode.scenes] when asked for, [BuildMode.live] otherwise.
BuildMode modeFromEnvironment() {
  const defined = String.fromEnvironment('CODE_ONLY_MODE');
  final mode = defined.isNotEmpty ? defined : Platform.environment['CODE_ONLY_MODE'] ?? '';
  return mode == 'scenes' ? BuildMode.scenes : BuildMode.live;
}

class GameApp extends StatelessWidget {
  const GameApp({super.key, this.launch = const FynkelLaunch()});

  final FynkelLaunch launch;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Fynkel Code Only',
      debugShowCheckedModeBanner: false,
      theme: ThemeData.dark(useMaterial3: true),
      home: GamePage(launch: launch),
    );
  }
}

class GamePage extends StatefulWidget {
  const GamePage({super.key, this.launch = const FynkelLaunch()});

  final FynkelLaunch launch;

  @override
  State<GamePage> createState() => _GamePageState();
}

class _GamePageState extends State<GamePage> {
  Game? _game;
  String? _error;
  FynkelViewportController? _viewport;
  final _input = FynkelInputCapture();
  final _view = GlobalKey<FynkelGameViewState>();

  @override
  void initState() {
    super.initState();
    _start();
  }

  Future<void> _start() async {
    try {
      final launch = widget.launch;
      final dir = launch.projectDir;
      final settings = SettingsStore.defaultPath('Fynkel Code Only');
      // The editor's Play opens a scene: the documents, then.
      final mode = launch.startScene != null ? BuildMode.scenes : modeFromEnvironment();
      if (dir != null && Directory(dir).existsSync()) {
        _game = Game.load(dir, mode: mode, cacheDir: launch.cacheDir, settingsPath: settings);
      } else {
        // Not on the machine that holds the project (a device): the cache
        // the app bundles.
        final cache = await unpackBundledCache();
        if (cache == null) {
          throw StateError('No project directory and no bundled cache: run `fynkel run` from the project.');
        }
        _game = Game.load(dir ?? '(bundled)', mode: mode, cacheDir: cache, settingsPath: settings);
      }
      final start = launch.startScene;
      if (start != null && start != _game!.world.scenes.current) {
        if (start == Assets.scenes.level.asset) _game!.state.reset();
        _game!.world.scenes.transition(start);
      }
      _viewport = FynkelViewportController(placeholders: false);
    } on FynkelLoadError catch (e) {
      _error = '$e\nrun `fynkel cook` in the project (or `fynkel run`, which cooks first)';
    } catch (e) {
      _error = '$e';
    }
    if (mounted) setState(() {});
  }

  /// The `pause` action, read after each frame: pausing is the app's.
  void _onFrame(double dt, FramePlan plan) {
    final game = _game!;
    if (game.state.screen.value == Screen.playing && game.world.input.pressed('pause')) _setPaused(true);
  }

  void _setPaused(bool paused) {
    _game!.state.paused.value = paused;
    _view.currentState?.setPaused(paused);
  }

  void _click() => _game!.world.audio.play(Assets.audio.click, presentation: true);

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
        key: _view,
        world: game.world,
        controller: _viewport,
        input: _input,
        audio: true,
        onFrame: _onFrame,
        overlay: ExcludeFocus(
          child: ModelBinding<GameState>(
            model: game.state,
            child: ModelBuilder<Screen>(
              value: game.state.screen,
              builder: (context, screen, _) => switch (screen) {
                Screen.menu => _Panel(
                    title: 'Fynkel Code Only',
                    lines: [
                      'Five coins, a gate, a switch, a sweeper — and not one line of scene JSON.',
                      'WASD to walk, Space to jump, E at the switch, R to start over, Esc to pause.',
                      'Built ${game.mode == BuildMode.live ? 'live from the specs' : 'from the written scenes'}.',
                    ],
                    actions: {
                      'Play': () {
                        _click();
                        game.play();
                      },
                    },
                  ),
                Screen.playing => Stack(
                    fit: StackFit.expand,
                    children: [
                      TouchOverlay.fromActions(game.report.inputActions, input: _input),
                      const Hud(),
                      ModelBuilder<bool>(
                        value: game.state.paused,
                        builder: (_, paused, _) => paused
                            ? _Panel(
                                title: 'Paused',
                                actions: {
                                  'Resume': () => _setPaused(false),
                                  'Give up': () {
                                    _setPaused(false);
                                    game.finish();
                                  },
                                },
                              )
                            : const SizedBox.shrink(),
                      ),
                    ],
                  ),
                Screen.results => ModelBuilder<RunResult?>(
                    value: game.state.lastRun,
                    builder: (_, run, _) => _Panel(
                      title: run?.won ?? false ? 'Every coin!' : 'Run over',
                      lines: [
                        if (run != null) ...[
                          '${run.collected} of ${run.total} coins in ${run.clock}',
                          '${run.deaths} ${run.deaths == 1 ? 'death' : 'deaths'}',
                        ],
                      ],
                      actions: {
                        'Play again': () {
                          _click();
                          game.play();
                        },
                        'Menu': () {
                          _click();
                          game.toMenu();
                        },
                      },
                    ),
                  ),
              },
            ),
          ),
        ),
      ),
    );
  }
}

/// The level's HUD: the coins, the clock, the deaths, the switch's hint
/// and the win — each bound to its field, rebuilt when it changes.
class Hud extends StatelessWidget {
  const Hud({super.key});

  @override
  Widget build(BuildContext context) {
    final state = ModelBinding.of<GameState>(context);
    final text = Theme.of(context).textTheme;
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Stack(
        children: [
          Align(
            alignment: Alignment.topLeft,
            child: _Box(
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  const Icon(Icons.circle, color: Color(0xFFFFC74D), size: 18),
                  const SizedBox(width: 8),
                  ModelBuilder<int>(
                    value: state.collected,
                    builder: (_, n, _) => ModelBuilder<int>(
                      value: state.total,
                      builder: (_, total, _) => Text('$n / $total', style: text.titleLarge),
                    ),
                  ),
                  const SizedBox(width: 20),
                  ModelBuilder<int>(
                    value: state.elapsedTenths,
                    builder: (_, t, _) => Text(formatTenths(t), style: text.titleLarge?.copyWith(fontFeatures: const [FontFeature.tabularFigures()])),
                  ),
                  ModelBuilder<int>(
                    value: state.deaths,
                    builder: (_, d, _) => d == 0 ? const SizedBox.shrink() : Text('   ✕ $d', style: text.titleLarge),
                  ),
                ],
              ),
            ),
          ),
          Align(
            alignment: const Alignment(0, 0.7),
            child: ModelBuilder<String>(
              value: state.hint,
              builder: (_, hint, _) => hint.isEmpty ? const SizedBox.shrink() : _Box(child: Text(hint, style: text.titleMedium)),
            ),
          ),
          Align(
            alignment: const Alignment(0, -0.3),
            child: ModelBuilder<bool>(
              value: state.won,
              builder: (_, won, _) => won
                  ? Text('All five!', style: text.displayMedium?.copyWith(color: const Color(0xFFFFC74D), fontWeight: FontWeight.bold))
                  : const SizedBox.shrink(),
            ),
          ),
        ],
      ),
    );
  }
}

class _Box extends StatelessWidget {
  const _Box({required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) => DecoratedBox(
        decoration: BoxDecoration(color: Colors.black.withValues(alpha: 0.55), borderRadius: BorderRadius.circular(8)),
        child: Padding(padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10), child: child),
      );
}

/// A centred card: a title, a few lines, buttons.
class _Panel extends StatelessWidget {
  const _Panel({required this.title, this.lines = const [], this.actions = const {}});

  final String title;
  final List<String> lines;
  final Map<String, VoidCallback> actions;

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    return Center(
      child: _Box(
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(title, style: text.headlineMedium),
              const SizedBox(height: 12),
              for (final l in lines) Text(l, style: text.bodyLarge, textAlign: TextAlign.center),
              if (actions.isNotEmpty) const SizedBox(height: 16),
              Wrap(
                spacing: 12,
                children: [for (final MapEntry(key: label, value: onPressed) in actions.entries) FilledButton(onPressed: onPressed, child: Text(label))],
              ),
            ],
          ),
        ),
      ),
    );
  }
}
