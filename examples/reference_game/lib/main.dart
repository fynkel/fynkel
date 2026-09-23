// Fynkel Reference Game — entry point.
//
// `FynkelGameView` (package:fynkel_flutter) owns the loop (ADR-0007): once
// per Flutter frame it hands the frame time to the world, which runs every
// behaviour's fixedUpdate for the planned ticks, steps the native runtime and
// runs update/lateUpdate; then it renders the world into the viewport
// texture. Flutter widgets drawn over it are the screens and the HUD. The
// first scene is fynkel.project.json's startupScene (the Menu); meshes come
// from the project's cooked cache (`fynkel cook`, which `fynkel run` does
// for you): by path when this process can see the project directory, else
// from the copy the app bundles (fynkel_cache/, unpacked at start-up) —
// which is how the same build runs on an iPhone.
//
// The flow (M3-15): the Menu's Play loads the Playground, the last coin
// (or the pause menu's Give up) the Results, and from there Play again or
// Menu — every step a `scenes.transition` the next frame performs;
// `GameState.screen` follows the loads and the overlay below switches on
// it (lib/ui: menu.dart, hud.dart, settings.dart). The pause menu and the
// settings panel are the page's own state: pausing mutes the view's loop,
// the settings edit the user's settings.json through `Game.settings`.
//
// Input: the view captures keys, the mouse and gamepads into a
// `FynkelInputCapture` and hands each frame's events to the world, which
// turns them into the project's actions (`inputActions`; behaviours read
// `Input.action2D('move')` and friends; the camera rig reads `look`). A
// click captures the mouse for looking around (the pointer hides and stops
// at the edges); Escape (the `pause` action) pauses and releases it. On a
// phone a `TouchOverlay` puts the same actions on screen while playing.
// Rebinding takes the next key through the view's `onKeyEvent` and writes
// it to the settings (`SettingsDocument.rebind`), which lay it over the
// project's bindings for every later load.
//
// HUD: the coin counter, the clock, the deaths, the switch's hint and the
// win flash are bound to the game's `GameState` fields (`ModelBuilder`;
// docs/hud.md) — behaviours write them, the bound widgets rebuild on
// change only. A `ProjectionLayer` pins a `WorldLabel` to the Switch
// (`ProjectedAnchor`). The stats block refreshes once a second from
// `onStats`; F1 toggles it, Q cycles the quality profiles.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel/fynkel.dart' show FramePlan, FynkelLoadError, SceneState, SettingsStore, Vec3;
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'fynkel_game.dart';
import 'game/game.dart';
import 'game/game_state.dart';
import 'ui/hud.dart';
import 'ui/menu.dart';
import 'ui/settings.dart';

/// A build: the project and start scene from the environment
/// (`FynkelLaunch.fromEnvironment`); the editor's Play calls [buildGame]
/// itself with the open project and scene (ADR-0021).
void main() => runApp(buildGame(FynkelLaunch.fromEnvironment()));

/// `FYNKEL_CAPTURE=1` in the environment: a benchmark run — the game skips
/// the Menu for the Playground and prints its stats window to stdout
/// (`benchmarks/m3/capture.py` records the reference game this way).
final bool capturing = (Platform.environment['FYNKEL_CAPTURE'] ?? '').isNotEmpty;

class GameApp extends StatelessWidget {
  const GameApp({super.key, this.launch = const FynkelLaunch()});

  /// Where to start (see [GamePage.launch]).
  final FynkelLaunch launch;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Fynkel Reference Game',
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
  bool _paused = false;
  bool _showStats = false;
  bool _settingsOpen = false;

  /// The action whose key the settings panel waits for.
  String? _rebinding;

