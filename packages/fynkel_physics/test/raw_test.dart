// The native world through package:fynkel_physics: the build hook builds
// crates/fynkel_physics_ffi, and these drive it as the later steps will.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/raw.dart';
import 'package:test/test.dart';

const step = 1 / 50;

RawPhysics withGround() {
  final physics = RawPhysics();
  addTearDown(physics.dispose);
  physics.addBox(0, const Vec3(50, 0.5, 50), position: const Vec3(0, -0.5, 0));
  return physics;
}

void main() {
  test('a dropped ball lands within a step of the formula and rests on the ground', () {
    final physics = withGround();
    final ball = physics.addBody(BodyKind.dynamic, 7, position: const Vec3(0, 10.5, 0));
    physics.addSphere(ball, 0.5);
    final expected = math.sqrt(2 * 10 / 9.81);
    double? landed;
    for (var i = 1; i <= 200; i++) {
      physics.step();
      if (landed == null && physics.bodyPose(ball).$1.y < 0.51) landed = i * step;
    }
    expect(landed, closeTo(expected, step * 1.5));
    expect(physics.bodyPose(ball).$1.y, closeTo(0.5, 0.02));
  });

  test('a step gives back the bodies it moved, by key, until they sleep', () {
    final physics = withGround();
    final keys = <int>{};
    for (var key = 100; key < 103; key++) {
      final body = physics.addBody(BodyKind.dynamic, key, position: Vec3((key - 100) * 2.0, 3, 0));
      physics.addBox(body, const Vec3(0.5, 0.5, 0.5));
    }
    final moved = physics.step();
    expect(moved, 3);
    for (var i = 0; i < moved; i++) {
      keys.add(physics.movedKey(i));
      expect(physics.movedPosition(i).y, lessThan(3));
      expect(physics.movedRotation(i).length, closeTo(1, 1e-6));
    }
    expect(keys, {100, 101, 102});
    expect(() => physics.movedKey(3), throwsRangeError);

    var last = moved;
    for (var i = 0; i < 500 && last > 0; i++) {
      last = physics.step();
    }
    expect(last, 0, reason: 'resting bodies fall asleep and aren\'t given back');
  });

  test('a teleport puts a body there at once', () {
    final physics = RawPhysics();
    addTearDown(physics.dispose);
    final body = physics.addBody(BodyKind.dynamic, 1);
    physics.addCapsule(body, 0.5, 0.25);
    final turn = Quat.axisAngle(Vec3.up, 1);
    physics.setBodyPose(body, const Vec3(1, 2, 3), turn);
    final (position, rotation) = physics.bodyPose(body);
    expect(position, const Vec3(1, 2, 3));
    expect(rotation.x, closeTo(turn.x, 1e-6));
    expect(rotation.y, closeTo(turn.y, 1e-6));
  });

  test('gravity can change', () {
    final physics = RawPhysics();
    addTearDown(physics.dispose);
    physics.gravity = const Vec3(0, 9.81, 0);
    final body = physics.addBody(BodyKind.dynamic, 1);
    physics.addSphere(body, 0.5);
    for (var i = 0; i < 50; i++) {
      physics.step();
    }
    expect(physics.bodyPose(body).$1.y, closeTo(9.81 / 2, 0.11));
  });

  test('a refused call throws with the physics\' message', () {
    final physics = RawPhysics();
    addTearDown(physics.dispose);
    final body = physics.addBody(BodyKind.dynamic, 1);
    physics.removeBody(body);
    expect(
      () => physics.removeBody(body),
      throwsA(
        isA<PhysicsException>()
            .having((e) => e.status, 'status', PhysicsStatus.noBody)
            .having((e) => e.message, 'message', 'no body $body (removed, or never added)'),
      ),
    );
    expect(
      () => physics.addSphere(0, -1),
      throwsA(isA<PhysicsException>().having((e) => e.status, 'status', PhysicsStatus.invalidValue)),
    );
    expect(
      () => physics.stepLength = 0,
      throwsA(isA<PhysicsException>().having((e) => e.status, 'status', PhysicsStatus.invalidValue)),
    );
  });

  test('the statuses the engine has too have its numbers, and every one is told apart', () {
    for (final (physics, engine) in [
      (PhysicsStatus.ok, FynkelStatus.ok),
      (PhysicsStatus.invalidValue, FynkelStatus.invalidValue),
      (PhysicsStatus.nullPointer, FynkelStatus.nullPointer),
      (PhysicsStatus.wrongSize, FynkelStatus.wrongSize),
      (PhysicsStatus.panic, FynkelStatus.panic),
      (PhysicsStatus.poisoned, FynkelStatus.poisoned),
    ]) {
      expect(physics.code, engine.index, reason: physics.name);
    }
    expect(PhysicsStatus.values.map((s) => s.code).toSet(), hasLength(PhysicsStatus.values.length));
  });

  test('a disposed world refuses to be used, and disposing twice is fine', () {
    final physics = RawPhysics()..dispose();
    expect(() => physics.step(), throwsStateError);
    physics.dispose();
  });

  test('a step on the physics\' own thread moves as a step here does, and the world waits for it', () {
    final here = withGround(), there = withGround();
    for (final physics in [here, there]) {
      for (var key = 0; key < 5; key++) {
        physics.addBox(
          physics.addBody(BodyKind.dynamic, key, position: Vec3(0, 1 + key * 1.1, 0)),
          const Vec3(0.5, 0.5, 0.5),
        );
      }
    }
    for (var i = 0; i < 50; i++) {
      final moved = here.step();
      there.beginStep();
      expect(() => there.step(), throwsA(isA<PhysicsException>()));
      expect(there.endStep(), moved);
      for (var j = 0; j < moved; j++) {
        expect((there.movedKey(j), there.movedPosition(j)), (here.movedKey(j), here.movedPosition(j)));
      }
    }
    expect(there.endStep, throwsA(isA<PhysicsException>()));
  });

  test('a raw raycast gives how far it went to the first collider, or null', () {
    final physics = withGround();
    expect(physics.raycast(const Vec3(0, 10, 0), const Vec3(0, -1, 0), 30), closeTo(10, 1e-4));
    expect(physics.raycast(const Vec3(0, 10, 0), const Vec3(0, 1, 0), 30), isNull);
    expect(physics.raycast(const Vec3(0, 10, 0), const Vec3(0, -1, 0), 5), isNull);
  });
}
