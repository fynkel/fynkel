// Stage 12 phase 3: particles and the world. Fireworks (sub emitters on
// birth and death), a ball that leaves dust and a trail as it rolls
// (emission over distance, inherit velocity, a TrailRenderer), and sparks
// that bounce off the floor (collision with a plane, its hits told back).
// Each effect is seeded, so it looks the same every time: its golden is in
// test/particles_world_test.dart.

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';
import 'particles_look.dart' show smokePuff, softDot;

/// `--dart-define=LAB_QUALITY=low` (or medium, high) starts the scene at
/// that level, for measuring; the device's own pick otherwise.
const _startQuality = String.fromEnvironment('LAB_QUALITY');

/// The three effects, by name, in the order they stand.
const worldEffects = ['fireworks', 'ball', 'sparks'];

class ParticlesWorldScene extends LabScene {
  /// The whole page; with [only], one effect, close up (for its golden).
  ParticlesWorldScene({this.only}) : assert(only == null || worldEffects.contains(only));

  /// The one effect it shows, or all of them.
  final String? only;

  @override
  String get title => 'Particles and the world';
  @override
  String get step => 'stage 12 phase 3';
  @override
  String get about =>
      'FIREWORKS: shells thrown up, each a SubEmitter on birth that leaves a trail of sparks as it climbs, and one '
      'on death that bursts where it ends, in its colour (SubEmitterInherit.color). '
      'BALL: rolling round, it leaves dust by the metre (Emission.rateOverDistance), thrown back from how it moves '
      '(InheritVelocity), and a TrailRenderer that tapers and fades behind it. '
      'SPARKS: thrown down onto the floor, they bounce off it (ParticleCollision with the floor\'s plane), losing '
      'speed and life at each bounce; onParticleCollision counts the hits. '
      '"show shapes" turns on Debug.drawShapes: where each system makes its particles, and the planes they hit.';

  @override
  Vec3 get eye => switch (only) {
    null => Vec3(0, 2.4, 7),
    final name => _at(name) + _middle(name) + Vec3(0, 1.1, 3.6),
  };
  @override
  Vec3 get target => switch (only) {
    null => Vec3(0, 1.3, 0),
    final name => _at(name) + _middle(name),
  };

  static Vec3 _at(String effect) => Vec3((worldEffects.indexOf(effect) - 1) * 2.8, 0, 0);

  // Where each effect's middle is, from where it stands: the fireworks
  // burst up high, the sparks land to the right.
  static Vec3 _middle(String effect) => switch (effect) {
    'fireworks' => Vec3(0, 1.6, 0),
    'sparks' => Vec3(0.8, 0.6, 0),
    _ => Vec3(0, 0.5, 0),
  };

  final _textures = <Texture2D>[];

  /// Each effect's systems, by its name.
  final systems = <String, List<ParticleSystem>>{};

  /// The ball's trail, and the object that rolls.
  TrailRenderer? trail;
  GameObject? _ball, _rolling;

  /// The sparks' hits on the floor, all told.
  var hits = 0;

  /// The bounce the slider set; the sparks start between 0.4 and 0.6.
  var _bounce = 0.5;

  // The step's times over the last two seconds, and what they came to.
  final _window = <int>[];
  var _shown = (median: 0, slowest: 0);
  var _fps = '–';
  var _hitsShown = 0, _hitsAtWindow = 0;

  Texture2D _keep(Texture2D texture) {
    _textures.add(texture);
    return texture;
  }

  @override
  void build() {
    if (_startQuality.isNotEmpty) world.quality = Quality.levels.firstWhere((q) => q.name == _startQuality);
    // The floor's plane, for the particles to hit: an empty object at
    // its height, facing up.
    final ground = add(GameObject(name: 'ground plane'));
    for (final name in worldEffects) {
      if (only != null && only != name) continue;
      switch (name) {
        case 'fireworks':
          _fireworks(_at(name));
        case 'ball':
          _rollingBall(_at(name));
        case _:
          _sparks(_at(name), ground);
      }
    }
  }

