// Motion (docs/stage-4.md §3.7, step 6): tweens, their controls, groups,
// owners, and awaiting them in routines.

import 'dart:async';
import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

final log = <String>[];

void expectVec(Vec3 actual, Vec3 expected, [double tolerance = 1e-9]) =>
    expect(actual.closeTo(expected, tolerance), isTrue, reason: '$actual is not $expected');

/// Logs where its x is in its update and late update.
class Watched extends GameObject {
  @override
  void onUpdate() {
    super.onUpdate();
    log.add('update ${position.x}');
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    log.add('late ${position.x}');
  }
}

/// Starts a tween in its first update.
class TweensInUpdate extends GameObject {
  final values = <double>[];

  @override
  void onStart() {
    super.onStart();
    tween(0.0, 1.0, seconds: 1, ease: Ease.linear, onUpdate: values.add);
  }
}

class Mover extends Behaviour {}

void main() {
  late World world;
  late GameObject thing;

  setUp(() {
    world = World();
    thing = world.add(GameObject(name: 'thing'));
    log.clear();
  });

  tearDown(() => world.dispose());

  void frame([double dt = 0.25]) => world.update(dt);

  void frames(int n, [double dt = 0.25]) {
    for (var i = 0; i < n; i++) {
      frame(dt);
    }
  }

  group('a tween', () {
    test('goes from one end to the other over its seconds, and completes', () {
      final values = <double>[];
      var completed = 0;
      final m = thing.tween(
        0.0,
        2.0,
        seconds: 1,
        ease: Ease.linear,
        onUpdate: values.add,
        onComplete: () => completed++,
      );
      expect(values, isEmpty, reason: 'nothing until the next frame');
      frames(3);
      expect(values, [0.5, 1.0, 1.5]);
      expect(m.isRunning, isTrue);
      frames(2);
      expect(values, [0.5, 1.0, 1.5, 2.0], reason: 'nothing after its end');
      expect((m.isRunning, m.isCompleted, m.isStopped, completed), (false, true, false, 1));
    });

    test('made during a frame, it moves from the next one', () {
      final t = world.add(TweensInUpdate());
      frame();
      expect(t.values, isEmpty);
      frame();
      expect(t.values, [0.25]);
    });

    test('eases out by default, as DOTween does', () {
      final values = <double>[];
      thing.tween(0.0, 1.0, seconds: 1, onUpdate: values.add);
      frames(2);
      expect(values, [Ease.outQuad(0.25), 0.75]);
    });

    test('knows Vec2, Vec3, Quat (the short way round) and Color', () {
      Object? v2, v3, q, c;
      thing.tween(Vec2.zero, const Vec2(2, 4), seconds: 1, ease: Ease.linear, onUpdate: (v) => v2 = v);
      thing.tween(Vec3.zero, const Vec3(2, 4, 6), seconds: 1, ease: Ease.linear, onUpdate: (v) => v3 = v);
      thing.tween(
        Quat.identity,
        Quat.axisAngle(Vec3.up, math.pi / 2),
        seconds: 1,
        ease: Ease.linear,
        onUpdate: (v) => q = v,
      );
      // -q is the same rotation: still the short way.
      thing.tween(Color.black, const Color.rgb(1, 0.5, 0, 0), seconds: 1, ease: Ease.linear, onUpdate: (v) => c = v);
      frames(2);
      expect(v2, const Vec2(1, 2));
      expect(v3, const Vec3(1, 2, 3));
      expect((q! as Quat).closeTo(Quat.axisAngle(Vec3.up, math.pi / 4)), isTrue);
      expect(c, const Color.rgb(0.5, 0.25, 0, 0.5));

      final m = Quat.axisAngle(Vec3.up, math.pi / 2);
      Quat? short;
      thing.tween(
        Quat.identity,
        Quat(-m.x, -m.y, -m.z, -m.w),
        seconds: 1,
        ease: Ease.linear,
        onUpdate: (v) => short = v,
      );
      frames(2);
      expect(short!.closeTo(Quat.axisAngle(Vec3.up, math.pi / 4)), isTrue);
    });

    test('takes a lerp for any other type, and says so without one', () {
      final values = <int>[];
      thing.tween(
        0,
        10,
        seconds: 1,
        ease: Ease.linear,
        lerp: (a, b, t) => (a + (b - a) * t).round(),
        onUpdate: values.add,
      );
      frames(2);
      expect(values, [3, 5]);
      expect(
        () => thing.tween(0, 10, seconds: 1, onUpdate: (_) {}),
        throwsA(isA<ArgumentError>().having((e) => '${e.message}', 'message', contains('0.0, not 0'))),
      );
      expect(() => thing.tween('a', 'b', seconds: 1, onUpdate: (_) {}), throwsArgumentError);
    });

    test('waits its delay, then reads what it moves from', () {
      final m = thing.moveTo(const Vec3(4, 0, 0), seconds: 1, ease: Ease.linear, delay: 0.5);
      frame();
      thing.position = const Vec3(2, 0, 0);
      frame();
      expectVec(thing.position, const Vec3(2, 0, 0), 0);
      frame();
      expectVec(thing.position, const Vec3(2.5, 0, 0));
      frames(3);
      expectVec(thing.position, const Vec3(4, 0, 0));
      expect(m.isCompleted, isTrue);
    });

    test('loops start over; a yoyo goes back every other time; -1 is for ever', () {
      final plain = <double>[], yoyo = <double>[], ever = <double>[];
      final a = thing.tween(0.0, 1.0, seconds: 0.5, ease: Ease.linear, loops: 2, onUpdate: plain.add);
      final b = thing.tween(0.0, 1.0, seconds: 0.5, ease: Ease.linear, loops: 3, yoyo: true, onUpdate: yoyo.add);
      final c = thing.tween(0.0, 1.0, seconds: 0.5, ease: Ease.linear, loops: -1, yoyo: true, onUpdate: ever.add);
      frames(7);
      expect(plain, [0.5, 1.0, 0.5, 1.0]);
      expect(yoyo, [0.5, 1.0, 0.5, 0.0, 0.5, 1.0]);
      expect(ever, [0.5, 1.0, 0.5, 0.0, 0.5, 1.0, 0.5]);
      expect((a.isCompleted, b.isCompleted, c.isRunning), (true, true, true));
    });

    test('a yoyo plays its ease backwards on the way back', () {
      final values = <double>[];
      thing.tween(0.0, 1.0, seconds: 1, ease: Ease.inQuad, loops: 2, yoyo: true, onUpdate: values.add);
      frames(5);
      expect(values, [0.0625, 0.25, 0.5625, 1.0, 0.5625]);
    });

    test('at a speed, it lasts as long as the distance takes', () {
      final m = thing.moveTo(const Vec3(0, 0, -4), speed: 2, ease: Ease.linear);
      frames(4);
      expectVec(thing.position, const Vec3(0, 0, -2));
      frames(4);
      expectVec(thing.position, const Vec3(0, 0, -4));
      expect(m.isCompleted, isTrue);
      frame(0.125);

      final r = thing.rotateTo(Quat.axisAngle(Vec3.up, math.pi), speed: math.pi, ease: Ease.linear);
      frames(2);
      expect(thing.rotation.closeTo(Quat.axisAngle(Vec3.up, math.pi / 2)), isTrue);
      frames(2);
      expect(r.isCompleted, isTrue);
    });

    test('refuses both seconds and speed, neither, a bad number, or a speed for a colour', () {
      expect(() => thing.tween(0.0, 1.0, onUpdate: (_) {}), throwsArgumentError);
      expect(() => thing.tween(0.0, 1.0, seconds: 1, speed: 1, onUpdate: (_) {}), throwsArgumentError);
      expect(() => thing.tween(0.0, 1.0, seconds: -1, onUpdate: (_) {}), throwsArgumentError);
      expect(() => thing.tween(0.0, 1.0, speed: 0, onUpdate: (_) {}), throwsArgumentError);
      expect(() => thing.tween(0.0, 1.0, seconds: 1, loops: 0, onUpdate: (_) {}), throwsArgumentError);
      expect(() => thing.tween(0.0, 1.0, seconds: 1, delay: double.nan, onUpdate: (_) {}), throwsArgumentError);
      expect(() => thing.tween(Color.black, Color.white, speed: 1, onUpdate: (_) {}), throwsArgumentError);
    });

    test('zero seconds jumps to the end in its first frame', () {
      final values = <double>[];
      final m = thing.tween(0.0, 1.0, seconds: 0, onUpdate: values.add);
      frame();
      expect(values, [1.0]);
      expect(m.isCompleted, isTrue);
    });

    test("runs on the game's time, or on real time when unscaled", () {
      final scaled = <double>[], unscaled = <double>[];
      world.time.scale = 0.5;
      thing.tween(0.0, 1.0, seconds: 1, ease: Ease.linear, onUpdate: scaled.add);
      thing.tween(0.0, 1.0, seconds: 1, ease: Ease.linear, unscaled: true, onUpdate: unscaled.add);
      frames(2);
      world.time.scale = 0;
      frame();
      expect(scaled, [0.125, 0.25, 0.25]);
      expect(unscaled, [0.25, 0.5, 0.75]);
    });

    test('stop leaves it where it is, without onComplete; complete jumps to the end, with it', () async {
      final a = <double>[], b = <double>[];
      var completed = 0;
      final stopped = thing.tween(
        0.0,
        1.0,
        seconds: 1,
        ease: Ease.linear,
        onUpdate: a.add,
        onComplete: () => completed++,
      );
      final done = thing.tween(0.0, 1.0, seconds: 1, ease: Ease.linear, onUpdate: b.add, onComplete: () => completed++);
      frame();
      stopped.stop();
      done.complete();
      frame();
      expect(a, [0.25]);
      expect(b, [0.25, 1.0]);
      expect(completed, 1);
      expect((stopped.isStopped, done.isCompleted), (true, true));
      await stopped;
      await done;
      stopped.stop();
      done.complete();
      expect(completed, 1, reason: 'an ended motion does nothing more');
    });

    test('completing one that loops for ever ends the play it is in', () {
      final values = <double>[];
      final m = thing.tween(0.0, 1.0, seconds: 1, ease: Ease.linear, loops: -1, yoyo: true, onUpdate: values.add);
      frames(5);
      expect(values.last, 0.75);
      m.complete();
      expect((values.last, m.isCompleted), (0.0, true), reason: 'its second play goes back, to 0');
    });

    test('what onUpdate throws goes to world.errors and stops it; the others go on', () async {
      final errors = <ObjectError>[];
      world.errors.listen(errors.add);
      final values = <double>[];
      final bad = thing.tween(0.0, 1.0, seconds: 1, onUpdate: (_) => throw StateError('bad'));
      thing.tween(0.0, 1.0, seconds: 1, ease: Ease.linear, onUpdate: values.add);
      frames(2);
      await pumpEventQueue();
      expect(errors.map((e) => '${e.error}'), ['Bad state: bad']);
      expect(errors.single.gameObject, same(thing));
      expect(bad.isStopped, isTrue);
      expect(values, [0.25, 0.5]);
    });

    test('moves after the updates and before the late updates', () {
      final watched = world.add(Watched());
      frame();
      log.clear();
      watched.moveTo(const Vec3(1, 0, 0), seconds: 1, ease: Ease.linear);
      frame();
      expect(log, ['update 0.0', 'late 0.25']);
    });
  });

  group('its owner', () {
    test('destroyed, stops it', () {
      final m = thing.moveTo(const Vec3(1, 0, 0), seconds: 1);
      frame();
      thing.destroy();
      frame();
      expect(m.isStopped, isTrue);
    });

    test('hidden, holds it still; shown again, it goes on', () {
      final values = <double>[];
      thing.tween(0.0, 1.0, seconds: 1, ease: Ease.linear, onUpdate: values.add);
      frame();
      thing.active = false;
      frames(2);
      thing.active = true;
      frame();
      expect(values, [0.25, 0.5]);
    });

    test('a behaviour: removed, stops it; disabled, holds it', () {
      final mover = thing.addComponent(Mover());
      frame();
      final values = <double>[];
      final m = mover.tween(0.0, 1.0, seconds: 1, ease: Ease.linear, onUpdate: values.add);
      frame();
      mover.enabled = false;
      frame();
      mover.enabled = true;
      frame();
      thing.removeComponent(mover);
      frame();
      expect(values, [0.25, 0.5]);
      expect(m.isStopped, isTrue);
      expect(() => mover.tween(0.0, 1.0, seconds: 1, onUpdate: (_) {}), throwsStateError);
    });

    test('destroyed, can start none', () {
      thing.destroy();
      expect(() => thing.moveTo(Vec3.zero, seconds: 1), throwsStateError);
    });
  });

  group('in a routine', () {
    test('await goes on in the routines step of the frame it ends', () {
      thing.startRoutine(() async {
        await thing.moveTo(const Vec3(1, 0, 0), seconds: 0.5);
        log.add('moved, frame ${world.time.frame}, x ${thing.position.x}');
      });
      frames(3);
      expect(log, ['moved, frame 2, x 1.0']);
    });

    test('a stopped routine unwinds from it, and the motion goes on', () {
      late Motion m;
      final r = thing.startRoutine(() async {
        try {
          await (m = thing.moveTo(const Vec3(1, 0, 0), seconds: 1));
        } finally {
          log.add('finally');
        }
      });
      frame();
      r.stop();
      expect(log, ['finally']);
      frame();
      expect(m.isRunning, isTrue);
    });

    test('race stops the motion that loses', () {
      late Motion m;
      thing.startRoutine(() async {
        await race([m = thing.moveTo(const Vec3(1, 0, 0), seconds: 2), wait(0.5)]);
        log.add('won');
      });
      frames(3);
      expect(log, ['won']);
      expect(m.isStopped, isTrue);
      expect(thing.position.x, lessThan(1));
    });

    test('outside a routine, it is a plain future', () async {
      final m = thing.moveTo(const Vec3(1, 0, 0), seconds: 0.25);
      var done = false;
      unawaited(m.then((_) => done = true));
      frame();
      await pumpEventQueue();
      expect(done, isTrue);
    });
  });

  group('a sequence', () {
    test('looping without a yoyo, moveBy goes on from where the last play left it', () {
      thing.sequence([thing.moveBy(const Vec3(1, 0, 0), seconds: 0.5, ease: Ease.linear)], loops: 2);
      final xs = <double>[];
      for (var i = 0; i < 4; i++) {
        frame();
        xs.add(thing.position.x);
      }
      expect(xs, [0.5, 1, 1.5, 2]);
    });

    test('a yoyo that plays forwards again leaves the later parts alone until their turn', () {
      final parts = <String>[];
      thing.sequence(
        [
          thing.moveTo(const Vec3(1, 0, 0), seconds: 0.5, ease: Ease.linear),
          thing.moveTo(const Vec3(1, 1, 0), seconds: 0.5, ease: Ease.linear, onComplete: () => parts.add('up')),
        ],
        loops: 3,
        yoyo: true,
      );
      final at = <Vec3>[];
      for (var i = 0; i < 12; i++) {
        frame();
        at.add(thing.position);
      }
      // The third play, forwards again: half way along the first part.
      expectVec(at[8], const Vec3(0.5, 0, 0));
      expectVec(at[9], const Vec3(1, 0, 0));
      expectVec(at[11], const Vec3(1, 1, 0));
      expect(parts, ['up', 'up'], reason: 'each time the group plays it to its end');
    });

    test('plays its motions one after another, each from where the last left it', () {
      final parts = <String>[];
      final s = thing.sequence([
        thing.moveTo(const Vec3(2, 0, 0), seconds: 1, ease: Ease.linear, onComplete: () => parts.add('first')),
        thing.moveTo(const Vec3(2, 0, 2), seconds: 0.5, ease: Ease.linear, onComplete: () => parts.add('second')),
      ], onComplete: () => parts.add('all'));
      frames(2);
      expectVec(thing.position, const Vec3(1, 0, 0));
      frames(3);
      expectVec(thing.position, const Vec3(2, 0, 1));
      expect(parts, ['first']);
      frame();
      expectVec(thing.position, const Vec3(2, 0, 2));
      expect(parts, ['first', 'second', 'all']);
      expect(s.isCompleted, isTrue);
    });

    test('a yoyo plays them all back, the last first', () {
      thing.sequence(
        [
          thing.moveTo(const Vec3(1, 0, 0), seconds: 0.5, ease: Ease.linear),
          thing.moveTo(const Vec3(1, 1, 0), seconds: 0.5, ease: Ease.linear),
        ],
        loops: 2,
        yoyo: true,
      );
      final xs = <Vec3>[];
      for (var i = 0; i < 9; i++) {
        frame();
        xs.add(thing.position);
      }
      const path = [
        Vec3(0.5, 0, 0),
        Vec3(1, 0, 0),
        Vec3(1, 0.5, 0),
        Vec3(1, 1, 0),
        Vec3(1, 0.5, 0),
        Vec3(1, 0, 0),
        Vec3(0.5, 0, 0),
        Vec3(0, 0, 0),
        Vec3(0, 0, 0),
      ];
      for (final (i, p) in path.indexed) {
        expectVec(xs[i], p);
      }
    });

    test('looping without a yoyo, each play begins again from where it is: a patrol', () {
      thing.position = const Vec3(-1, 0, 0);
      thing.sequence([
        thing.moveTo(const Vec3(1, 0, 0), seconds: 0.5, ease: Ease.linear),
        thing.moveTo(const Vec3(-1, 0, 0), seconds: 0.5, ease: Ease.linear),
      ], loops: -1);
      final xs = <double>[];
      for (var i = 0; i < 8; i++) {
        frame();
        xs.add(thing.position.x);
      }
      expect(xs, [0, 1, 0, -1, 0, 1, 0, -1]);
    });

    test('works out the length of a part at a speed when it begins', () {
      final s = thing.sequence([
        thing.moveTo(const Vec3(1, 0, 0), speed: 2, ease: Ease.linear),
        thing.moveTo(const Vec3(1, 0, 3), speed: 2, ease: Ease.linear),
      ]);
      // 1 at 2 a second, then 3: 0.5 s, then 1.5 s.
      frames(4);
      expectVec(thing.position, const Vec3(1, 0, 1));
      frames(3);
      expect(s.isRunning, isTrue);
      frame();
      expectVec(thing.position, const Vec3(1, 0, 3));
      expect(s.isCompleted, isTrue);
    });

    test("counts a part's delay and loops as part of it", () {
      final values = <double>[];
      thing.sequence([
        thing.tween(0.0, 1.0, seconds: 0.25, ease: Ease.linear, loops: 2, yoyo: true, onUpdate: values.add),
        thing.tween(5.0, 6.0, seconds: 0.5, delay: 0.25, ease: Ease.linear, onUpdate: values.add),
      ]);
      frames(5);
      expect(values, [1.0, 0.0, 5.0, 5.5, 6.0]);
    });
  });

  group('together', () {
    test('plays them all at once, as long as the longest', () {
      final a = <double>[], b = <double>[];
      final t = thing.together([
        thing.tween(0.0, 1.0, seconds: 0.5, ease: Ease.linear, onUpdate: a.add),
        thing.tween(0.0, 1.0, seconds: 1, ease: Ease.linear, onUpdate: b.add),
      ]);
      frames(3);
      expect(a, [0.5, 1.0]);
      expect(b, [0.25, 0.5, 0.75]);
      expect(t.isRunning, isTrue);
      frame();
      expect(t.isCompleted, isTrue);
    });

    test('in a sequence, with a yoyo', () {
      final values = <String>[];
      thing.sequence(
        [
          thing.together([
            thing.tween(0.0, 1.0, seconds: 0.5, ease: Ease.linear, onUpdate: (v) => values.add('a$v')),
            thing.tween(0.0, 1.0, seconds: 0.25, ease: Ease.linear, onUpdate: (v) => values.add('b$v')),
          ]),
          thing.tween(0.0, 1.0, seconds: 0.25, ease: Ease.linear, onUpdate: (v) => values.add('c$v')),
        ],
        loops: 2,
        yoyo: true,
      );
      frames(6);
      // c begins as the others end, at 0.5 s; back, it ends where it began.
      expect(values, ['a0.5', 'b1.0', 'a1.0', 'c0.0', 'c1.0', 'c0.0', 'a0.5', 'b0.0', 'a0.0']);
    });
  });

  group('groups', () {
    test('take motions made in this frame and not yet in a group, none for ever', () {
      final old = thing.moveTo(Vec3.zero, seconds: 1);
      frame();
      expect(() => thing.sequence([old]), throwsStateError);
      final m = thing.moveTo(Vec3.zero, seconds: 1);
      thing.sequence([m]);
      expect(() => thing.together([m]), throwsStateError);
      expect(() => thing.sequence([thing.moveTo(Vec3.zero, seconds: 1, loops: -1)]), throwsArgumentError);
      expect(() => thing.sequence([]), throwsArgumentError);
    });

    test("stopping a part stops the group; the parts' futures complete with it", () async {
      final a = thing.moveTo(const Vec3(1, 0, 0), seconds: 1);
      final b = thing.moveTo(const Vec3(2, 0, 0), seconds: 1);
      final s = thing.sequence([a, b]);
      frame();
      a.stop();
      expect((s.isStopped, a.isStopped, b.isStopped), (true, true, true));
      await Future.wait([a, b, s]);
    });

    test("a part whose owner goes drops out; the group goes on", () {
      final other = world.add(GameObject());
      final s = thing.together([
        other.moveTo(const Vec3(1, 0, 0), seconds: 1, ease: Ease.linear),
        thing.moveTo(const Vec3(1, 0, 0), seconds: 1, ease: Ease.linear),
      ]);
      frame();
      other.destroy();
      frames(3);
      expect((s.isCompleted, thing.position.x), (true, 1.0));
    });
  });

  group('the helpers', () {
    test('moveBy, scaleTo and rotateBy, from where the object is', () {
      thing.position = const Vec3(1, 1, 1);
      thing.moveBy(const Vec3(2, 0, 0), seconds: 1, ease: Ease.linear);
      thing.scaleTo(const Vec3(3, 3, 3), seconds: 1, ease: Ease.linear);
      thing.rotateBy(Vec3.up, 2 * math.pi, seconds: 1, ease: Ease.linear);
      frame();
      expectVec(thing.position, const Vec3(1.5, 1, 1));
      expectVec(thing.localScale, const Vec3(1.5, 1.5, 1.5));
      expect(thing.rotation.closeTo(Quat.axisAngle(Vec3.up, math.pi / 2)), isTrue);
      frames(3);
      expectVec(thing.position, const Vec3(3, 1, 1));
      expect(thing.rotation.closeTo(Quat.identity), isTrue, reason: 'a whole turn');
    });

    test('a behaviour moves its object by hand as the object does: position, rotate, translate, scale', () {
      final mover = thing.addComponent(Mover());
      mover.position = const Vec3(1, 0, 0);
      mover.translate(const Vec3(0, 2, 0));
      mover.rotate(Vec3.up, math.pi / 2);
      mover.localScale = Vec3.one * 2;
      expectVec(thing.position, const Vec3(1, 2, 0));
      expect(thing.rotation.closeTo(Quat.axisAngle(Vec3.up, math.pi / 2)), isTrue);
      expectVec(thing.localScale, const Vec3(2, 2, 2));
    });

    test('lookAtOver ends looking at the point', () {
      thing.lookAtOver(const Vec3(5, 0, 0), seconds: 0.5);
      frames(2);
      expectVec(thing.forward, Vec3.right, 1e-6);
    });

    test("colorTo changes its renderer's material, or the one given", () {
      final paint = Material(color: Color.black);
      thing.addComponent(MeshRenderer(Mesh.cube(), paint));
      thing.colorTo(Color.white, seconds: 1, ease: Ease.linear);
      frames(2);
      expect(paint.color, const Color.rgb(0.5, 0.5, 0.5));
      final bare = world.add(GameObject());
      expect(() => bare.colorTo(Color.white, seconds: 1), throwsStateError);
      final other = Material();
      bare.colorTo(Color.black, seconds: 0.25, material: other);
      frame();
      expect(other.color, Color.black);
    });

    test('punchScale swells by up to the punch and ends where it began', () {
      thing.localScale = const Vec3(2, 2, 2);
      final m = thing.punchScale(const Vec3(1, 1, 1), seconds: 1);
      var most = 0.0;
      for (var i = 0; i < 64; i++) {
        frame(1 / 64);
        most = math.max(most, thing.localScale.x - 2);
      }
      expect(most, inInclusiveRange(0.5, 1));
      expectVec(thing.localScale, const Vec3(2, 2, 2));
      expect(m.isCompleted, isTrue);
    });

    test('shake stays within its strength, ends where it began, and repeats with a seed', () {
      List<Vec3> run() {
        final w = World();
        final o = w.add(GameObject(transform: Transform(position: const Vec3(1, 2, 3))));
        o.shake(0.5, seconds: 1, seed: 7);
        final at = <Vec3>[];
        for (var i = 0; i < 32; i++) {
          w.update(1 / 32);
          at.add(o.position);
        }
        w.dispose();
        return at;
      }

      final a = run(), b = run();
      expect(a, b);
      for (final p in a) {
        final d = p - const Vec3(1, 2, 3);
        expect([d.x.abs(), d.y.abs(), d.z.abs()].reduce(math.max), lessThanOrEqualTo(0.5));
      }
      expect(a.any((p) => p != const Vec3(1, 2, 3)), isTrue);
      expectVec(a.last, const Vec3(1, 2, 3));
    });

    test("a game's own field named like a helper's value doesn't change what the helper moves", () {
      final o = world.add(_OwnPosition());
      o.moveTo(const Vec3(2, 0, 0), seconds: 0.5, ease: Ease.linear);
      frames(2);
      expectVec(o.transform.position, const Vec3(2, 0, 0));
    });
  });
}

/// Has a `position` of its own, which isn't where it is.
class _OwnPosition extends GameObject {
  @override
  Vec3 get position => const Vec3(100, 100, 100);
}
