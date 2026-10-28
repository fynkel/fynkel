// Sheepdog's field for the comparison with Unity (stage 14, phase 4):
// the same scene is built in tools/unity_field/Assets/Field.cs from the
// same numbers, so a frame rate on a device says how the two engines
// draw it, not how two games differ. Sheepdog's own shaders are left out
// (Unity would need its own of them); everything here is each engine's
// standard surface.
//
// 60 sheep walking in rings and the dog running round them (skinned,
// each on its clip), 120 fence posts, 8 lamps on poles with glowing
// bulbs, a sun with shadows, 500 lit dust particles, linear fog, from a
// camera 14 m up. Each quality in [fieldLevels] is shown for 20 s, the
// first 5 to settle, and the frames counted over the rest.
//
// With `--define FIELD_GPU=true` each part of a frame is also timed on the
// GPU (WorldView.timeGpu), and reported a frame's worth a level. Off by
// default: timing costs the GPU a little, so the frame rates are read
// without it. `--define FIELD_PARTS=true` shows [partLevels] instead:
// medium with one thing at a time taken away, to find what its GPU time
// goes on.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_measure/fynkel_measure.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart' show FynkelViewStats;

/// Medium, and medium with one thing at a time taken away.
final partLevels = [
  Quality.medium,
  Quality.medium.copyWith(name: 'medium, scale 0.75', renderScale: 0.75),
  Quality.medium.copyWith(name: 'medium, no AA', antiAliasing: 0),
  Quality.medium.copyWith(name: 'medium, no HDR', hdr: false, bloom: false),
  Quality.medium.copyWith(name: 'medium, no lamp highlights', lampHighlights: false),
  Quality.medium.copyWith(name: 'medium, hard shadows', shadowSoftness: ShadowSoftness.hard),
  Quality.medium.copyWith(name: 'medium, no soft particles', softParticles: false),
];

/// The levels compared: ours, and low with less anti-aliasing.
final fieldLevels = [
  Quality.low,
  Quality.low.copyWith(name: 'low, 2x AA', antiAliasing: 2),
  Quality.low.copyWith(name: 'low, no AA', antiAliasing: 0),
  Quality.medium,
  Quality.high,
];

const _timeGpu = bool.fromEnvironment('FIELD_GPU');
const _parts = bool.fromEnvironment('FIELD_PARTS');
const _sky = Color(0xFF9DB8CF);
const _sheep = 60;
const _posts = 120;
const _lamps = 8;

class Field extends GameObject {
  Field({this.seconds = 20}) : super(name: 'field');

  final int seconds;
  final _walkers = <(GameObject, double radius, double speed, double angle)>[];
  var _t = 0.0;

  var _counting = false, _shown = 0, _window = 0.0, _drawn = 0;
  var _busy = Duration.zero;
  var _gpu = const GpuTimes();
  final _update = <int>[];

