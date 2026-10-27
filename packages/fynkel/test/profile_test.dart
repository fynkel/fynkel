// world.profile (stage 14's GT7): the last frame's update, part by part,
// and each system's part, as Unity's Profiler shows them.

import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

/// Keeps the thread busy for [ms] milliseconds.
void busy(double ms) {
  final watch = Stopwatch()..start();
  while (watch.elapsedMicroseconds < ms * 1000) {}
}

class Busy extends GameObject {
  Busy({this.update = 0, this.fixed = 0, this.late = 0});

  double update, fixed, late;

  @override
  void onUpdate() {
    super.onUpdate();
    busy(update);
  }

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    busy(fixed);
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    busy(late);
  }
}

class BusySystem extends WorldSystem {
  BusySystem(super.world);

  double ms = 0;

  @override
  void afterUpdate() {
    super.afterUpdate();
    busy(ms);
  }

  @override
  void afterFixedUpdate() {
    super.afterFixedUpdate();
    busy(ms);
  }
}

double ms(Duration d) => d.inMicroseconds / 1000;

void main() {
  late World world;
  setUp(() => world = World());
  tearDown(() => world.dispose());

  test('is zero before the first frame', () {
    expect(world.profile.total, Duration.zero);
    expect(world.profile.updates, Duration.zero);
  });

  test('puts the game\'s updates, late updates and fixed updates each in its own part', () {
    world.time.fixedDelta = 1 / 60;
    world.add(Busy(update: 3, late: 2, fixed: 1));
    world.update(1 / 60); // starts it
    world.update(1 / 60);
    final p = world.profile;
    expect(ms(p.updates), inInclusiveRange(3, 6), reason: '$p');
    expect(ms(p.lateUpdates), inInclusiveRange(2, 5), reason: '$p');
    // One fixed step in a 1/60 s frame.
    expect(ms(p.fixedUpdates), inInclusiveRange(1, 4), reason: '$p');
    expect(ms(p.total), greaterThanOrEqualTo(6), reason: '$p');
  });

  test('gives a system its hooks\' time, every one of them, and not the part they run in', () {
    world.time.fixedDelta = 1 / 60;
    final system = world.system(BusySystem.new)..ms = 2;
    world.add(Busy(update: 1));
    world.update(1 / 60);
    world.update(1 / 60);
    final p = world.profile;
    // Its afterUpdate and its one afterFixedUpdate.
    expect(ms(system.frameTime), inInclusiveRange(4, 8), reason: '$p');
    expect(p.systems.map((s) => s.name), contains('BusySystem'));
    final listed = p.systems.firstWhere((s) => s.name == 'BusySystem');
    expect(listed.time, system.frameTime);
    // Not counted in the fixed updates, though its afterFixedUpdate runs
    // in the fixed step, nor in the rest of the world's work.
    expect(ms(p.fixedUpdates), lessThan(1.5), reason: '$p');
    expect(ms(p.other), lessThan(1.5), reason: '$p');
    expect(ms(p.updates), inInclusiveRange(1, 3), reason: '$p');
  });

  test('adds up: the parts and the systems are the whole update', () {
    world.system(BusySystem.new).ms = 1;
    world.add(Busy(update: 1, late: 1));
    for (var i = 0; i < 3; i++) {
      world.update(1 / 60);
    }
    final p = world.profile;
    final parts = [
      p.input,
      p.starts,
      p.fixedUpdates,
      p.updates,
      p.motions,
      p.lateUpdates,
      p.meshes,
      p.debug,
      p.other,
      for (final s in p.systems) s.time,
    ];
    final sum = parts.fold(Duration.zero, (a, b) => a + b);
    // Each part is rounded down to a microsecond on its own.
    expect((sum - p.total).inMicroseconds.abs(), lessThanOrEqualTo(parts.length), reason: '$p');
  });

  test('is the last finished frame\'s while one runs', () {
    late Duration seen;
    final watcher = _Watcher((w) => seen = w.profile.updates);
    final busy = world.add(Busy());
    world.add(watcher);
    world.update(1 / 60);
    busy.update = 2;
    world.update(1 / 60);
    // Read in the second frame's late update, after its busy update: the
    // first frame's, when the update did nothing.
    expect(ms(seen), lessThan(1));
    busy.update = 0;
    world.update(1 / 60);
    expect(ms(seen), greaterThanOrEqualTo(2));
  });

  test('names the built-in systems, and prints every part', () {
    world.cameras;
    world.update(1 / 60);
    final text = '${world.profile}';
    for (final part in ['input', 'starts', 'fixed updates', 'updates', 'motions', 'late updates', 'meshes', 'debug']) {
      expect(text, contains(part));
    }
    expect(world.profile.systems.map((s) => s.name), contains('Cameras'));
  });
}

class _Watcher extends GameObject {
  _Watcher(this.read);

  final void Function(World world) read;

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    read(world);
  }
}
