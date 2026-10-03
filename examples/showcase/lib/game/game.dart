import 'package:fynkel/advanced.dart' show NativeLibrary;
// Fynkel Showcase: the game behind the app and the tests.
//
// One `World` on the native runtime, the project's startup scene loaded
// into it from the cooked cache (`.fynkel/cache`, written by `fynkel cook`;
// `fynkel run` and `fynkel test` cook first, a test calls `cookProject`,
// and on a device lib/main.dart unpacks the copy the app bundles),
// and every `@FynkelBehaviour` class of this package registered so the
// scene's behaviour components become live Dart objects. The host
// (lib/main.dart, or a test) owns the loop and calls [frame] once per frame
// (ADR-0007/0008).

import 'package:fynkel/gameplay.dart';

import '../generated/behaviours.g.dart';

class Game {
  Game._(this.world, this.report) : scene = report;

  final World world;

  /// What `World.loadProject` found: scene, entities, attached behaviours
  /// and any load diagnostics (warnings such as an unregistered type).
  final LoadReport report;

  /// What the scene on screen reported: [report] until [loadScene].
  LoadReport scene;

  /// The scenes the app switches between with the number keys: the
  /// startup scene, then one per example shader (ADR-0031).
  static const scenes = {
    'Showcase': '3a56fbbd-b340-41ca-ac2b-b5603d1dc0da',
    'Dissolve': '6cdb53dd-cdd3-5638-a9d6-058f3d61059f',
    'Water': 'e4593a1e-c8ba-5037-af95-02d1142b16cc',
    'Rim Light': '4fa09cf3-8f27-5190-b783-5b8bc8651ec9',
  };

  /// Replaces what is loaded by the scene [id] from the same cache.
  LoadReport loadScene(String id) => scene = world.loadScene(id);

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
