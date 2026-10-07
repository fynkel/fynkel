// The engine comparison's scene (docs/engine-comparison.md § Rules): Sponza,
// 20 walking characters, a sun, eight point lights, ambient 2 000 lux and
// a camera on a 30 s path — the same numbers as the Bevy and Unity apps.

import 'dart:math' as math;

import 'package:fynkel/advanced.dart' show NativeLibrary;
import 'package:fynkel/gameplay.dart';

import '../generated/assets.g.dart';

class Game {
  Game._(this.world, this.camera, [this.cubes = const []]);

  final World world;
  final GameObject camera;

  /// The head-to-head scene's cubes (spikes/bevy_flutter): moved from Dart
  /// every frame by [place]; empty for Sponza.
  final List<GameObject> cubes;

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

  /// The Bevy spike's scene (spikes/bevy_flutter/native/src/lib.rs `setup`),
  /// for the head-to-head: an 80 m ground, a shadowed sun, [count] 0.4 m
  /// cubes in eight colours that [place] moves from Dart every frame, and
  /// one character walking at (0, 0, 8) at 1.5× scale. The camera is fixed.
  static Game loadCubes(String projectDir, {String? cacheDir, int count = 1000}) {
    final world = World.create();
    final report = world.loadProject(projectDir, cacheDir: cacheDir);
    if (!report.ok) {
      world.dispose();
      throw StateError('$projectDir did not load:\n${report.diagnostics.join('\n')}');
    }
    late final GameObject camera;
    final cubes = <GameObject>[];
    world.edit(() {
      final ground = world.createMaterial(const PbrMaterial(baseColor: Vec4(0.55, 0.55, 0.5, 1), roughness: 0.9));
      world
          .spawn(name: 'Ground', transform: const Pose(scale: Vec3(8, 1, 8)))
          .set(MeshRenderer(mesh: Primitive.plane.mesh, material: ground));
      final palette = [
        for (var i = 0; i < 8; i++)
          world.createMaterial(PbrMaterial(baseColor: _hsl(i * 45.0, 0.6, 0.55), roughness: 0.6)),
      ];
      for (var i = 0; i < count; i++) {
        cubes.add(
          world
              .spawn(name: 'Cube', transform: const Pose(position: Vec3(0, -10, 0), scale: Vec3(0.4, 0.4, 0.4)))
              ..set(MeshRenderer(mesh: Primitive.cube.mesh, material: palette[i % 8])),
        );
      }
      final c = world.instantiate(
        Assets.models.character.prefab,
        transform: const Pose(position: Vec3(0, 0, 8), scale: Vec3(1.5, 1.5, 1.5)),
      );
      c.set(Animator(clip: Assets.models.character.clips.walk, looping: true, playing: true));
      world
          .spawn(name: 'Sun', transform: Pose(rotation: Quat.lookRotation(const Vec3(-0.3, -1.0, -0.25))))
          .set(const DirectionalLight(color: Vec3(1, 1, 1), intensity: 8000, castShadows: true));
      // Bevy's GlobalAmbientLight 400 cd/m² ≈ 400·π lux.
      world.spawn(name: 'Environment').set(const Environment(ambientColor: Vec3(1, 1, 1), ambientIntensity: 1257));
      camera = world.spawn(
        name: 'Camera',
        transform: Pose(position: const Vec3(0, 9, 22), rotation: Quat.lookRotation(const Vec3(0, -8, -22))),
      )..set(
          // Bevy's defaults: 45° vertical, EV100 9.7.
          const Camera(fovY: math.pi / 4, near: 0.1, far: 200, exposureMode: ExposureMode.manual, exposure: 9.7),
        );
    });
    final game = Game._(world, camera, cubes);
    game.place(0);
    return game;
  }

  /// Sponza: the camera at [t] seconds on its 30 s path. Cubes: the spike's
  /// wave (the same numbers as spikes/bevy_flutter/app/lib/bevy.dart).
  void place(double t) {
    if (cubes.isNotEmpty) {
      world.edit(() {
        for (var i = 0; i < cubes.length; i++) {
          final col = (i % 40).toDouble(), row = (i ~/ 40).toDouble();
          cubes[i].transform.value = Pose(
            position: Vec3((col - 19.5) * 0.9, 0.6 + 0.5 * math.sin(t * 2.0 + i * 0.15), (row - 12.0) * 0.9 - 6.0),
            rotation: Quat.axisAngle(Vec3.up, t + i * 0.01),
            scale: const Vec3(0.4, 0.4, 0.4),
          );
        }
      });
      return;
    }
    final phi = 2 * math.pi * t / 30;
    world.edit(() {
      camera.transform.position = Vec3(10 * math.sin(phi), 2.0, 0);
      camera.transform.rotation = Quat.lookRotation(Vec3(math.cos(phi), -0.05, math.sin(phi)));
    });
  }

  FramePlan frame(double dtSeconds) => world.frame(dtSeconds);

  void dispose() => world.dispose();
}

/// HSL (degrees, 0–1, 0–1) as an opaque colour, like Bevy's `Color::hsl`.
Vec4 _hsl(double h, double s, double l) {
  final c = (1 - (2 * l - 1).abs()) * s;
  final hp = (h % 360) / 60;
  final x = c * (1 - (hp % 2 - 1).abs());
  final (r, g, b) = switch (hp.floor()) {
    0 => (c, x, 0.0),
    1 => (x, c, 0.0),
    2 => (0.0, c, x),
    3 => (0.0, x, c),
    4 => (x, 0.0, c),
    _ => (c, 0.0, x),
  };
  final m = l - c / 2;
  return Vec4(r + m, g + m, b + m, 1);
}