  @override
  Future<void> onLoad() async {
    super.onLoad();
    final camera = Camera(
      fovY: 50.deg,
      clearColor: _sky,
      bloom: const Bloom(),
      ambientOcclusion: const AmbientOcclusion(),
    );
    add(
      GameObject(
        name: 'camera',
        transform: Transform(position: Vec3(0, 14, 24), rotation: Quat.lookRotation(Vec3(0, -14, -24))),
        components: [camera],
      ),
    );
    world.fog = const Fog(color: _sky, start: 18, end: 60);
    add(
      GameObject(
        name: 'sun',
        components: [Light.sun(direction: Vec3(-0.4, -1, -0.6), shadows: true)],
      ),
    );
    add(GameObject(name: 'ambient', components: [Light.ambient(intensity: 0.3)]));
    add(
      GameObject(
        name: 'ground',
        transform: Transform(scale: Vec3(80, 1, 80)),
        components: [MeshRenderer(Mesh.plane(), Material(color: const Color(0xFF6E8B4A), roughness: 1))],
      ),
    );
    final wood = Material(color: const Color(0xFF8B6B4A), roughness: 0.8);
    for (var i = 0; i < _posts; i++) {
      final a = i / _posts * math.pi * 2;
      add(
        GameObject(
          transform: Transform(position: Vec3(math.cos(a) * 26, 0.6, math.sin(a) * 26), scale: Vec3(0.15, 1.2, 0.15)),
          components: [MeshRenderer(Mesh.cube(), wood)],
        ),
      );
    }
    final glass = Material(color: const Color(0xFFFFC880), emissive: const Color(0xFFFFC880), emissiveIntensity: 2);
    final pole = Material(color: const Color(0xFF333333));
    for (var i = 0; i < _lamps; i++) {
      final a = (i + 0.5) / _lamps * math.pi * 2;
      final x = math.cos(a) * 18, z = math.sin(a) * 18;
      add(
        GameObject(
          transform: Transform(position: Vec3(x, 1.25, z), scale: Vec3(0.1, 2.5, 0.1)),
          components: [MeshRenderer(Mesh.cube(), pole)],
        ),
      );
      add(
        GameObject(
          transform: Transform(position: Vec3(x, 2.6, z), scale: Vec3(0.3, 0.3, 0.3)),
          components: [
            MeshRenderer(Mesh.sphere(), glass),
            Light.point(color: const Color(0xFFFFC880), intensity: 4, range: 10),
          ],
        ),
      );
    }
    final dust = await assets.texture('particles/smoke_04.png');
    add(
      GameObject(
        name: 'dust',
        components: [
          ParticleSystem(
            maxParticles: 500,
            startLifetime: const Range(2, 2),
            startSpeed: const Range(0.2, 0.5),
            startSize: const Range(0.5, 0.8),
            startColor: const ColorRange(Color(0xFFC8B48E), Color(0xFFC8B48E)),
            emission: const Emission(rateOverTime: 250),
            shape: const ParticleShape.box(size: Vec3(40, 0.2, 40)),
            prewarm: true,
            lit: true,
            softness: 0.3,
            material: Material(baseColorTexture: dust, blending: Blending.transparent, roughness: 1),
          ),
        ],
      ),
    );
    for (var i = 0; i < _sheep + 1; i++) {
      final dog = i == _sheep;
      // As Sheepdog has them: a game's size, turned to face the way it goes.
      final walker = add(GameObject(name: dog ? 'dog' : 'sheep'));
      final model = walker.add(await assets.instantiate(dog ? 'dog.glb' : 'sheep.glb'))
        ..transform = Transform(rotation: Quat.axisAngle(Vec3.up, math.pi), scale: Vec3.all(dog ? 0.28 : 0.2));
      final radius = dog ? 14.0 : 4 + (i % 6) * 2.5;
      final speed = dog ? 4.0 : 1.2;
      _walkers.add((walker, radius, speed / radius, i * 2.399963));
      final animator = model.animator!..play(dog ? 'Run' : 'Walk');
      animator.time = (i * 0.37) % animator.length;
    }
    _place();
  }

  void _place() {
    for (final (o, radius, w, start) in _walkers) {
      final a = start + _t * w;
      o.position = Vec3(math.cos(a) * radius, 0, math.sin(a) * radius);
      // Facing along its ring, the way it goes.
      o.rotation = Quat.lookRotation(Vec3(-math.sin(a), 0, math.cos(a)));
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _t += time.delta;
    _place();
    if (_counting) _update.add(world.profile.total.inMicroseconds);
  }

  void onStats(FynkelViewStats stats) {
    if (!_counting) return;
    _shown += stats.view.presented > 0 ? stats.view.presented : stats.view.rendered;
    _window += stats.window.inMicroseconds / 1e6;
    _drawn += stats.view.rendered;
    _busy += stats.view.busy;
    _gpu += stats.view.gpu;
  }

  @override
  void onStart() {
    super.onStart();
    _run();
  }

  Future<void> _run() async {
    await loaded;
    WorldView.timeGpu = _timeGpu;
    for (final level in _parts ? partLevels : fieldLevels) {
      if (isDestroyed) return;
      world.quality = level;
      await world.shadersReady();
      await Future<void>.delayed(const Duration(seconds: 5));
      _shown = _drawn = 0;
      _window = 0;
      _busy = Duration.zero;
      _gpu = const GpuTimes();
      _update.clear();
      _counting = true;
      await Future<void>.delayed(Duration(seconds: seconds - 5));
      _counting = false;
      if (isDestroyed) return;
      final fps = _window > 0 ? _shown / _window : 0;
      final render = _drawn > 0 ? _busy.inMicroseconds / _drawn / 1000 : 0;
      final update = _update.isEmpty ? 0 : (_update..sort())[_update.length ~/ 2] / 1000;
      Measurement.report(
        'FIELD fynkel ${level.name}: ${fps.toStringAsFixed(1)} fps, world.update ${update.toStringAsFixed(2)} ms, '
        'render thread ${render.toStringAsFixed(2)} ms a frame',
      );
      if (_gpu.frames > 0) {
        String ms(Duration d) => (d.inMicroseconds / _gpu.frames / 1000).toStringAsFixed(2);
        Measurement.report(
          'FIELD fynkel ${level.name} gpu over ${_gpu.frames} frames: total ${ms(_gpu.total)}, '
          'shadows ${ms(_gpu.shadows)}, ambient occlusion ${ms(_gpu.ambientOcclusion)}, scene ${ms(_gpu.scene)}, '
          'bloom ${ms(_gpu.bloom)}, finish ${ms(_gpu.finish)}, effects ${ms(_gpu.effects)}, '
          'sky light ${ms(_gpu.skyLight)} ms a frame; ${_gpu.untimed} passes untimed',
        );
      }
    }
    Measurement.report('FIELD done');
  }
}