  /// Keys the app owns, before the input capture sees them: a rebind takes
  /// the next key; Escape closes the settings, resumes a pause (the loop
  /// does not run while paused, so the `pause` action cannot be read then)
  /// and otherwise reaches the world as the `pause` action; F1 the stats
  /// block; Q the next quality profile. Everything else goes to the input
  /// capture (WASD, Space, E, R, …).
  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    final escape = event.logicalKey == LogicalKeyboardKey.escape;
    if (_rebinding case final action?) {
      if (escape) {
        setState(() => _rebinding = null);
        return KeyEventResult.handled;
      }
      final name = FynkelInputCapture.keyName(event.physicalKey);
      if (name != null) _rebind(action, name);
      return KeyEventResult.handled;
    }
    if (escape) {
      if (_settingsOpen) {
        setState(() => _settingsOpen = false);
        return KeyEventResult.handled;
      }
      if (_paused) {
        _setPaused(false);
        return KeyEventResult.handled;
      }
      return KeyEventResult.ignored;
    }
    if (event.logicalKey == LogicalKeyboardKey.f1) {
      setState(() => _showStats = !_showStats);
      return KeyEventResult.handled;
    }
    if (event.logicalKey != LogicalKeyboardKey.keyQ) return KeyEventResult.ignored;
    final current = qualityProfiles.indexOf(_viewport!.quality ?? 'medium');
    _setQuality(qualityProfiles[(current + 1) % qualityProfiles.length]);
    return KeyEventResult.handled;
  }

  /// The profile to the viewport (the renderer reads it there) and to the
  /// user's settings.json for the next launch.
  void _setQuality(String profile) {
    setState(() => _viewport!.quality = profile);
    _game!.settings.update(_game!.settings.current.copyWith(quality: profile));
  }

  /// [action] on the keyboard key [name] for this user: the project's
  /// other devices stay (gamepad, touch), the settings store lays the new
  /// list over the project's actions at once (`SettingsStore.apply`).
  void _rebind(String action, String name) {
    final game = _game!;
    final project = game.report.inputActions.where((a) => a.name == action).firstOrNull?.bindings ?? const <String>[];
    final others = project.where((b) => !b.startsWith('keyboard:'));
    game.settings.update(game.settings.current.rebind(action, ['keyboard:$name', ...others]));
    setState(() => _rebinding = null);
  }

  /// The `pause` action (Escape, the gamepad's menu button), read after
  /// each frame while playing: the app owns pausing, so it reads the
  /// action here rather than in a behaviour.
  void _onFrame(double dt, FramePlan plan) {
    final game = _game!;
    if (!_paused && game.state.screen.value == Screen.playing && game.world.input.pressed('pause')) _setPaused(true);
  }

  void _setPaused(bool v) {
    setState(() {
      _paused = v;
      _settingsOpen = false;
      _rebinding = null;
    });
    _game!.state.paused.value = v;
    final view = _view.currentState;
    view?.setPaused(v);
    // The pointer is free while paused; a click takes it back.
    if (v) view?.capturePointer(false);
  }

  /// A scene landed: outside the level the pointer is free for the menus.
  void _onScene(SceneState s) {
    if (s == SceneState.ready && _game!.state.screen.value != Screen.playing) _view.currentState?.capturePointer(false);
  }

  /// Once a second: the HUD's stats block; under `FYNKEL_CAPTURE` also one
  /// `[stats]` line on stdout for benchmarks/m3/capture.py.
  void _onStats(FynkelFrameStats s) {
    setState(() => _stats = s);
    if (capturing) debugPrint('[stats] ${s.summary()}');
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
      final settings = SettingsStore.defaultPath('Fynkel Reference Game');
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
      _game!.scenes.addListener(_onScene);
      // A capture run skips the Menu: the Playground loads at the first
      // frame, as Play would.
      if (capturing) _game!.play();
      // The editor's Play (or FYNKEL_SCENE): the open scene instead of the
      // Menu — the Playground as a fresh run, anything else as it is.
      final start = launch.startScene;
      if (start == playgroundScene) {
        _game!.play();
      } else if (start != null && start != _game!.scenes.current) {
        _game!.scenes.transition(start);
      }
      // The viewport draws from the world's asset store (the cooked cache
      // the scene came from); scene entities without a mesh stay invisible
      // (placeholders off) as they will in the shipped game.
      _viewport = FynkelViewportController(placeholders: false);
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
        // Opens the output device (docs/audio.md): the music from the load,
        // the footsteps, pickups and hits from the behaviours, the menus'
        // clicks. The view suspends it while the app is hidden.
        audio: true,
        onKeyEvent: _onKey,
        onFrame: _onFrame,
        onStats: _onStats,
        profileDartHeap: true,
        // The buttons never take the keyboard focus from the viewport, so
        // Escape, F1, Q and a rebind's key keep reaching `_onKey`.
        overlay: ExcludeFocus(
          child: ModelBinding<GameState>(
            model: game.state,
            child: ModelBuilder<Screen>(
              value: game.state.screen,
              builder: (_, screen, _) => switch (screen) {
                Screen.menu =>
                  _settingsOpen
                      ? _settings(game)
                      : MenuScreen(game: game, onSettings: () => setState(() => _settingsOpen = true)),
                Screen.results => ResultsScreen(game: game),
                Screen.playing => _playing(game),
              },
            ),
          ),
        ),
      ),
    );
  }

  Widget _settings(Game game) => SettingsPanel(
    game: game,
    viewport: _viewport!,
    rebinding: _rebinding,
    onRebind: (action) => setState(() => _rebinding = action),
    onBack: () => setState(() {
      _settingsOpen = false;
      _rebinding = null;
    }),
  );

  /// The level: the HUD, the touch overlay, the pause menu over it all.
  Widget _playing(Game game) {
    return Stack(
      fit: StackFit.expand,
      children: [
        // A click takes the mouse for the `look` action (desktop); Escape
        // gives it back.
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
        if (!_paused) TouchOverlay.fromActions(game.report.inputActions, input: _input),
        SafeArea(
          child: Stack(
            fit: StackFit.expand,
            children: [
              const Padding(
                padding: EdgeInsets.all(16),
                child: Align(alignment: Alignment.topLeft, child: Counter()),
              ),
              // The pause button: the `pause` action has no overlay
              // control (Escape, the gamepad's menu button), so a phone
              // pauses here.
              if (!_paused)
                Padding(
                  padding: const EdgeInsets.all(12),
                  child: Align(
                    alignment: Alignment.topRight,
                    child: IconButton.filledTonal(
                      onPressed: () => _setPaused(true),
                      icon: const Icon(Icons.pause),
                      tooltip: 'Pause (Esc)',
                    ),
                  ),
                ),
              if (_showStats)
                Padding(
                  padding: const EdgeInsets.fromLTRB(16, 64, 16, 16),
                  child: Align(
                    alignment: Alignment.topRight,
                    child: StatsBlock(game: game, stats: _stats, viewport: _viewport!),
                  ),
                ),
              // The keyboard's controls; the overlay's buttons say their own.
              if (!TouchOverlay.isTouchPlatform)
                Align(
                  alignment: Alignment.bottomCenter,
                  child: Padding(
                    padding: const EdgeInsets.all(24),
                    child: Hint(game: game),
                  ),
                ),
            ],
          ),
        ),
        const WinFlash(),
        if (_paused)
          _settingsOpen
              ? _settings(game)
              : PauseMenu(
                  game: game,
                  onResume: () => _setPaused(false),
                  onSettings: () => setState(() => _settingsOpen = true),
                ),
      ],
    );
  }
}
