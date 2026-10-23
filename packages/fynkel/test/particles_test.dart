// Particle systems from Dart (docs/stage-12.md phase 1): the settings go
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

  test('a gradient blends its stops, before the first and after the last', () {
    final g = ColorGradient([(0.25, Color.rgb(1, 0, 0)), (0.75, Color.rgb(0, 0, 1))]);
    expect(g.at(0), Color.rgb(1, 0, 0));
    expect(g.at(0.5), Color.rgb(0.5, 0, 0.5));
    expect(g.at(1), Color.rgb(0, 0, 1));
    expect(ColorGradient.fadeOut().at(0.5).a, 0.5);
    expect(() => ColorGradient([]), throwsArgumentError);
    expect(() => ColorGradient([(0.5, Color.white), (0.2, Color.white)]), throwsArgumentError);
  });
}
