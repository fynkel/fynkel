// Particle systems from Dart (docs/stage-12.md phases 1 to 3): the settings go
// to the engine, the world steps them after the animators, and the
// counts, commands and stop actions come back. The simulation's own
// numbers are the engine's tests (crates/fynkel_world/src/particles/tests.rs).

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

const dt = 1 / 60;

void frames(World world, double seconds) {
  for (var i = 0; i < (seconds / dt).round(); i++) {
    world.update(dt);
  }
}

/// A burst of [count], not looping, each living [lifetime] seconds.
ParticleSystem burst({int count = 30, double lifetime = 1, StopAction stopAction = StopAction.none}) => ParticleSystem(
  looping: false,
  duration: 1,
  startLifetime: Range(lifetime),
  stopAction: stopAction,
  randomSeed: 7,
  emission: Emission(rateOverTime: 0, bursts: [Burst(count: Range(count.toDouble()))]),
);

void main() {
  late World world;
  setUp(() => world = World());
  tearDown(() => world.dispose());

  test('a system plays when it comes, and its particles live their lives', () {
    final system = burst(lifetime: 0.5);
    expect(system.isPlaying, isFalse, reason: 'on no object yet');
    expect(system.particleCount, 0);
    final object = world.add(GameObject(components: [system]));
    expect(object.get<ParticleSystem>(), same(system));
    world.update(dt);
    expect((system.particleCount, system.isPlaying, system.isEmitting), (30, true, true));
    frames(world, 0.6);
    expect(system.particleCount, 0);
    frames(world, 0.5);
    expect((system.isPlaying, system.isStopped), (false, true), reason: 'its second-long timeline is over');
  });

  test('the rate makes so many a second, and a looping system goes on', () {
    final system = ParticleSystem(emission: Emission(rateOverTime: 30), startLifetime: Range(10));
    world.add(GameObject(components: [system]));
    frames(world, 2);
    expect(system.particleCount, inInclusiveRange(59, 61));
    frames(world, 4);
    expect(system.isPlaying, isTrue);
  });

  test('play, stop, pause, clear, emit and simulate', () {
    final system = ParticleSystem(playOnAwake: false, startLifetime: Range(5), emission: Emission(rateOverTime: 0));
    expect(() => system.play(), throwsStateError, reason: 'on no object yet');
    world.add(GameObject(components: [system]));
    frames(world, 0.2);
    expect((system.particleCount, system.isPlaying), (0, false));
    system.emit(4);
    expect(system.particleCount, 4);
    expect(() => system.emit(-1), throwsArgumentError);
    system.play();
    expect(system.isPlaying, isTrue);
    system.pause();
    expect((system.isPaused, system.isPlaying, system.isStopped), (true, false, false));
    system.clear();
    expect(system.particleCount, 0);
    system
      ..stop(clear: true)
      ..emission = Emission(rateOverTime: 0, bursts: [Burst(count: Range(12))])
      ..play()
      ..simulate(0.5);
    expect(system.particleCount, 12);
    system.stop();
    expect((system.isEmitting, system.particleCount), (false, 12), reason: 'they live on');
    system.stop(clear: true);
    expect(system.particleCount, 0);
  });

  test('a burst that destroys its object when done, one that turns it off, and a callback', () {
    final destroyed = world.add(
      GameObject(
        name: 'sparkle',
        components: [burst(stopAction: StopAction.destroy)],
      ),
    );
    final pooled = world.add(GameObject(components: [burst(stopAction: StopAction.disable)]));
    var called = 0;
    final told = burst(stopAction: StopAction.callback)..onStopped = () => called++;
    world.add(GameObject(components: [told]));
    frames(world, 1.5);
    expect(destroyed.isAlive, isFalse);
    expect(pooled.active, isFalse);
    expect(called, 1);
    // The pooled one, turned on again, plays from the start.
    pooled.active = true;
    world.update(dt);
    expect(pooled.get<ParticleSystem>()!.particleCount, 30);
  });

  test('paused with the world and stopped by time.scale 0, unless it uses unscaled time', () {
    final scaled = ParticleSystem(startSpeed: Range(1), startLifetime: Range(10));
    final unscaled = ParticleSystem(startSpeed: Range(1), startLifetime: Range(10), useUnscaledTime: true);
    world
      ..add(GameObject(components: [scaled]))
      ..add(GameObject(components: [unscaled]));
    frames(world, 0.5);
    final (a, b) = (scaled.particleCount, unscaled.particleCount);
    world.paused = true;
    frames(world, 0.5);
    expect(scaled.particleCount, a);
    expect(unscaled.particleCount, greaterThan(b));
    world.paused = false;
    world.time.scale = 0;
    frames(world, 0.5);
    expect(scaled.particleCount, a);
  });

  test('turned off or hidden it clears, and on again it plays from the start', () {
    final system = burst(lifetime: 5);
    final object = world.add(GameObject(components: [system]));
    world.update(dt);
    expect(system.particleCount, 30);
    system.enabled = false;
    expect((system.particleCount, system.isPlaying), (0, false));
    system.enabled = true;
    world.update(dt);
    expect(system.particleCount, 30);
    object.active = false;
    world.update(dt);
    expect(system.particleCount, 0);
    object.active = true;
    world.update(dt);
    expect(system.particleCount, 30);
  });

  test('settings change on a playing system, keep its particles, and wrong ones are refused', () {
    final system = ParticleSystem(startLifetime: Range(10), emission: Emission(rateOverTime: 60));
    world.add(GameObject(components: [system]));
    frames(world, 0.5);
    final n = system.particleCount;
    system
      ..startColor = ColorRange(Color(0xFFFF0000), Color(0xFF0000FF))
      ..startSize = Range(0.1, 0.4)
      ..shape = ParticleShape.sphere(radius: 2)
      ..sizeOverLifetime = ((t) => 1 - t)
      ..colorOverLifetime = ColorGradient.fadeOut();
    expect(system.particleCount, n);
    expect(() => system.maxParticles = 0, throwsArgumentError);
    expect(() => system.duration = double.nan, throwsArgumentError);
    expect(() => system.startLifetime = Range(-1), throwsArgumentError);
    expect(() => system.shape = ParticleShape.cone(angle: 90.deg), throwsArgumentError);
    expect(() => system.sizeOverLifetime = (t) => double.infinity, throwsArgumentError);
    expect(
      () => system.emission = Emission(bursts: List.filled(9, const Burst())),
      throwsA(isA<ArgumentError>().having((e) => e.message, 'message', contains('8 bursts'))),
    );
    expect((system.maxParticles, system.duration, system.startLifetime), (1000, 5, Range(10)), reason: 'kept');
    expect(() => ParticleSystem(maxParticles: 200000), throwsArgumentError);
  });

  test('a system draws with its material, which stays while it does', () {
    final glow = Material(color: Color(0xFFFFAA33), blending: Blending.additive);
    final system = ParticleSystem(material: glow);
    final object = world.add(GameObject(components: [system]));
    world.update(dt);
    expect(system.material, same(glow));
    system.material = null;
    object.removeComponent(system);
    expect(system.isRemoved, isTrue);
    expect(() => system.play(), throwsStateError);
    world.update(dt);
  });

  test('a seed makes the same particles each time', () {
    // Two systems with one seed, one with another: told apart by how many
    // are left after a while, as their lifetimes differ at random.
    int left(int seed) {
      final system = ParticleSystem(
        randomSeed: seed,
        startLifetime: Range(0.5, 1.5),
        looping: false,
        emission: Emission(rateOverTime: 0, bursts: [Burst(count: Range(200))]),
      );
      world.add(GameObject(components: [system]));
      frames(world, 1);
      return system.particleCount;
    }

    final a = left(1);
    expect(left(1), a);
    expect(left(2), isNot(a));
  });

  test('the modules and how it is drawn are sent, and wrong ones are refused', () {
    final system = ParticleSystem(
      startLifetime: Range(2),
      emission: Emission(rateOverTime: 100),
      velocityOverLifetime: ParticleVector(x: ParticleCurve.curve((t) => t, scale: 2), space: ParticleSpace.world),
      limitVelocity: LimitVelocity(limit: ParticleCurve(3), dampen: 0.5, drag: ParticleCurve.between(0.1, 0.3)),
      forceOverLifetime: ParticleVector(y: ParticleCurve(2)),
      rotationOverLifetime: ParticleCurve.curves((t) => 1 - t, (t) => t, scale: 3),
      sizeBySpeed: SizeBySpeed((t) => 1 + t, maxSpeed: 5),
      colorBySpeed: ColorBySpeed(ColorGradient.fadeOut(), minSpeed: 1, maxSpeed: 4),
      noise: ParticleNoise(strength: ParticleCurve(0.5), frequency: 1, scrollSpeed: 0.2, octaves: 3),
      textureSheet: TextureSheet(columns: 4, rows: 2, randomRow: true),
      renderMode: ParticleRenderMode.stretched(lengthScale: 1, speedScale: 0.1),
      lit: true,
      softness: 0.5,
      material: Material(blending: Blending.transparent),
    );
    world.add(GameObject(components: [system]));
    frames(world, 0.5);
    expect(system.particleCount, greaterThan(40));
    final n = system.particleCount;
    system
      ..renderMode = ParticleRenderMode.horizontal
      ..renderMode = ParticleRenderMode.vertical
      ..noise = null
      ..textureSheet = TextureSheet(columns: 2, rows: 2, frameOverTime: ParticleCurve(0.5), cycles: 2);
    expect(system.particleCount, n, reason: 'kept as its settings change');
    expect(() => system.limitVelocity = LimitVelocity(dampen: 2), throwsArgumentError);
    expect(() => system.limitVelocity = LimitVelocity(drag: ParticleCurve(-1)), throwsArgumentError);
    expect(() => system.noise = ParticleNoise(octaves: 5), throwsArgumentError);
    expect(() => system.textureSheet = TextureSheet(columns: 0), throwsArgumentError);
    expect(() => system.rotationOverLifetime = ParticleCurve(double.nan), throwsArgumentError);
    expect(() => system.sizeBySpeed = SizeBySpeed((t) => 1, minSpeed: 2, maxSpeed: 1), throwsArgumentError);
    expect(() => system.softness = -1, throwsArgumentError);
    expect(() => system.renderMode = ParticleRenderMode.stretched(lengthScale: -1), throwsArgumentError);
    expect(system.renderMode, ParticleRenderMode.vertical, reason: 'kept');
    expect(system.noise, isNull);
  });

  test('a mesh its particles are drawn as stays while they are', () {
    final mesh = Mesh.custom(positions: [Vec3.zero, Vec3(1, 0, 0), Vec3(0, 1, 0)], indices: [0, 1, 2]);
    final system = ParticleSystem(renderMode: ParticleRenderMode.mesh(mesh), material: Material());
    world.add(GameObject(components: [system]));
    world.update(dt);
    expect(mesh.dispose, throwsStateError);
    system.renderMode = ParticleRenderMode.billboard;
    mesh.dispose();
    expect(() => system.renderMode = ParticleRenderMode.mesh(mesh), throwsArgumentError);
  });

  test('a gradient blends its stops, before the first and after the last', () {
    final g = ColorGradient([(0.25, Color.rgb(1, 0, 0)), (0.75, Color.rgb(0, 0, 1))]);
    expect(g.at(0), Color.rgb(1, 0, 0));
    expect(g.at(0.5), Color.rgb(0.5, 0, 0.5));
    expect(g.at(1), Color.rgb(0, 0, 1));
    expect(ColorGradient.fadeOut().at(0.5).a, 0.5);
    expect(() => ColorGradient([]), throwsArgumentError);
    expect(() => ColorGradient([(0.5, Color.white), (0.2, Color.white)]), throwsArgumentError);
  });

  group('phase 3: particles and the world', () {
    test('rate over distance makes them as the object moves, none standing still', () {
      final system = ParticleSystem(
        emission: Emission(rateOverTime: 0, rateOverDistance: 10),
        startSpeed: Range(0),
        startLifetime: Range(10),
        simulationSpace: ParticleSpace.world,
      );
      final object = world.add(GameObject(components: [system]));
      frames(world, 0.5);
      expect(system.particleCount, 0);
      for (var i = 1; i <= 30; i++) {
        object.transform = Transform(position: Vec3(i * 0.1, 0, 0));
        world.update(dt);
      }
      expect(system.particleCount, inInclusiveRange(29, 30), reason: 'three metres at ten a metre');
      expect(() => system.emission = Emission(rateOverDistance: -1), throwsArgumentError);
    });

    test('particles bounce off a plane and say so, a plane added after the system', () {
      final hits = <(String, int)>[];
      final system = ParticleSystem(
        looping: false,
        startSpeed: Range(0),
        startSize: Range(0.2),
        startLifetime: Range(5),
        gravityModifier: 1,
        simulationSpace: ParticleSpace.world,
        emission: Emission(rateOverTime: 0, bursts: [Burst(count: Range(5))]),
        shape: ParticleShape.point(),
        onParticleCollision: (plane, count) => hits.add((plane.name!, count)),
      );
      final floor = GameObject(
        name: 'floor',
        transform: Transform(position: Vec3(0, -1, 0)),
      );
      system.collision = ParticleCollision(planes: [floor], bounce: ParticleCurve(0.5), sendCollisionMessages: true);
      world.add(GameObject(components: [system]));
      world.add(floor);
      frames(world, 1);
      expect(hits.first, ('floor', 5), reason: 'all five hit together, once');
      expect(system.particleCount, 5, reason: 'they bounce, nothing lost');
      expect(
        () => system.collision = ParticleCollision(planes: List.filled(7, floor)),
        throwsArgumentError,
        reason: 'at most 6 planes',
      );
    });

    test('a death sub emitter bursts where each dies, its system a child added after', () {
      final sparks = ParticleSystem(
        startLifetime: Range(5),
        startSpeed: Range(1),
        randomSeed: 3,
        emission: Emission(rateOverTime: 100, bursts: [Burst(count: Range(10))]),
      );
      final shell = burst(count: 4, lifetime: 0.5)..subEmitters = [SubEmitter(sparks, SubEmitterType.death)];
      world.add(
        GameObject(
          components: [shell],
          children: [
            GameObject(components: [sparks]),
          ],
        ),
      );
      frames(world, 0.3);
      expect(sparks.particleCount, 0, reason: 'a sub emitter emits only for its parent');
      frames(world, 0.4);
      expect((shell.particleCount, sparks.particleCount), (0, 40));
      expect(() => shell.subEmitters = [SubEmitter(shell, SubEmitterType.birth)], throwsArgumentError);
      expect(() => shell.subEmitters = List.filled(5, SubEmitter(sparks, SubEmitterType.death)), throwsArgumentError);
    });

    test('inherit velocity takes the object\'s speed', () {
      final system = ParticleSystem(
        emission: Emission(rateOverTime: 60),
        startSpeed: Range(0),
        startLifetime: Range(10),
        simulationSpace: ParticleSpace.world,
        shape: ParticleShape.point(),
        inheritVelocity: InheritVelocity(),
      );
      final object = world.add(GameObject(components: [system]));
      for (var i = 1; i <= 30; i++) {
        object.transform = Transform(position: Vec3(i * 0.05, 0, 0));
        world.update(dt);
      }
      expect(system.particleCount, greaterThan(20));
      expect(() => system.inheritVelocity = InheritVelocity(curve: ParticleCurve(double.nan)), throwsArgumentError);
    });

    test('a trail leaves points as its object moves, and clears', () {
      final trail = TrailRenderer(time: 1, minVertexDistance: 0.25, widthCurve: (t) => 1 - t);
      expect(trail.positionCount, 0, reason: 'on no object yet');
      final object = world.add(GameObject(components: [trail]));
      for (var i = 0; i <= 30; i++) {
        object.transform = Transform(position: Vec3(i * 0.1, 0, 0));
        world.update(dt);
      }
      expect(trail.positionCount, inInclusiveRange(10, 11));
      trail.clear();
      expect(trail.positionCount, 0);
      trail.emitting = false;
      for (var i = 0; i <= 10; i++) {
        object.transform = Transform(position: Vec3(3 + i * 0.1, 0, 0));
        world.update(dt);
      }
      expect(trail.positionCount, 0, reason: 'not emitting');
      expect(() => trail.time = -1, throwsArgumentError);
      expect(trail.time, 1);
      trail.enabled = false;
      expect(trail.positionCount, 0);
      object.removeComponent(trail);
      expect(object.get<TrailRenderer>(), isNull);
    });

    test('soft particles are off at low', () {
      expect(
        (Quality.low.softParticles, Quality.medium.softParticles, Quality.high.softParticles),
        (false, true, true),
      );
      expect(Quality.low.copyWith(softParticles: true).softParticles, isTrue);
    });
  });
}
