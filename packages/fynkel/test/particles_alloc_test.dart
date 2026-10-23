// A frame where particle systems play and burst allocates nothing in
// Dart: the settings went to the engine when they were set, and a frame
// is one call with its times in native memory (docs/stage-12.md phases 1
// to 3: half the systems with every module on; sparks bouncing off a
// floor, told each frame, with sub emitters; a trail going round).
// Counted by the VM service's allocation profile (see allocations.dart).

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';

void main() {
  final madeEachFrame = allocations();

  test('a frame where 20 systems play and burst, with their modules, allocates nothing', () async {
    final world = World();
    addTearDown(world.dispose);
    final glow = Material(color: Color(0xFFFFAA33), blending: Blending.additive);
    for (var i = 0; i < 20; i++) {
      world.add(
        GameObject(
          transform: Transform(position: Vec3(i.toDouble(), 0, 0)),
          components: [
            ParticleSystem(
              duration: 0.5,
              startLifetime: Range(0.4, 0.8),
              startColor: ColorRange(Color(0xFFFFFFFF), Color(0xFFFF8800)),
              gravityModifier: 0.5,
              simulationSpace: i.isEven ? ParticleSpace.world : ParticleSpace.local,
              emission: Emission(rateOverTime: 50, bursts: [Burst(count: Range(10, 20))]),
              sizeOverLifetime: (t) => 1 - t,
              colorOverLifetime: ColorGradient.fadeOut(),
              material: glow,
              velocityOverLifetime: i.isOdd ? ParticleVector(x: ParticleCurve.curve((t) => t)) : null,
              limitVelocity: i.isOdd ? LimitVelocity(limit: ParticleCurve(4), drag: ParticleCurve(0.5)) : null,
              forceOverLifetime: i.isOdd ? ParticleVector(y: ParticleCurve.between(1, 2)) : null,
              rotationOverLifetime: i.isOdd ? ParticleCurve(1) : null,
              noise: i.isOdd ? ParticleNoise(octaves: 2) : null,
              textureSheet: i.isOdd ? TextureSheet(columns: 4, rows: 4) : null,
              renderMode: i.isOdd ? ParticleRenderMode.stretched() : ParticleRenderMode.billboard,
              lit: i.isOdd,
            ),
          ],
        ),
      );
    }

    // Sparks thrown up that fall on a floor, every hit a call, and burst
    // as they die; and a trail going round.
    final floor = world.add(
      GameObject(
        name: 'floor',
        transform: Transform(position: Vec3(0, -1, 0)),
      ),
    );
    var hits = 0;
    final burst = ParticleSystem(
      startLifetime: Range(0.2),
      startSpeed: Range(1),
      emission: Emission(rateOverTime: 0, bursts: [Burst(count: Range(3))]),
      material: glow,
    );
    final ball = world.add(
      GameObject(
        components: [
          ParticleSystem(
            startLifetime: Range(1, 2),
            startSpeed: Range(3, 5),
            gravityModifier: 1,
            simulationSpace: ParticleSpace.world,
            emission: Emission(rateOverTime: 100, rateOverDistance: 5),
            inheritVelocity: InheritVelocity(),
            collision: ParticleCollision(planes: [floor], bounce: ParticleCurve(0.6), sendCollisionMessages: true),
            subEmitters: [
              SubEmitter(burst, SubEmitterType.death, inherit: {SubEmitterInherit.color}),
            ],
            onParticleCollision: (plane, count) => hits += count,
            material: glow,
          ),
          TrailRenderer(time: 0.5, widthCurve: (t) => 1 - t, colorGradient: ColorGradient.fadeOut(), material: glow),
        ],
        children: [
          GameObject(components: [burst]),
        ],
      ),
    );
    world.add(Spinner(ball));

    expect(await madeEachFrame(world, 2000), isEmpty);
    expect(hits, greaterThan(0), reason: 'the collisions were told');
  });
}

/// Moves [ball] round a circle, a step each frame, through transforms
/// made once: what a game makes moving its objects isn't counted here.
class Spinner extends GameObject {
  Spinner(this.ball);

  final GameObject ball;
  final _ring = [
    for (var i = 0; i < 120; i++)
      Transform(position: Vec3(math.cos(i * math.pi / 60) * 2, 0, math.sin(i * math.pi / 60) * 2)),
  ];
  var _step = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    ball.transform = _ring[_step++ % _ring.length];
  }
}
