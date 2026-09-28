// Code first, Tier 3 (docs/code-first.md, ADR-0026 §7): tweens —
// `transform.moveTo` / `rotateTo` / `scaleTo` and a general `tween`, each
// a coroutine a script can yield, `await` or stop. Over the real native
// library, with a hand-driven clock.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class Sequence extends Behaviour {
  final log = <String>[];

  @override
  void start() => startCoroutine(run());

  Iterable<Object?> run() sync* {
    yield transform.moveTo(const Vec3(0, 2, 0), 0.5, ease: Ease.linear);
    log.add('moved at ${transform.localPosition.y}');
    yield transform.scaleTo(const Vec3(2, 2, 2), 0.25);
    log.add('scaled to ${transform.localScale.x}');
  }
}

class Fader extends Behaviour {
  double intensity = 0;
  Coroutine? fade;

  @override
  void start() => fade = tween(0.0, 8.0, 1, (v) => intensity = v, ease: Ease.linear);
}

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    return w;
  }

  void frames(World w, int n) {
    for (var i = 0; i < n; i++) {
      w.frame(1 / 60);
    }
  }

  void near(Vec3 a, Vec3 b) {
    expect((a - b).length, lessThan(1e-4), reason: '$a vs $b');
  }

  test('moveTo: halfway at half time, there at the end, then done', () async {
    final w = fresh();
    final box = w.spawn(name: 'Box', transform: const Pose(position: Vec3(0, 0, 0)));
    frames(w, 1);
    final move = box.transform.moveTo(const Vec3(10, 0, 0), 1, ease: Ease.linear);
    var finished = false;
    move.done.then((_) => finished = true);
    frames(w, 30);
    near(box.transform.position, const Vec3(5, 0, 0));
    expect(move.isRunning, isTrue);
    frames(w, 30);
    near(box.transform.position, const Vec3(10, 0, 0));
    expect(move.isRunning, isFalse);
    frames(w, 2);
    await Future<void>.delayed(Duration.zero);
    expect(finished, isTrue);
    near(box.worldTransform!.position, const Vec3(10, 0, 0));
  });

  test('moveTo is in the world under a parent; moveLocalTo, rotateTo and scaleTo', () {
    final w = fresh();
    final parent = w.spawn(name: 'Parent', transform: const Pose(position: Vec3(0, 5, 0)));
    final child = w.spawn(name: 'Child', parent: parent);
    frames(w, 2);
    child.transform.moveTo(const Vec3(0, 5, 4), 0.5);
    child.transform.rotateTo(Quat.euler(const Vec3(0, 90, 0)), 1, ease: Ease.linear);
    child.transform.scaleTo(const Vec3(3, 3, 3), 0.5, ease: Ease.inOutCubic);
    frames(w, 30);
    near(child.transform.localPosition, const Vec3(0, 0, 4));
    near(child.transform.localScale, const Vec3(3, 3, 3));
    final halfway = child.transform.rotation;
    expect(halfway.eulerAngles.y, closeTo(45, 1e-3), reason: 'slerp at a constant speed');
    frames(w, 30);
    expect(child.transform.rotation.eulerAngles.y, closeTo(90, 1e-3));

    parent.transform.moveLocalTo(const Vec3(1, 5, 0), 0);
    frames(w, 1);
    near(parent.transform.localPosition, const Vec3(1, 5, 0));
  });

  test('a script yields tweens in order; Behaviour.tween stops with its script', () {
    final w = fresh();
    final seq = Sequence();
    w.spawn(name: 'Seq').addBehaviour(seq);
    frames(w, 1 + 30 + 1);
    expect(seq.log, ['moved at 2.0']);
    frames(w, 16);
    expect(seq.log, ['moved at 2.0', 'scaled to 2.0']);

    final fader = Fader();
    final lamp = w.spawn(name: 'Lamp')..addBehaviour(fader);
    frames(w, 15); // started in the first frame's update, a frame's time in by its end
    expect(fader.intensity, closeTo(2, 1e-6));
    lamp.setActive(false);
    frames(w, 30);
    expect(fader.fade!.isRunning, isFalse, reason: 'its object left the active hierarchy');
    expect(fader.intensity, closeTo(2, 0.2));
  });

  test('stop leaves the object where it got to; a destroyed object ends its tweens', () {
    final w = fresh();
    final a = w.spawn(name: 'A'), b = w.spawn(name: 'B');
    frames(w, 1);
    final move = a.transform.moveTo(const Vec3(6, 0, 0), 1, ease: Ease.linear);
    final other = b.transform.moveTo(const Vec3(6, 0, 0), 1);
    frames(w, 20);
    move.stop();
    b.destroy();
    frames(w, 20);
    near(a.transform.localPosition, const Vec3(2, 0, 0));
    expect(other.isRunning, isFalse);
  });

  test('World.tween: numbers, whole numbers, colours, rotations; bad input throws before it starts', () {
    final w = fresh();
    final seen = <Object>[];
    w.tween(0.0, 1.0, 1, seen.add, ease: Ease.linear);
    w.tween(0, 10, 1, seen.add, ease: Ease.linear);
    w.tween(const Vec4(0, 0, 0, 1), const Vec4(1, 0.5, 0, 1), 1, seen.add, ease: Ease.linear);
    expect(seen, [0.0, 0, const Vec4(0, 0, 0, 1)], reason: 'the first value now');
    seen.clear();
    frames(w, 30);
    final [number, whole, colour as Vec4] = seen.sublist(seen.length - 3);
    expect(number, closeTo(0.5, 1e-9));
    expect(whole, 5);
    expect((colour - const Vec4(0.5, 0.25, 0, 1)).dot(colour - const Vec4(0.5, 0.25, 0, 1)), lessThan(1e-12));

    expect(() => w.tween('a', 'b', 1, (_) {}), throwsArgumentError);
    expect(() => w.tween(0.0, 1.0, -1, (_) {}), throwsArgumentError);
    expect(() => w.tween(0.0, 1.0, double.nan, (_) {}), throwsArgumentError);
    final zero = <double>[];
    w.tween(3.0, 4.0, 0, zero.add);
    expect(zero, [4.0], reason: 'no time: the end at once');
  });

  test('a tween that throws reports once and stops', () {
    final w = fresh();
    final errors = <BehaviourError>[];
    w.onBehaviourError = errors.add;
    var calls = 0;
    final t = w.tween(0.0, 1.0, 1, (v) {
      if (++calls == 3) throw StateError('boom');
    });
    frames(w, 10);
    expect(errors.single.behaviour, isNull);
    expect(errors.single.message, 'World.tween threw: Bad state: boom');
    expect(t.isRunning, isFalse);
  });

  test('every ease starts at 0 and ends at 1', () {
    for (final e in Ease.values) {
      expect(e(0), closeTo(0, 1e-9), reason: '$e');
      expect(e(1), closeTo(1, 1e-9), reason: '$e');
    }
    expect(Ease.outQuad(0.5), 0.75);
    expect(Ease.inQuad(0.5), 0.25);
    expect(Ease.inOutSine(0.5), closeTo(0.5, 1e-12));
    expect(Ease.outBack(0.8), greaterThan(1), reason: 'overshoots');
    expect(Quat.identity.slerp(Quat.axisAngle(Vec3.up, math.pi), 0.5).eulerAngles.y.abs(), closeTo(90, 1e-6));
  });
}