  void _fireworks(Vec3 at) {
    final dot = _keep(softDot());
    final glow = Material(baseColorTexture: dot, blending: Blending.additive);
    // What a shell leaves as it climbs: sparks that hang and fade.
    final trailSparks = ParticleSystem(
      randomSeed: 11,
      startLifetime: const Range(0.3, 0.5),
      startSpeed: const Range(0, 0.3),
      startSize: const Range(0.05, 0.08),
      startColor: const Color(0xFFFFE0A0),
      gravityModifier: 0.2,
      simulationSpace: ParticleSpace.world,
      emission: const Emission(rateOverTime: 50),
      shape: const ParticleShape.sphere(radius: 0.02),
      colorOverLifetime: ColorGradient.fadeOut(),
      material: glow,
    );
    // Its burst where it ends: in its colour, slowing, falling, fading.
    final burst = ParticleSystem(
      randomSeed: 12,
      maxParticles: 2000,
      startLifetime: const Range(0.9, 1.3),
      startSpeed: const Range(2.2, 3),
      startSize: const Range(0.05, 0.08),
      gravityModifier: 0.35,
      simulationSpace: ParticleSpace.world,
      emission: Emission(rateOverTime: 0, bursts: [Burst(count: const Range(90, 110))]),
      shape: const ParticleShape.sphere(radius: 0.05, radiusThickness: 0),
      limitVelocity: const LimitVelocity(limit: ParticleCurve(100), drag: ParticleCurve(1.2)),
      colorOverLifetime: ColorGradient([(0, Color.white), (0.6, Color.white), (1, Color.rgb(1, 1, 1, 0))]),
      renderMode: const ParticleRenderMode.stretched(lengthScale: 1, speedScale: 0.05),
      material: glow,
    );
    // The shells: up, a little apart, a new one every 0.6 s.
    final shells = ParticleSystem(
      randomSeed: 10,
      startLifetime: const Range(0.75, 0.9),
      startSpeed: const Range(5.5, 6.5),
      startSize: const Range(0.12),
      startColor: const ColorRange(Color(0xFFFF4FA0), Color(0xFF40D0FF)),
      gravityModifier: 1,
      simulationSpace: ParticleSpace.world,
      emission: const Emission(rateOverTime: 1.6),
      shape: ParticleShape.cone(angle: 14.deg, radius: 0.05),
      material: glow,
      subEmitters: [
        SubEmitter(trailSparks, SubEmitterType.birth),
        SubEmitter(burst, SubEmitterType.death, inherit: {SubEmitterInherit.color}),
      ],
    );
    systems['fireworks'] = [shells, trailSparks, burst];
    add(
      GameObject(
        name: 'fireworks',
        transform: Transform(position: at + Vec3(0, 0.2, 0)),
        components: [shells],
        children: [
          GameObject(name: 'shell trail', components: [trailSparks]),
          GameObject(name: 'shell burst', components: [burst]),
        ],
      ),
    );
    add(
      shape('launcher', Mesh.cylinder(), const Color(0xFF2A2D33), at: at + Vec3(0, 0.1, 0), size: Vec3(0.3, 0.2, 0.3)),
    );
  }

  void _rollingBall(Vec3 at) {
    // Dust where it touches the floor, a puff each 15 cm, thrown a little
    // back from the way it goes, rising, growing, fading.
    final dust = ParticleSystem(
      randomSeed: 13,
      startLifetime: const Range(0.8, 1.2),
      startSpeed: const Range(0.1, 0.3),
      startSize: const Range(0.15, 0.25),
      startRotation: const Range(0, 6.28),
      startColor: const ColorRange(Color(0xFFB8A58A), Color(0xFFD8C8A8)),
      gravityModifier: -0.02,
      simulationSpace: ParticleSpace.world,
      emission: const Emission(rateOverTime: 0, rateOverDistance: 7),
      shape: const ParticleShape.hemisphere(radius: 0.1),
      inheritVelocity: const InheritVelocity(curve: ParticleCurve(-0.15)),
      sizeOverLifetime: (t) => 1 + 1.5 * t,
      colorOverLifetime: ColorGradient.fadeInOut(Color.white),
      lit: true,
      material: Material(baseColorTexture: _keep(smokePuff()), blending: Blending.transparent, roughness: 1),
    );
    trail = TrailRenderer(
      time: 0.7,
      minVertexDistance: 0.05,
      widthMultiplier: 0.3,
      widthCurve: (t) => 1 - t,
      colorGradient: ColorGradient([
        (0, const Color(0xFFFFF0B0)),
        (0.5, const Color(0xFFFF9040)),
        (1, Color.rgb(1, 0.3, 0.1, 0)),
      ]),
      material: Material(blending: Blending.additive),
    );
    systems['ball'] = [dust];
    _ball = add(
      GameObject(
        name: 'ball',
        transform: Transform(position: at),
        components: [dust],
        children: [
          _rolling = shape(
            'rolling',
            Mesh.sphere(),
            const Color(0xFFE0613A),
            at: Vec3(0, 0.25, 0),
            size: Vec3(0.5, 0.5, 0.5),
            components: [trail!],
          ),
        ],
      ),
    );
    _roll(at, 0);
  }

  /// The ball [seconds] on: round a circle of a metre, once in three
  /// seconds, turning as it rolls.
  void _roll(Vec3 at, double seconds) {
    const radius = 1.0, period = 3.0;
    final a = seconds / period * 2 * math.pi;
    _ball!.transform = Transform(position: at + Vec3(math.cos(a) * radius, 0, math.sin(a) * radius));
    // Rolled round the way it goes: a radius of 0.25 turns 4 times a metre.
    final along = Vec3(-math.sin(a), 0, math.cos(a));
    final axis = Vec3(along.z, 0, -along.x);
    _rolling!.transform = _rolling!.transform.copyWith(rotation: Quat.axisAngle(axis, a * radius / 0.25));
  }

