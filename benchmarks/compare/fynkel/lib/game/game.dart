// The engine comparison's scene (docs/engine-comparison.md § Rules): Sponza,
// 20 walking characters, a sun, eight point lights, ambient 2 000 lux and
// a camera on a 30 s path — the same numbers as the Bevy and Unity apps.

import 'dart:math' as math;

import 'package:fynkel/advanced.dart' show NativeLibrary;
import 'package:fynkel/gameplay.dart';

import '../generated/assets.g.dart';

class Game {
  Game._(this.world, this.camera);

  final World world;
  final GameObject camera;

  static Game load(String projectDir, {String? cacheDir, NativeLibrary? library}) {
    final world = World.create(library: library);
    final report = world.loadProject(projectDir, cacheDir: cacheDir);
    if (!report.ok) {
      world.dispose();
      throw StateError('$projectDir did not load:\n${report.diagnostics.join('\n')}');
    }
    final camera = world.edit(() {
      // instantiate() replaces the prefab root's own transform (Sponza's
      // 0.008 scale) with its argument, so the scale is passed here.
      world.instantiate(Assets.sponza.sponzaGltf.prefab, transform: const Pose(scale: Vec3(0.008, 0.008, 0.008)));
      for (var i = 0; i < 20; i++) {
        final c = world.instantiate(Assets.models.character.prefab);
        c.transform.position = Vec3(-9 + i * 18 / 19, 0, i.isEven ? 1.2 : -1.2);
        c.set(Animator(clip: Assets.models.character.clips.walk, looping: true, playing: true));
      }
      world
          .spawn(
            name: 'Sun',
            transform: Pose(rotation: Quat.lookRotation(const Vec3(-0.3, -1.0, -0.25))),
          )
          .set(const DirectionalLight(color: Vec3(1, 1, 1), intensity: 10000, castShadows: true));
      // 2 000 lumen each: 2000 / 4π ≈ 159 candela.
      for (final x in const [-10.5, -3.5, 3.5, 10.5]) {
        for (final z in const [-3.0, 3.0]) {
          world
              .spawn(
                name: 'Lamp',
                transform: Pose(position: Vec3(x, 1.5, z)),
              )
              .set(PointLight(color: const Vec3(1.0, 0.8, 0.6), intensity: 2000 / (4 * math.pi), range: 6));
        }
      }
      world.spawn(name: 'Environment').set(const Environment(ambientColor: Vec3(1, 1, 1), ambientIntensity: 2000));
      // AO and bloom settings; the quality profile decides whether they run.
      world.spawn(name: 'Look').set(Volume(profile: Assets.volumes.look, shape: VolumeShape.global));
      final cam = world.spawn(name: 'Camera');
      cam.set(
        const Camera(fovY: 60 * math.pi / 180, near: 0.1, far: 100, exposureMode: ExposureMode.manual, exposure: 12),
      );
      return cam;
    });
    final game = Game._(world, camera);
    game.place(0);
    return game;
  }

  /// The camera at [t] seconds on its 30 s path.
  void place(double t) {
    final phi = 2 * math.pi * t / 30;
    world.edit(() {
      camera.transform.position = Vec3(10 * math.sin(phi), 2.0, 0);
      camera.transform.rotation = Quat.lookRotation(Vec3(math.cos(phi), -0.05, math.sin(phi)));
    });
  }

  FramePlan frame(double dtSeconds) => world.frame(dtSeconds);

  void dispose() => world.dispose();
}
