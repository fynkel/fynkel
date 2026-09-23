// Fynkel Reference Game: the game behind the app and the tests.
//
// One `World` on the native runtime, the project's startup scene (the
// Menu) loaded into it from the cooked cache (`.fynkel/cache`, written by
// `fynkel cook`; `fynkel run` and `fynkel test` cook first, a test calls
// `cookProject`, and on a device lib/main.dart unpacks the copy the app
// bundles), and every `@FynkelBehaviour` class of this package registered
// so the scene's behaviour components become live Dart objects. The host
// (lib/main.dart, or a test) owns the loop and calls [frame] once per frame
// (ADR-0007/0008).
//
// The flow (M3-15): Menu → Playground → Results → Menu or Playground again,
// each a `scenes.transition` performed at the start of the next frame;
// `GameState.screen` follows the loads, so the app's overlay (the title,
// the HUD, the results) switches on it without asking the engine.

import 'dart:io';

import 'package:fynkel/fynkel.dart';

import '../generated/behaviours.g.dart';
import 'game_state.dart';

/// The Menu scene (assets/scenes/menu.scene.json): the project's startup
/// scene, the title's backdrop.
const menuScene = '65905281-dc97-58ac-8d21-f5fbbac251a9';

/// The Playground scene (assets/scenes/playground.scene.json): the level.
const playgroundScene = 'bca5eecb-c8d8-58fe-825c-5fce41819bf1';

/// The Results scene (assets/scenes/results.scene.json): it plays the win
/// jingle from its load.
const resultsScene = '8ac37ea4-2149-55a2-b535-802e54715bea';

/// The menus' button click (assets/audio/click.ogg, Kenney), played by
/// the app on the presentation lane (`world.audio.play(clickSound,
/// presentation: true)`), outside the ticks.
const clickSound = AssetRef('a716d781-e657-4de4-a9b7-aeda36a941fa');

/// Seconds the win banner stays before the Results scene takes over
/// (`Player` counts them in ticks).
const winDelay = 1.5;

/// The burst of sparks a coin leaves (assets/prefabs/burst.prefab.json),
/// spawned at runtime by `Collectible`.
const burstPrefab = '88641ff3-d525-5d0b-b21f-226ee8e246a8';

/// The project's tags (fynkel.project.json `tags`, ADR-0020): the Player
/// carries [playerTag], every coin [pickupTag].
const playerTag = 'player';
const pickupTag = 'pickup';

/// Whether [entity] (a zone event's `other`) is the player: the object
/// tagged [playerTag], as the last tick left the world. The zones' masks
/// already admit only the `player` layer; the tag says which object on it
/// is the one that plays.
bool isPlayer(World world, int entity) => world.findWithTag(playerTag)?.entity == entity;

class Game {
  Game._(this.world, this.report) {
    scenes.addListener(_onScene);
    _onScene(scenes.state);
  }

  final World world;

  /// What `World.loadProject` found: scene, entities, attached behaviours
  /// and any load diagnostics (warnings such as an unregistered type).
  final LoadReport report;

  /// Loads the project at [projectDir] (the directory holding
  /// `fynkel.project.json`) from its cooked cache ([cacheDir], default
  /// `<projectDir>/.fynkel/cache`), and the user's `settings.json` at
  /// [settingsPath] when given (the app passes the platform's
  /// application-support directory; a test leaves it out for a memory
  /// store). Throws when the cache is missing or stale (`FYN-R0001`/`R0002`:
  /// run `fynkel cook`) or the scene does not load; warnings stay in
  /// [report].diagnostics.
  static Game load(String projectDir, {String? cacheDir, String? settingsPath, NativeLibrary? library}) {
    final world = World.create(library: library);
    registerBehaviours(world.registry);
    // The observable game state: behaviours write it, widgets bind to it.
    world.context.install(GameState());
    final warnings = <LoadDiagnostic>[];
    world.onLoadDiagnostics = warnings.addAll;
    final report = world.loadProject(projectDir, cacheDir: cacheDir);
    if (!report.ok) {
      world.dispose();
      throw StateError('$projectDir did not load:\n${report.diagnostics.join('\n')}');
    }
    // The user's settings (volumes, quality, rebinds) read once here and
    // written on every `settings.update`/`edit`; the rebinds sit over the
    // project's actions from now on.
    if (settingsPath != null) world.useSettings(SettingsStore.open(settingsPath, world: world));
    return Game._(world, report);
  }

  /// The screen follows the scene: set as each replace load lands (inside
  /// the frame that performs the transition), read by the overlay.
  void _onScene(SceneState s) {
    if (s != SceneState.ready) return;
    state.screen.value = switch (scenes.current) {
      playgroundScene => Screen.playing,
      resultsScene => Screen.results,
      _ => Screen.menu,
    };
  }

  /// The user's settings service (`world.context.settings`).
  SettingsService get settings => world.context.settings;

  /// The game's observable fields (behaviours write, lib/main.dart binds).
  GameState get state => world.context.service<GameState>();

  /// Scene flow: `scenes.transition(id)` loads at the next frame.
  SceneManager get scenes => world.scenes;

  /// A fresh run: the counters reset and the Playground loads at the next
  /// frame (every coin back, the player at the start). The Menu's Play,
  /// the Results' Play again; `Player` does the same on the `restart`
  /// action.
  Future<LoadReport> play() => restartRun(world);

  /// The run is over: its numbers kept for the Results screen, which
  /// loads at the next frame. `Player` calls this [winDelay] after the
  /// last coin; the pause menu's "Give up" too.
  Future<LoadReport> finish() => finishRun(world);

  /// Back to the title at the next frame (from the Results, or the pause
  /// menu's "Quit to menu").
  Future<LoadReport> toMenu() {
    state.reset();
    return scenes.transition(menuScene);
  }

  /// The bindings in force for [action]: the user's rebind when there is
  /// one (settings.json), the project's otherwise. Reads the settings and
  /// the load report — no engine call.
  List<String> bindingsOf(String action) =>
      settings.current.rebinds[action] ??
      report.inputActions.where((a) => a.name == action).firstOrNull?.bindings ??
      const [];

  /// One host frame of [dtSeconds] with the frame's raw [input] events:
  /// fixed ticks (behaviour `fixedUpdate`, native step, `afterPhysics`),
  /// then `update`/`lateUpdate`.
  FramePlan frame(double dtSeconds, {List<InputEvent> input = const []}) => world.frame(dtSeconds, input: input);

  void dispose() {
    state.dispose();
    world.dispose();
  }
}

/// A fresh run of [world]'s game: the [GameState] counters reset and the
/// Playground loads at the next frame (works from inside a tick).
Future<LoadReport> restartRun(World world) {
  world.context.service<GameState>().reset();
  return world.scenes.transition(playgroundScene);
}

/// The end of [world]'s run: the counters snapshotted into
/// [GameState.lastRun] and the Results scene loaded at the next frame
/// (works from inside a tick).
Future<LoadReport> finishRun(World world) {
  world.context.service<GameState>().finish();
  return world.scenes.transition(resultsScene);
}

/// The project directory this process should load: `FYNKEL_PROJECT`
/// (`--dart-define` or environment variable; `fynkel run` sets it), else the
/// working directory when it holds `fynkel.project.json`.
String? projectDirectory() {
  const defined = String.fromEnvironment('FYNKEL_PROJECT');
  if (defined.isNotEmpty) return defined;
  final env = Platform.environment['FYNKEL_PROJECT'];
  if (env != null && env.isNotEmpty) return env;
  final cwd = Directory.current.path;
  if (File('$cwd/fynkel.project.json').existsSync()) return cwd;
  return null;
}