  void _sparks(Vec3 at, GameObject ground) {
    final dot = _keep(softDot());
    // A flash where each hits.
    final flash = ParticleSystem(
      randomSeed: 15,
      startLifetime: const Range(0.15),
      startSpeed: const Range(0),
      startSize: const Range(0.12, 0.18),
      startColor: const Color(0xFFFFD080),
      simulationSpace: ParticleSpace.world,
      emission: Emission(rateOverTime: 0, bursts: [Burst(count: const Range(1))]),
      shape: const ParticleShape.point(),
      colorOverLifetime: ColorGradient.fadeOut(),
      material: Material(baseColorTexture: dot, blending: Blending.additive),
    );
    final sparks = ParticleSystem(
      randomSeed: 14,
      startLifetime: const Range(1.2, 1.8),
      startSpeed: const Range(2.5, 4),
      startSize: const Range(0.04, 0.06),
      startColor: const Color(0xFFFFE8B0),
      gravityModifier: 1,
      simulationSpace: ParticleSpace.world,
      emission: const Emission(rateOverTime: 70),
      // Out to the right and a little down, off the edge of a wheel 1.2 m up.
      shape: ParticleShape.cone(angle: 20.deg, radius: 0.05),
      collision: ParticleCollision(
        planes: [ground],
        bounce: const ParticleCurve.between(0.4, 0.6),
        dampen: const ParticleCurve(0.1),
        lifetimeLoss: const ParticleCurve(0.15),
        minKillSpeed: 0.3,
        sendCollisionMessages: true,
      ),
      subEmitters: [SubEmitter(flash, SubEmitterType.collision, probability: 0.3)],
      onParticleCollision: (plane, count) => hits += count,
      colorOverLifetime: ColorGradient([
        (0, Color.white),
        (0.7, const Color(0xFFFF9030)),
        (1, Color.rgb(1, 0.2, 0, 0)),
      ]),
      renderMode: const ParticleRenderMode.stretched(lengthScale: 1, speedScale: 0.05),
      material: Material(baseColorTexture: dot, blending: Blending.additive),
    );
    systems['sparks'] = [sparks, flash];
    add(
      GameObject(
        name: 'grinder',
        transform: Transform(position: at + Vec3(-0.45, 1.2, 0), rotation: Quat.axisAngle(Vec3.back, -100.deg)),
        components: [sparks],
        children: [
          GameObject(name: 'flash', components: [flash]),
        ],
      ),
    );
    add(
      shape('wheel', Mesh.cylinder(), const Color(0xFF6A7080), at: at + Vec3(-0.65, 1.2, 0), size: Vec3(0.4, 0.06, 0.4))
        ..transform = Transform(
          position: at + Vec3(-0.65, 1.2, 0),
          rotation: Quat.axisAngle(Vec3.right, 90.deg),
          scale: Vec3(0.4, 0.06, 0.4),
        ),
    );
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (_ball != null) _roll(_at('ball'), time.sinceStart);
    _window.add(world.particleStepTime.inMicroseconds);
    if (_window.length == 120) {
      _window.sort();
      _shown = (median: _window[60], slowest: _window.last);
      _window.clear();
      _hitsShown = ((hits - _hitsAtWindow) / 2).round();
      _hitsAtWindow = hits;
      if (printRates) {
        // ignore: avoid_print
        print('[particles] the world: step median ${_ms(_shown.median)}, slowest ${_ms(_shown.slowest)}');
      }
    }
    readout.value = [
      [
        for (final MapEntry(key: name, value: list) in systems.entries)
          '$name ${list.fold(0, (n, s) => n + s.particleCount)}',
        if (trail case final t?) 'trail ${t.positionCount} points',
      ].join(', '),
      if (systems.containsKey('sparks')) 'sparks hitting the floor: $_hitsShown a second',
      'the step, over two seconds: ${_ms(_shown.median)} a frame, at most ${_ms(_shown.slowest)}   $_fps fps',
    ].join('\n');
  }

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('particles', s));

  String _ms(int micros) => '${(micros / 1000).toStringAsFixed(2)} ms';

  @override
  void onDestroy() {
    super.onDestroy();
    // Textures made in code are the scene's to free.
    for (final texture in _textures) {
      texture.dispose();
    }
  }

  @override
  List<Control> get controls => [
    for (final name in systems.keys)
      Toggle(name, () => systems[name]!.first.isEmitting, (on) {
        final first = systems[name]!.first;
        on ? first.play() : first.stop();
        if (name == 'ball') trail?.emitting = on;
      }),
    Toggle('show shapes', () => world.debug.drawShapes, (on) {
      world.debug.drawShapes = on;
      log.add(on ? 'Debug.drawShapes: each system\'s shape, and the planes they hit' : 'shapes hidden');
    }),
    if (systems['sparks'] case final sparks?)
      Slider('bounce', () => _bounce, (v) {
        _bounce = v;
        final c = sparks.first.collision!;
        sparks.first.collision = ParticleCollision(
          planes: c.planes,
          bounce: ParticleCurve(v),
          dampen: c.dampen,
          lifetimeLoss: c.lifetimeLoss,
          minKillSpeed: c.minKillSpeed,
          sendCollisionMessages: true,
        );
      }, max: 1),
    if (trail case final t?) Slider('trail time', () => t.time, (v) => t.time = v, min: 0.1, max: 2),
  ];
}
