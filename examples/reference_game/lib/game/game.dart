// Fynkel Reference Game: the game behind the app and the tests.
//
// One `World` on the native runtime, the project's startup scene (the
// Playground) loaded into it from the cooked cache (`.fynkel/cache`,
// written by `fynkel cook`; `fynkel run` and `fynkel test` cook first, a
// test calls `cookProject`, and on a device lib/main.dart unpacks the copy
// the app bundles), and every `@FynkelBehaviour` class of this package
// registered so the scene's behaviour components become live Dart objects.
// The host (lib/main.dart, or a test) owns the loop and calls [frame] once
// per frame (ADR-0007/0008).

import 'dart:io';

import 'package:fynkel/fynkel.dart';

import '../generated/behaviours.g.dart';
import 'game_state.dart';

/// The Playground scene (assets/scenes/playground.scene.json).
const playgroundScene = 'bca5eecb-c8d8-58fe-825c-5fce41819bf1';

/// The burst of sparks a coin leaves (assets/prefabs/burst.prefab.json),
/// spawned at runtime by `Collectible`.
const burstPrefab = '88641ff3-d525-5d0b-b21f-226ee8e246a8';

class Game {
  Game._(this.world, this.report);

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

  /// The user's settings service (`world.context.settings`).
  SettingsService get settings => world.context.settings;

  /// The game's observable fields (behaviours write, lib/main.dart binds).
  GameState get state => world.context.service<GameState>();

  /// Scene flow: `scenes.transition(id)` loads at the next frame.
  SceneManager get scenes => world.scenes;

  /// A fresh run: the counters reset and the Playground reloads at the next
  /// frame (every coin back, the player at the start). `Player` calls this
  /// on the `restart` action; the HUD's button too.
  Future<LoadReport> restart() => restartRun(world);

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
/// Playground reloads at the next frame (works from inside a tick).
Future<LoadReport> restartRun(World world) {
  world.context.service<GameState>().reset();
  return world.scenes.transition(playgroundScene);
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
