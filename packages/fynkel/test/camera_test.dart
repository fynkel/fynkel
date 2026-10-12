// Cameras (docs/stage-4.md §3.8, step 7): the orthographic lens, between
// the screen and the world, virtual cameras with their priorities, blends
// and cuts, the rigs, and shake, against hand-worked values.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'golden.dart';
import 'render_test.dart' show stage1;

// The world's transforms come back from the engine in 32-bit floats.
void expectVec(Vec3 actual, Vec3 expected, [double tolerance = 1e-6]) =>
    expect(actual.closeTo(expected, tolerance), isTrue, reason: '$actual is not $expected');

void expectPoint(ViewPoint? actual, double x, double y, [double tolerance = 1e-6]) {
  expect(actual, isNotNull);
  expect(actual!.x, closeTo(x, tolerance), reason: '$actual');
  expect(actual.y, closeTo(y, tolerance), reason: '$actual');
}

class Grab extends GameObject {
  @override
  bool onDragStart(Finger finger) => true;
}

class MoveLate extends Behaviour {
  @override
  void onLateUpdate() {
    super.onLateUpdate();
    position += const Vec3(1, 0, 0);
  }
}

void main() {
  late World world;

  setUp(() => world = World());
  tearDown(() => world.dispose());

  void frames(int n, [double dt = 0.25]) {
    for (var i = 0; i < n; i++) {
      world.update(dt);
    }
  }

  // The view's camera at the origin, looking along -Z.
  GameObject camera({Camera? lens}) => world.add(GameObject(components: [lens ?? Camera()]));

  group('the lens', () {
    test('orthographic and orthographicSize are written to the engine and read back', () {
      final o = camera(lens: Camera(orthographic: true, orthographicSize: 3));
      expect(o.get<Camera>()!.orthographic, isTrue);
      o.get<Camera>()!.orthographicSize = 2;
      expect(o.get<Camera>()!.orthographicSize, 2);
      expect(() => o.get<Camera>()!.orthographicSize = 0, throwsA(isA<FynkelException>()));
      expect(o.get<Camera>()!.orthographicSize, 2, reason: 'refused, so undone');
      expect(o.get<Camera>().toString(), contains('orthographic, orthographicSize: 2'));
    });

    test('an orthographic camera renders to the same golden image as the Rust test', () {
      final scene = stage1(world);
      scene.cube.destroy();
      final view = world.camera!
        ..transform = Transform(position: const Vec3(0, 0, 10), rotation: Quat.lookRotation(Vec3.forward))
        ..get<Camera>()!.clearColor = Color.black;
      view.get<Camera>()!
        ..orthographic = true
        ..orthographicSize = 2;
      for (final (x, z) in [(-2.5, 0.0), (0.0, -5.0), (2.5, -20.0)]) {
        world.add(
          GameObject(
            transform: Transform(position: Vec3(x, 0, z)),
            components: [MeshRenderer(Mesh.cube(), scene.orange)],
          ),
        );
      }
      final headless = HeadlessView(world, width: 256, height: 128);
      addTearDown(headless.dispose);
      expectGolden('orthographic', headless.read(headless.frame()).rgba, 256, 128);
    });
  });

  group('between the screen and the world', () {
    test('world.camera is the one the view draws from', () {
      expect(world.camera, isNull);
      final a = camera();
      camera();
      expect(world.camera, a.get<Camera>());
      expect(world.camera!.gameObject, a);
    });

    test('screenPointToRay through an orthographic camera goes straight ahead from the point on the near plane', () {
      final o = world.add(
        GameObject(
          transform: Transform(position: const Vec3(1, 2, 3)),
          components: [Camera(orthographic: true, orthographicSize: 2, near: 0.5)],
        ),
      );
      // A 200 × 100 view shows 8 m by 4 m.
      final centre = o.get<Camera>()!.screenPointToRay(const ViewPoint(100, 50, width: 200, height: 100));
      expectVec(centre.origin, const Vec3(1, 2, 2.5));
      expectVec(centre.direction, Vec3.forward);
      final corner = o.get<Camera>()!.screenPointToRay(const ViewPoint(0, 0, width: 200, height: 100));
      expectVec(corner.origin, const Vec3(-3, 4, 2.5));
      expectVec(corner.direction, Vec3.forward);
    });

    test('worldToScreenPoint finds a point in the view, the other way from screenPointToRay', () {
      // 90° up and down, a 200 × 100 view: 1 m ahead the view is 4 m across.
      final o = camera(lens: Camera(fovY: 90.deg));
      expectPoint(o.get<Camera>()!.worldToScreenPoint(const Vec3(0, 0, -1), width: 200, height: 100), 100, 50);
      expectPoint(o.get<Camera>()!.worldToScreenPoint(const Vec3(1, 0, -1), width: 200, height: 100), 150, 50);
      expectPoint(o.get<Camera>()!.worldToScreenPoint(const Vec3(0, 2, -2), width: 200, height: 100), 100, 0);
      expect(
        o.get<Camera>()!.worldToScreenPoint(const Vec3(0, 0, 1), width: 200, height: 100),
        isNull,
        reason: 'behind',
      );
      expect(
        () => o.get<Camera>()!.worldToScreenPoint(Vec3.zero),
        throwsArgumentError,
        reason: 'the input has no view size yet',
      );
      world.input.simulate.resize(200, 100);
      expectPoint(o.get<Camera>()!.worldToScreenPoint(const Vec3(1, 0, -1)), 150, 50);

      // Round trips, turned and orthographic too.
      o.transform = Transform(
        position: const Vec3(3, 1, -2),
        rotation: Quat.lookRotation(const Vec3(0, 0, -9) - const Vec3(3, 1, -2)),
      );
      for (final orthographic in [false, true]) {
        o.get<Camera>()!.orthographic = orthographic;
        for (final p in [
          const ViewPoint(10, 20, width: 200, height: 100),
          const ViewPoint(190, 77, width: 200, height: 100),
        ]) {
          final ray = o.get<Camera>()!.screenPointToRay(p);
          expectPoint(o.get<Camera>()!.worldToScreenPoint(ray.at(7)), p.x, p.y, 1e-4);
        }
      }
    });

    test('screenPointToPlane finds the spot on the ground under a point', () {
      final o = world.add(
        GameObject(
          transform: Transform(position: const Vec3(0, 10, 10), rotation: Quat.lookRotation(Vec3(0, -10, -10))),
          components: [Camera()],
        ),
      );
      expectVec(
        o.get<Camera>()!.screenPointToPlane(const ViewPoint(50, 50, width: 100, height: 100), Plane.ground)!,
        Vec3.zero,
        1e-5,
      );
      // Looking down 45°: the view's top edge (30° above the middle) is
      // 15° below level, so it meets the ground far off.
      final top = o.get<Camera>()!.screenPointToPlane(const ViewPoint(50, 0, width: 100, height: 100), Plane.ground)!;
      expect(top.z, closeTo(10 - 10 / math.tan(15 * math.pi / 180), 1e-4));
      o.transform = Transform(position: const Vec3(0, 10, 0), rotation: Quat.lookRotation(Vec3.right));
      expect(
        o.get<Camera>()!.screenPointToPlane(const ViewPoint(50, 50, width: 100, height: 100), Plane.ground),
        isNull,
        reason: 'level',
      );
      expect(
        o.get<Camera>()!.screenPointToPlane(const ViewPoint(50, 0, width: 100, height: 100), Plane.ground),
        isNull,
        reason: 'up',
      );
    });

    test('Plane', () {
      final p = Plane(const Vec3(0, 2, 0), const Vec3(5, 3, 1));
      expect((p.normal, p.distance), (Vec3.up, -3.0), reason: "the origin is 3 below it, as Unity's");
      expect(p.distanceTo(const Vec3(0, 5, 0)), 2);
      expect(p.raycast(Ray(const Vec3(0, 5, 0), Vec3.down)), 2);
      expect(p.raycast(Ray(const Vec3(0, 5, 0), Vec3.up)), isNull);
      expect(Plane.ground, Plane(Vec3.up, Vec3.zero));
    });
  });

  group('virtual cameras', () {
    GameObject at(Vec3 p, {int priority = 0, double? fovY, Iterable<Component> more = const []}) => world.add(
      GameObject(
        transform: Transform(position: p),
        components: [
          VirtualCamera(priority: priority, fovY: fovY ?? 60.deg),
          ...more,
        ],
      ),
    );

    test('the enabled one with the highest priority drives the view, newest first among equals', () {
      final view = camera();
      world.cameras.defaultBlend = Blend.cut;
      final a = at(const Vec3(1, 0, 0), priority: 5);
      final b = at(const Vec3(2, 0, 0), priority: 1);
      frames(1);
      expect(view.position, const Vec3(1, 0, 0));
      expect(world.cameras.live, a.get<VirtualCamera>());
      expect(a.get<VirtualCamera>()!.isLive, isTrue);

      b.get<VirtualCamera>()!.priority = 9;
      frames(1);
      expect(view.position, const Vec3(2, 0, 0));
      a.get<VirtualCamera>()!.priority = 9;
      frames(1);
      expect(view.position, const Vec3(1, 0, 0), reason: 'equal: the one given its priority last');

      a.active = false;
      frames(1);
      expect(view.position, const Vec3(2, 0, 0));
      b.get<VirtualCamera>()!.enabled = false;
      frames(1);
      expect(world.cameras.live, isNull);
      expect(view.position, const Vec3(2, 0, 0), reason: 'with none it stays where it was');
      view.position = const Vec3(7, 7, 7);
      frames(1);
      expect(view.position, const Vec3(7, 7, 7), reason: 'and is the game\'s again');
    });

    test('its lens: the field of view, or the size of an orthographic camera', () {
      final view = camera();
      at(Vec3.zero, fovY: 30.deg).get<VirtualCamera>()!.orthographicSize = 3;
      frames(1);
      expect(view.get<Camera>()!.fovY, 30.deg);
      view.get<Camera>()!.orthographic = true;
      frames(1);
      expect(view.get<Camera>()!.orthographicSize, 3);
    });

    test('going to another blends over the seconds with the ease, position, turn and lens together', () {
      final view = camera();
      world.cameras.defaultBlend = const Blend(1, ease: Ease.linear);
      at(Vec3.zero, priority: 1, fovY: 40.deg);
      frames(1);
      final b = at(const Vec3(4, 0, 0), fovY: 80.deg)..rotation = Quat.axisAngle(Vec3.up, 1);
      b.get<VirtualCamera>()!.priority = 2;
      frames(1);
      expect(world.cameras.isBlending, isTrue);
      expectVec(view.position, const Vec3(1, 0, 0));
      expect(view.get<Camera>()!.fovY, closeTo(50.deg, 1e-9));
      expect(view.rotation.closeTo(Quat.axisAngle(Vec3.up, 0.25)), isTrue);
      frames(2);
      expectVec(view.position, const Vec3(3, 0, 0));
      frames(1);
      expect(world.cameras.isBlending, isFalse);
      expect(view.position, const Vec3(4, 0, 0));
      expect(view.get<Camera>()!.fovY, 80.deg);
    });

    test('the blend follows the cameras as they move', () {
      final view = camera();
      world.cameras.defaultBlend = const Blend(1, ease: Ease.linear);
      final a = at(Vec3.zero, priority: 1);
      frames(1);
      final b = at(const Vec3(4, 0, 0), priority: 2);
      frames(2);
      expectVec(view.position, const Vec3(2, 0, 0));
      a.position = const Vec3(0, 4, 0);
      b.position = const Vec3(4, 4, 0);
      frames(1);
      expectVec(view.position, const Vec3(3, 4, 0));
    });

    test('a blend cut short blends on from where it got to', () {
      final view = camera();
      world.cameras.defaultBlend = const Blend(1, ease: Ease.linear);
      final a = at(Vec3.zero, priority: 1);
      frames(1);
      final b = at(const Vec3(4, 0, 0), priority: 2);
      frames(2);
      expectVec(view.position, const Vec3(2, 0, 0));
      b.get<VirtualCamera>()!.priority = 0;
      frames(1);
      // From (2, 0, 0), held, back to a: a quarter of the way.
      expectVec(view.position, const Vec3(1.5, 0, 0));
      a.position = const Vec3(0, 4, 0);
      frames(1);
      expectVec(view.position, const Vec3(1, 2, 0));
    });

    test('blends per pair: both, then to, then from, then the default', () {
      world.cameras.defaultBlend = const Blend(9);
      final a = at(Vec3.zero, priority: 1).get<VirtualCamera>()!;
      final b = at(Vec3.zero).get<VirtualCamera>()!;
      final c = at(Vec3.zero).get<VirtualCamera>()!;
      world.cameras
        ..setBlend(const Blend(1), from: a)
        ..setBlend(const Blend(2), to: b)
        ..setBlend(const Blend(3), from: a, to: b)
        ..setBlend(Blend.cut, from: c, to: a);
      camera();
      final seen = <VirtualCamera?>[];
      world.cameras.onLiveChanged.listen(seen.add);
      frames(1);
      expect(seen, [a], reason: 'the first one cuts');
      // Whether going to [next] blends; then it settles.
      bool blends(VirtualCamera next, VirtualCamera last) {
        next.priority = last.priority + 1;
        frames(1);
        final blending = world.cameras.isBlending;
        frames(40);
        return blending;
      }

      expect(blends(b, a), isTrue);
      expect(blends(c, b), isTrue, reason: 'the default');
      expect(blends(a, c), isFalse, reason: 'c to a cuts');
      expect(seen, [a, b, c, a]);
    });

    test('a blend to a camera beats one from the camera before', () {
      final a = at(Vec3.zero, priority: 1).get<VirtualCamera>()!;
      final b = at(const Vec3(3, 0, 0)).get<VirtualCamera>()!;
      world.cameras
        ..setBlend(Blend.cut, from: a)
        ..setBlend(const Blend(2), to: b);
      camera();
      frames(1);
      b.priority = 2;
      frames(1);
      expect(world.cameras.isBlending, isTrue);
    });

    test('one on the view\'s camera itself leaves it, and its lens, alone', () {
      final view = world.add(
        GameObject(
          components: [
            Camera(fovY: 40.deg),
            VirtualCamera(fovY: 70.deg),
          ],
        ),
      )..position = const Vec3(1, 2, 3);
      frames(1);
      expect(world.cameras.live, view.get<VirtualCamera>());
      expect(view.position, const Vec3(1, 2, 3));
      expect(view.get<Camera>()!.fovY, 40.deg);
    });

    test('a blend measures 3 s for a to b, from the pair', () {
      final view = camera();
      final a = at(Vec3.zero, priority: 1).get<VirtualCamera>()!;
      final b = at(const Vec3(3, 0, 0)).get<VirtualCamera>()!;
      world.cameras
        ..setBlend(const Blend(1, ease: Ease.linear), from: a)
        ..setBlend(
          const Blend(3, ease: Ease.linear),
          from: a,
          to: b,
        );
      frames(1);
      b.priority = 2;
      frames(4);
      expectVec(view.position, const Vec3(1, 0, 0));
    });

    test('a paused game holds the blend, unless the cameras are unscaled', () {
      final view = camera();
      world.cameras.defaultBlend = const Blend(1, ease: Ease.linear);
      at(Vec3.zero, priority: 1);
      frames(1);
      at(const Vec3(4, 0, 0), priority: 2);
      world.time.scale = 0;
      frames(2);
      expectVec(view.position, Vec3.zero);
      world.cameras.unscaled = true;
      frames(2);
      expectVec(view.position, const Vec3(2, 0, 0));
    });
  });

  group('rigs', () {
    late GameObject target;
    setUp(() => target = world.add(GameObject(transform: Transform(position: const Vec3(1, 0, 0)))));

    test('Follow keeps its offset from the target, in the world, and doesn\'t turn', () {
      final rig = world.add(
        GameObject(
          transform: Transform(rotation: Quat.axisAngle(Vec3.up, 0.5)),
          components: [Follow(target, offset: const Vec3(0, 2, 5))],
        ),
      );
      frames(1);
      expect(rig.position, const Vec3(1, 2, 5));
      target.position = const Vec3(3, 0, 0);
      frames(1);
      expect(rig.position, const Vec3(3, 2, 5));
      expect(rig.rotation, Quat.axisAngle(Vec3.up, 0.5));
    });

    test("Follow in the target's space turns with it; in its heading, stays level", () {
      Vec3 at(FollowSpace space) {
        final rig = world.add(
          GameObject(
            components: [Follow(target, offset: const Vec3(0, 2, 5), space: space)],
          ),
        );
        frames(1);
        final p = rig.position;
        rig.destroy();
        return p;
      }

      void near(Vec3 a, Vec3 b, {String? reason}) =>
          expect((a - b).length, lessThan(1e-6), reason: reason ?? '$a is not $b');
      target.rotation = Quat.axisAngle(Vec3.up, math.pi / 2);
      near(at(FollowSpace.world), const Vec3(1, 2, 5));
      near(at(FollowSpace.target), const Vec3(6, 2, 0), reason: 'behind it, which faces -X now');
      near(at(FollowSpace.targetYaw), const Vec3(6, 2, 0));
      target.rotation = Quat.axisAngle(Vec3.up, math.pi / 2) * Quat.axisAngle(const Vec3(1, 0, 0), -0.5);
      near(at(FollowSpace.targetYaw), const Vec3(6, 2, 0), reason: 'tipped forward, the heading is the same');
      expect(at(FollowSpace.target).y, isNot(closeTo(2, 0.1)), reason: 'all the way: it tips too');
    });

    test('Follow with damping closes 99 % of the gap in its seconds', () {
      final follow = Follow(target, offset: Vec3.zero, damping: 1);
      final rig = world.add(GameObject(components: [follow]));
      frames(1);
      expect(rig.position, const Vec3(1, 0, 0), reason: 'it starts right there');
      target.position = const Vec3(101, 0, 0);
      frames(4);
      expect(rig.position.x, closeTo(100, 1e-9));
      frames(1);
      // A quarter of a second more: 1 − 0.01^0.25 of the last metre.
      expect(rig.position.x, closeTo(100 + 1 - math.pow(0.01, 0.25), 1e-9));
    });

    test('Follow looks ahead by where the target would be at its speed', () {
      final rig = world.add(GameObject(components: [Follow(target, offset: Vec3.zero, lookAhead: 0.5)]));
      frames(1);
      target.position += const Vec3(1, 0, 0);
      frames(1);
      // 1 m in a quarter of a second: 4 m/s, so 2 m ahead of (2, 0, 0).
      expect(rig.position, const Vec3(4, 0, 0));
    });

    test('LookAt turns to the target, from where a Follow on the same object put it this frame', () {
      final rig = world.add(
        GameObject(
          components: [
            LookAt(target, offset: const Vec3(0, 0, -1)),
            Follow(target, offset: const Vec3(0, 0, 5)),
          ],
        ),
      );
      frames(1);
      expect(rig.position, const Vec3(1, 0, 5));
      expectVec(rig.forward, Vec3.forward);
      target.position = const Vec3(1, 6, 0);
      frames(1);
      expectVec(rig.forward, Vec3.forward, 1e-6);
    });

    test('LookAt with damping turns 99 % of the way in its seconds', () {
      final rig = world.add(GameObject(components: [LookAt(target, damping: 1)]));
      target.position = const Vec3(0, 0, -1);
      frames(1);
      expectVec(rig.forward, Vec3.forward);
      target.position = const Vec3(1, 0, 0);
      frames(4);
      // A quarter turn, 99 % of it: π/2 × 0.99 to the right.
      final turned = Quat.axisAngle(Vec3.up, -math.pi / 2 * 0.99).rotate(Vec3.forward);
      expectVec(rig.forward, turned, 1e-6);
    });

    test('rigs run after the late updates, so they see where everything went', () {
      target.addComponent(MoveLate());
      final rig = world.add(GameObject(components: [Follow(target, offset: Vec3.zero)]));
      frames(1);
      expect(rig.position, target.position);
      expect(rig.position, const Vec3(2, 0, 0));
    });

    test('a disabled rig rests; a destroyed target leaves it where it is', () {
      final follow = Follow(target, offset: Vec3.zero);
      final rig = world.add(GameObject(components: [follow]));
      frames(1);
      follow.enabled = false;
      target.position = const Vec3(5, 0, 0);
      frames(1);
      expect(rig.position, const Vec3(1, 0, 0));
      final errors = [];
      world.errors.listen(errors.add);
      follow.enabled = true;
      target.destroy();
      frames(2);
      expect(rig.position, const Vec3(1, 0, 0));
      expect(errors, isEmpty);
    });

    group('Orbit', () {
      test('circles the target at its yaw, pitch and distance, looking at it, within its limits', () {
        final orbit = Orbit(target, distance: 4, yaw: math.pi / 2, pitch: 0, controls: false);
        final rig = world.add(GameObject(components: [orbit]));
        frames(1);
        expectVec(rig.position, const Vec3(5, 0, 0));
        expectVec(rig.forward, Vec3.left);
        orbit
          ..yaw = 0
          ..pitch = math.pi / 6;
        frames(1);
        expectVec(rig.position, Vec3(1, 2, 4 * math.cos(math.pi / 6)));
        expectVec(rig.forward, (target.position - rig.position).normalized);
        orbit
          ..pitch = 3
          ..distance = 1000;
        frames(1);
        expect((orbit.pitch, orbit.distance), (1.45, 100.0));
      });

      test('dragging turns it, pinching and the mouse wheel zoom it, and a drag an object took doesn\'t', () {
        world.input.simulate.resize(200, 200);
        camera();
        world.camera!.transform = Transform(position: const Vec3(0, 0, 10));
        world.add(
          Grab()
            ..transform = Transform(position: const Vec3(-3, 0, 0))
            ..addComponent(MeshRenderer(Mesh.cube(), Material())),
        );
        final orbit = Orbit(target, yaw: 0, pitch: 0, distance: 8);
        world.add(GameObject(components: [orbit]));
        frames(1);
        final input = world.input;
        input.simulate.fingerDown(1, 150, 100);
        frames(1);
        input.simulate.fingerMove(1, 180, 120);
        frames(1);
        input.simulate.fingerMove(1, 210, 140);
        frames(1);
        input.simulate.fingerUp(1, 210, 140);
        frames(1);
        expect(orbit.yaw, closeTo(-0.6, 1e-9));
        expect(orbit.pitch, closeTo(0.4, 1e-9));

        input.simulate.trackpad(scale: 2);
        frames(1);
        expect(orbit.distance, closeTo(4, 1e-9));
        input.simulate.scrollBy(0, 500);
        frames(1);
        expect(orbit.distance, closeTo(4 * math.e, 1e-9));

        // Over the grabbed cube, at x = −3: the cube takes the drag.
        final over = world.camera!.worldToScreenPoint(const Vec3(-3, 0, 0))!;
        input.simulate.fingerDown(2, over.x, over.y);
        frames(1);
        input.simulate.fingerMove(2, over.x + 60, over.y);
        frames(1);
        input.simulate.fingerUp(2, over.x + 60, over.y);
        frames(1);
        expect(orbit.yaw, closeTo(-0.6, 1e-9));

        orbit.controls = false;
        input.simulate.scrollBy(0, 500);
        frames(1);
        expect(orbit.distance, closeTo(4 * math.e, 1e-9));
      });
    });
  });

  group('shake', () {
    test('stays within its strength across the view, and ends where it began', () {
      final view = camera();
      view.transform = Transform(position: const Vec3(1, 2, 3), rotation: Quat.lookRotation(Vec3(-1, -2, -3)));
      final start = view.transform;
      world.cameras.shake(0.5, seconds: 1, seed: 1);
      final offsets = <Vec3>[];
      for (var i = 0; i < 59; i++) {
        world.update(1 / 60);
        offsets.add(view.position - start.position);
      }
      expect(world.cameras.isShaking, isTrue);
      expect(offsets.map((o) => o.length).reduce(math.max), inInclusiveRange(0.05, 0.5));
      for (final (i, o) in offsets.indexed) {
        expect(o.dot(view.forward).abs(), lessThan(1e-6), reason: 'across the view, not into it');
        final left = 1 - (i + 1) / 60;
        expect(o.length, lessThanOrEqualTo(0.5 * left * left + 1e-9), reason: 'fading as (1 - p)²');
      }
      world.update(1 / 60);
      expect(world.cameras.isShaking, isFalse);
      expect(view.transform, start);
    });

    test('the same seed shakes the same way', () {
      List<Vec3> run(int seed) {
        final w = World();
        final v = w.add(GameObject(components: [Camera()]));
        w.cameras.shake(1, seed: seed);
        final seen = <Vec3>[];
        for (var i = 0; i < 10; i++) {
          w.update(1 / 60);
          seen.add(v.position);
        }
        w.dispose();
        return seen;
      }

      expect(run(4), run(4));
      expect(run(4), isNot(run(5)));
    });

    test('on top of a virtual camera, and of the game moving the camera', () {
      final view = camera();
      final v = world.add(
        GameObject(
          transform: Transform(position: const Vec3(0, 0, 9)),
          components: [VirtualCamera()],
        ),
      );
      world.cameras.shake(1, seconds: 1);
      frames(1);
      expect(view.position, isNot(const Vec3(0, 0, 9)));
      v.position = const Vec3(0, 0, 4);
      frames(3);
      expect(view.position, const Vec3(0, 0, 4), reason: 'it ends where the virtual camera is');

      v.get<VirtualCamera>()!.enabled = false;
      frames(1);
      world.cameras.shake(1, seconds: 1);
      frames(1);
      view.position = const Vec3(5, 0, 0);
      frames(3);
      expect(view.position, const Vec3(5, 0, 0), reason: 'and where the game put it');
    });

    test('stops with stopShaking; holds while paused unless unscaled', () {
      final view = camera();
      world.time.scale = 0;
      world.cameras.shake(1, seconds: 0.5);
      frames(4);
      expect(world.cameras.isShaking, isTrue);
      expect(view.position, isNot(Vec3.zero), reason: 'held where it was');
      world.cameras.stopShaking();
      frames(1);
      expect(view.position, Vec3.zero);
      world.cameras.shake(1, seconds: 0.5, unscaled: true);
      frames(2);
      expect(world.cameras.isShaking, isFalse);
      expect(view.position, Vec3.zero);
    });
  });
}
