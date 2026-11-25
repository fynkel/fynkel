// The scene on fynkel.com's front page: a knight on a stone floor, built up
// by the steps the page shows beside it, one every few seconds. Filmed by
// test/film_test.dart.

import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// When each step comes, in seconds, and how long it all lasts. The page
/// (website/src/components/Film.astro) has the same times.
const steps = [0.0, 2.0, 4.5, 6.5, 10.0, 13.0];
const length = 17.0;

const _blocks = [
  Color(0xFFFF5A36),
  Color(0xFFFFB81F),
  Color(0xFF2F6BFF),
  Color(0xFF18B3A4),
  Color(0xFFF2EDE2),
  Color(0xFFE83F8C),
];

class Showcase extends GameObject {
  late final GameObject knight, eye, sun;
  late final Camera camera;
  late final Light light, ambient, moon;
  final _random = math.Random(7);

  /// Seconds since it started.
  var at = 0.0;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    world.quality = Quality.high.copyWith(shadowDistance: 24, shadowResolution: 4096);
    world.sky = null;
    camera = Camera(
      clearColor: const Color(0xFF1A1E2E),
      fovY: 34.deg,
      bloom: const Bloom(strength: 0.6, threshold: 1.3),
      ambientOcclusion: const AmbientOcclusion(intensity: 1.4, radius: 0.4),
    );
    eye = add(GameObject(name: 'eye', components: [camera]));
    ambient = Light.ambient(intensity: 0.9);
    add(GameObject(components: [ambient]));
    light = Light.sun(direction: Vec3(-0.75, -0.55, -0.35), color: const Color(0xFFFFD2A0), intensity: 0);
    sun = add(GameObject(name: 'sun', components: [light]));
    moon = Light.spot(
      color: const Color(0xFFA9C4FF),
      intensity: 0,
      range: 24,
      angle: 40.deg,
      softness: 0.8,
      shadows: true,
    );
    final high = Vec3(-4, 7, 6.5);
    add(
      GameObject(
        name: 'moon',
        transform: Transform(position: high, rotation: Quat.lookRotation(Vec3(0, 0.8, 0) - high)),
        components: [moon],
      ),
    );
    _floor();
    knight = add(await world.assets.instantiate('kaykit/knight.glb'));
    for (final extra in [
      '1H_Sword',
      '1H_Sword_Offhand',
      'Badge_Shield',
      'Rectangle_Shield',
      'Spike_Shield',
      'Round_Shield',
    ]) {
      knight.find(extra)?.active = false;
    }
    knight
      ..addComponent(CapsuleCollider(radius: 0.5, height: 2.4, center: Vec3(0, 1.2, 0)))
      ..rotation = Quat.axisAngle(Vec3.up, -0.25);
    look();
    startRoutine(play);
  }

  /// The steps, each a moment after the last.
  Future<void> play() async {
    await until(steps[1]);
    tween(0.0, 1.0, seconds: 1.4, ease: Ease.inOutSine, onUpdate: dusk);
    await until(steps[2]);
    knight.animator!.crossFade('Cheer', seconds: 0.25, loop: false);
    knight.animator!.onEnd = (_) => knight.animator!.crossFade('2H_Melee_Idle', seconds: 0.3);
    await until(steps[3]);
    for (var i = 0; i < 72; i++) {
      _block();
      await until(steps[3] + i * 0.03);
    }
    await until(steps[4]);
    tween(0.0, 1.0, seconds: 1.2, ease: Ease.inOutSine, onUpdate: _night);
    for (final (i, x) in [(0, -3.6), (1, 3.9), (2, -1.6), (3, 2.0)]) {
      torch(Vec3(x, 0, i < 2 ? -0.8 : -3.3));
      await wait(0.3);
    }
    await until(steps[5]);
    knight.animator!.crossFade('2H_Melee_Attack_Spin', seconds: 0.2, loop: false);
    await wait(0.45);
    time.scale = 0.3;
    _burst();
    await wait(0.55);
    tween(0.3, 1.0, seconds: 0.5, onUpdate: (v) => time.scale = v);
  }

  Future<void> until(double when) => wait(math.max(0.001, when - at));

  void _floor() {
    final stone = Material(color: const Color(0xFF8C8577), roughness: 0.85);
    final dark = Material(color: const Color(0xFF6C675E), roughness: 0.9);
    add(
      GameObject(
        name: 'ground',
        transform: Transform(position: Vec3(0, -0.55, 0), scale: Vec3(300, 1, 300)),
        components: [
          MeshRenderer(Mesh.cube(), Material(color: const Color(0xFF4A4B47), roughness: 1)),
          BoxCollider(),
        ],
      ),
    );
    // Rings of slabs, none quite level.
    for (var ring = 0; ring < 5; ring++) {
      final n = ring == 0 ? 1 : ring * 7;
      for (var i = 0; i < n; i++) {
        final a = (i + (ring.isOdd ? 0.5 : 0)) / n * 2 * math.pi, r = ring * 1.15;
        final high = 0.12 + _random.nextDouble() * 0.05;
        add(
          GameObject(
            name: 'slab',
            transform: Transform(
              position: Vec3(r * math.cos(a), -0.05, r * math.sin(a)),
              rotation: Quat.axisAngle(Vec3.up, -a),
              scale: ring == 0 ? Vec3(1.1, high, 1.1) : Vec3(1.1, high, 2 * math.pi * r / n + 0.02),
            ),
            components: [MeshRenderer(Mesh.cube(), _random.nextInt(3) == 0 ? dark : stone), BoxCollider()],
          ),
        );
      }
    }
    // Broken pillars behind.
    for (final (x, z, h) in [
      (-5.2, -4.0, 3.6),
      (-2.6, -5.6, 1.5),
      (0.6, -6.2, 4.4),
      (3.6, -5.3, 2.2),
      (5.8, -3.2, 3.9),
    ]) {
      add(
        GameObject(
          name: 'pillar',
          transform: Transform(position: Vec3(x, h / 2, z), scale: Vec3(0.9, h, 0.9)),
          components: [MeshRenderer(Mesh.cylinder(), stone), BoxCollider()],
        ),
      );
      add(
        GameObject(
          name: 'foot',
          transform: Transform(position: Vec3(x, 0.15, z), scale: Vec3(1.4, 0.4, 1.4)),
          components: [MeshRenderer(Mesh.cube(), dark), BoxCollider()],
        ),
      );
    }
  }

  /// The sun up, [t] of the way: a low one, under an evening sky.
  void dusk(double t) {
    light.intensity = 3.2 * t;
    ambient.intensity = 0.9 * (1 - t);
    _sky(const Color(0xFF1A1E2E), const Color(0xFF1A1E2E), const Color(0xFF1F5FC4), const Color(0xFFF0A987), t);
  }

  /// The sun down again, for the torches.
  void _night(double t) {
    light.intensity = 3.2 - 2.9 * t;
    moon.intensity = 130 * t;
    _sky(const Color(0xFF1F5FC4), const Color(0xFFF0A987), const Color(0xFF0B1030), const Color(0xFF4A2E5E), t);
  }

  void _sky(Color zenith, Color horizon, Color zenith2, Color horizon2, double t) {
    final low = Color.lerp(horizon, horizon2, t);
    world
      ..sky = Sky.gradient(zenith: Color.lerp(zenith, zenith2, t), horizon: low, ground: const Color(0xFF3A3630))
      ..fog = Fog(start: 13, end: 34, color: low);
  }

  void _block() {
    final size = 0.28 + _random.nextDouble() * 0.34;
    final a = _random.nextDouble() * 2 * math.pi, r = 1.35 + _random.nextDouble() * 2.1;
    final shape = _random.nextInt(3);
    add(
      GameObject(
        name: 'block',
        transform: Transform(
          position: Vec3(r * math.cos(a), 5 + _random.nextDouble() * 2, r * math.sin(a) - 0.3),
          rotation: Quat.axisAngle(Vec3(1, 2, 3).normalized, _random.nextDouble() * 6),
          scale: Vec3.all(size),
        ),
        components: [
          MeshRenderer(
            shape == 0 ? Mesh.sphere() : Mesh.cube(),
            Material(color: _blocks[_random.nextInt(_blocks.length)], roughness: shape == 0 ? 0.25 : 0.5, metallic: 0),
          ),
          shape == 0 ? SphereCollider() : BoxCollider(),
          Rigidbody(mass: size, interpolation: RigidbodyInterpolation.interpolate),
        ],
      ),
    );
  }

  late final _dot = _alpha(64, (d) => math.pow(math.max(0, 1 - d), 2).toDouble());

  void torch(Vec3 at) {
    const warm = Color(0xFFFF9A3C);
    add(
      GameObject(
        name: 'post',
        transform: Transform(position: at + Vec3(0, 0.8, 0), scale: Vec3(0.12, 1.6, 0.12)),
        components: [MeshRenderer(Mesh.cylinder(), Material(color: const Color(0xFF241C18), roughness: 0.8))],
      ),
    );
    add(
      GameObject(
        name: 'bowl',
        transform: Transform(position: at + Vec3(0, 1.62, 0), scale: Vec3(0.42, 0.16, 0.42)),
        components: [MeshRenderer(Mesh.cylinder(), Material(color: const Color(0xFF241C18), roughness: 0.6))],
      ),
    );
    final lamp = Light.point(color: warm, intensity: 0, range: 8);
    add(
      GameObject(
        name: 'flame',
        transform: Transform(position: at + Vec3(0, 1.8, 0)),
        components: [
          lamp,
          ParticleSystem(
            randomSeed: at.x.toInt() + 9,
            startLifetime: const Range(0.5, 0.9),
            startSpeed: const Range(0.3, 0.7),
            startSize: const Range(0.35, 0.6),
            emission: const Emission(rateOverTime: 55),
            shape: ParticleShape.cone(angle: 8.deg, radius: 0.12),
            forceOverLifetime: const ParticleVector(y: ParticleCurve(2.4)),
            noise: const ParticleNoise(strength: ParticleCurve(0.4), frequency: 1.6, scrollSpeed: 1),
            sizeOverLifetime: (t) => t < 0.2 ? 0.5 + 2.5 * t : 1 - 0.85 * (t - 0.2) / 0.8,
            colorOverLifetime: ColorGradient([
              (0, Color.rgb(1, 0.9, 0.6, 0)),
              (0.15, Color.rgb(1, 0.72, 0.3, 0.6)),
              (0.5, Color.rgb(0.9, 0.42, 0.08, 0.45)),
              (1, Color.rgb(0.5, 0.12, 0, 0)),
            ]),
            softness: 0.2,
            material: Material(baseColorTexture: _dot, blending: Blending.additive),
          ),
        ],
      ),
    );
    add(
      GameObject(
        name: 'embers',
        transform: Transform(position: at + Vec3(0, 1.9, 0)),
        components: [
          ParticleSystem(
            randomSeed: at.x.toInt() + 30,
            startLifetime: const Range(1.5, 2.6),
            startSpeed: const Range(0.4, 1),
            startSize: const Range(0.03, 0.06),
            simulationSpace: ParticleSpace.world,
            emission: const Emission(rateOverTime: 14),
            shape: ParticleShape.cone(angle: 20.deg, radius: 0.15),
            forceOverLifetime: const ParticleVector(y: ParticleCurve(0.5)),
            noise: const ParticleNoise(strength: ParticleCurve(0.7), frequency: 0.8, scrollSpeed: 0.4),
            colorOverLifetime: ColorGradient([(0, Color.rgb(3, 1.6, 0.5)), (1, Color.rgb(1, 0.1, 0, 0))]),
            material: Material(baseColorTexture: _dot, blending: Blending.additive),
          ),
        ],
      ),
    );
    tween(0.0, 14.0, seconds: 0.5, onUpdate: (v) => lamp.intensity = v);
  }

  /// The spin's blow: every block thrown out, and sparks.
  void _burst() {
    final at = knight.position + Vec3(0, 0.1, 0);
    for (final body in world.all<Rigidbody>()) {
      body.addExplosionForce(9, at, 5, upwardsModifier: 0.9, mode: ForceMode.velocityChange);
    }
    final flash = Light.point(color: const Color(0xFF8FB4FF), intensity: 0, range: 9);
    add(
      GameObject(
        name: 'burst',
        transform: Transform(position: at + Vec3(0, 0.8, 0)),
        components: [
          flash,
          ParticleSystem(
            randomSeed: 5,
            looping: false,
            duration: 1,
            startLifetime: const Range(0.5, 1.1),
            startSpeed: const Range(4, 11),
            startSize: const Range(0.04, 0.09),
            emission: Emission(rateOverTime: 0, bursts: [Burst(count: const Range(240))]),
            shape: const ParticleShape.sphere(radius: 0.5),
            gravityModifier: 0.4,
            limitVelocity: const LimitVelocity(limit: ParticleCurve(100), drag: ParticleCurve(2)),
            colorOverLifetime: ColorGradient([
              (0, Color.rgb(2, 3, 5)),
              (0.4, Color.rgb(0.6, 1.2, 4)),
              (1, Color.rgb(0.1, 0.2, 1, 0)),
            ]),
            renderMode: const ParticleRenderMode.stretched(lengthScale: 1, speedScale: 0.06),
            material: Material(baseColorTexture: _dot, blending: Blending.additive),
          ),
        ],
      ),
    );
    tween(45.0, 0.0, seconds: 0.6, onUpdate: (v) => flash.intensity = v);
  }

  static Texture2D _alpha(int size, double Function(double d) alpha) {
    final pixels = Uint8List(size * size * 4);
    for (var y = 0; y < size; y++) {
      for (var x = 0; x < size; x++) {
        final dx = (x + 0.5) / size - 0.5, dy = (y + 0.5) / size - 0.5;
        final i = (y * size + x) * 4;
        pixels
          ..[i] = 255
          ..[i + 1] = 255
          ..[i + 2] = 255
          ..[i + 3] = (alpha(math.sqrt(dx * dx + dy * dy) * 2).clamp(0.0, 1.0) * 255).round();
      }
    }
    return Texture2D.rgba(size, size, pixels);
  }

  /// The camera, a slow arc round the knight, who stands right of the
  /// middle so the code has the left.
  void look() {
    final t = at / length;
    final a = 0.42 - 0.55 * t, far = 11.5 - 1.2 * math.sin(t * math.pi);
    final from = Vec3(far * math.sin(a), 2.9 + 0.5 * t, far * math.cos(a));
    final forward = (Vec3(0, 1.25, 0) - from).normalized;
    final right = forward.cross(Vec3.up).normalized;
    eye
      ..position = from - right * 2.0
      ..rotation = Quat.lookRotation(forward);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    at += time.unscaledDelta;
    look();
  }
}
