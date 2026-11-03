// A body a step moved costs its object's new transform and nothing more
// (stage 16, the pile): 100 bodies turning without end, counted as
// alloc_test.dart counts. A Transform is a value, so each move makes one,
// with its position and rotation; reading the step and keeping the body's
// velocities make nothing. 1,000 tumbling cubes made 596 KB a frame.

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

import 'allocations.dart';

const _bodies = 100;
const _frames = 6000;

void main() {
  final madeEachFrame = allocations();

  test('a moved body makes only its transform, position and rotation', () async {
    final world = World();
    addTearDown(world.dispose);
    final bodies = <Rigidbody>[];
    for (var i = 0; i < _bodies; i++) {
      final body = Rigidbody(useGravity: false, angularDamping: 0);
      world.add(
        GameObject(
          transform: Transform(position: Vec3(i * 3.0, 0, 0)),
          components: [BoxCollider(), body],
        ),
      );
      bodies.add(body);
    }
    world.update(1 / 60);
    for (final body in bodies) {
      body.angularVelocity = const Vec3(0, 2, 0);
    }

    final made = await madeEachFrame(world, _frames);
    // 50 steps a second, frames of 1/60 s: each body moved at each step.
    const moves = _bodies * _frames * 50 ~/ 60;
    const allowed = {'Transform', 'Vec3', 'Quat'};
    expect(made.keys.where((name) => !allowed.contains(name)), isEmpty, reason: '$made');
    for (final name in allowed) {
      expect(made[name] ?? 0, lessThanOrEqualTo(moves * 1.05), reason: '$name: $made');
    }
    // And they all moved throughout (their speed drifts over the 100,000
    // frames of warming, 2 to about 2.4, but they never stop).
    expect(made['Transform'] ?? 0, greaterThanOrEqualTo(moves * 0.95), reason: '$made');
  });

  // Under a parent, the object's transform is its pose against the
  // parent's: that position and rotation are made too, but nothing on the
  // way to them (the pile's cubes are a scene's children).
  test('a moved body under a parent makes only its pose and its local pose', () async {
    final world = World();
    addTearDown(world.dispose);
    final parent = world.add(
      GameObject(
        transform: Transform(
          position: Vec3(1, 2, 3),
          rotation: Quat.axisAngle(Vec3(0, 1, 0), 0.5),
          scale: Vec3(2, 2, 2),
        ),
      ),
    );
    final bodies = <Rigidbody>[];
    for (var i = 0; i < _bodies; i++) {
      final body = Rigidbody(useGravity: false, angularDamping: 0);
      parent.add(
        GameObject(
          transform: Transform(position: Vec3(i * 3.0, 0, 0)),
          components: [BoxCollider(), body],
        ),
      );
      bodies.add(body);
    }
    world.update(1 / 60);
    for (final body in bodies) {
      body.angularVelocity = const Vec3(0, 2, 0);
    }

    final made = await madeEachFrame(world, _frames);
    const moves = _bodies * _frames * 50 ~/ 60;
    const allowed = {'Transform': 1, 'Vec3': 2, 'Quat': 2};
    expect(made.keys.where((name) => !allowed.containsKey(name)), isEmpty, reason: '$made');
    for (final MapEntry(key: name, value: each) in allowed.entries) {
      expect(made[name] ?? 0, lessThanOrEqualTo(moves * each * 1.05), reason: '$name: $made');
    }
    expect(made['Transform'] ?? 0, greaterThanOrEqualTo(moves * 0.95), reason: '$made');
  });
}
