// Fynkel Showcase: the game behind the app and the tests.
//
// One `World` on the native runtime, the project's startup scene loaded
// into it from the cooked cache (`.fynkel/cache`, written by `fynkel cook`;
// `fynkel run` and `fynkel test` cook first, a test calls `cookProject`),
// and every `@FynkelBehaviour` class of this package registered so the
// scene's behaviour components become live Dart objects. The host
// (lib/main.dart, or a test) owns the loop and calls [frame] once per frame
// (ADR-0007/0008).

import 'dart:io';

import 'package:fynkel/fynkel.dart';

import '../generated/behaviours.g.dart';

class Game {
  Game._(this.world, this.report);

  final World world;

  /// What `World.loadProject` found: scene, entities, attached behaviours
  /// and any load diagnostics (warnings such as an unregistered type).
  final LoadReport report;

  /// Loads the project at [projectDir] (the directory holding
  /// `fynkel.project.json`) from its cooked cache ([cacheDir], default
  /// `<projectDir>/.fynkel/cache`). Throws when the cache is missing or
  /// stale (`FYN-R0001`/`R0002`: run `fynkel cook`) or the scene does not
  /// load; warnings stay in [report].diagnostics.
  static Game load(String projectDir, {String? cacheDir, NativeLibrary? library}) {
    final world = World.create(library: library);
    registerBehaviours(world.registry);
    final warnings = <LoadDiagnostic>[];
    world.onLoadDiagnostics = warnings.addAll;
    final report = world.loadProject(projectDir, cacheDir: cacheDir);
    if (!report.ok) {
      world.dispose();
      throw StateError('$projectDir did not load:\n${report.diagnostics.join('\n')}');
    }
    return Game._(world, report);
  }

  /// One host frame of [dtSeconds]: fixed ticks (behaviour `fixedUpdate`,
  /// native step, `afterPhysics`), then `update`/`lateUpdate`.
  FramePlan frame(double dtSeconds) => world.frame(dtSeconds);

  void dispose() => world.dispose();
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
