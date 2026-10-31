// Stage 14's pass over the public API, package:fynkel's features (F1–F18,
// docs/stage-14-audit/api/features.md): bugs, errors that didn't say what
// was wrong, and the everyday helpers that were missing.

import 'dart:async';
import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'model_test.dart' show glb;

/// Throws an [ArgumentError] whose message has each of [parts], and never
/// "Instance of", which hides the value given.
Matcher refused(List<String> parts) => throwsA(
  isA<ArgumentError>().having(
    (e) => '${e.message}',
    'message',
    allOf([for (final p in parts) contains(p), isNot(contains('Instance of'))]),
  ),
);

/// What [world] reports while [frames] frames run.
Future<List<ObjectError>> reported(World world, int frames) async {
  final errors = <ObjectError>[];
  final listening = world.errors.listen(errors.add);
  for (var i = 0; i < frames; i++) {
    world.update(0.1);
  }
  // world.errors is a broadcast stream: what it got arrives a microtask on.
  await Future<void>.delayed(Duration.zero);
  await listening.cancel();
  return errors;
}

void main() {
  test('F1: a 2D shape of no size is refused where it is made, and extruding one is an error, not a panic', () {
    expect(() => Shape2D.circle(0), refused(['Shape2D.circle', 'radius is 0']));
    expect(() => Shape2D.circle(double.nan), refused(['radius is NaN']));
    expect(() => Shape2D.rect(0, 1), refused(['Shape2D.rect', 'width is 0']));
    expect(() => Shape2D.rect(1, double.infinity), refused(['height is Infinity']));
    expect(() => Shape2D.rounded(1, -1, 0.1), refused(['Shape2D.rounded', 'height is -1']));
    expect(() => Shape2D.rounded(1, 1, -0.1), refused(['radius is -0.1']));
    // Points all in one place: a shape with no size, however it was made.
    expect(() => Mesh.extrude(Shape2D([Vec2.one, Vec2.one, Vec2.one])), refused(['no size']));
  });

  group('F2 and F13: virtual cameras and shakes', () {
    test('a VirtualCamera refuses a field of view or size the camera would, where it is given', () {
      expect(() => VirtualCamera(fovY: 60), refused(['VirtualCamera.fovY is 60', '60.deg']));
      final vc = VirtualCamera();
      expect(() => vc.fovY = 0, refused(['VirtualCamera.fovY is 0']));
      expect(() => vc.fovY = double.nan, refused(['VirtualCamera.fovY is NaN']));
      expect(() => VirtualCamera(orthographicSize: 0), refused(['VirtualCamera.orthographicSize is 0']));
      expect(() => vc.orthographicSize = double.infinity, refused(['orthographicSize is Infinity']));
      expect(vc.fovY, 60.deg, reason: 'unchanged');
    });

    test('both shakes refuse a strength, time or frequency that isn\'t a number they can use', () {
      final world = World();
      addTearDown(world.dispose);
      final o = world.add(GameObject());
      expect(() => world.cameras.shake(double.nan), refused(['strength is NaN']));
      expect(() => world.cameras.shake(-1), refused(['strength is -1']));
      expect(() => world.cameras.shake(1, seconds: 0), refused(['seconds is 0']));
      expect(() => world.cameras.shake(1, frequency: double.infinity), refused(['frequency is Infinity']));
      expect(() => o.shake(double.nan), refused(['strength is NaN']));
      expect(() => o.shake(1, seconds: 0), refused(['seconds is 0']));
      expect(() => o.shake(1, frequency: -2), refused(['frequency is -2']));
      expect(world.cameras.isShaking, isFalse);
    });

    test('both shakes last 0.4 s unless told', () {
      final world = World();
      addTearDown(world.dispose);
      world.add(GameObject(components: [Camera()]));
      final o = world.add(GameObject());
      world.cameras.shake(0.5);
      var done = false;
      o.shake(0.5, onComplete: () => done = true);
      for (var i = 0; i < 3; i++) {
        world.update(0.1);
      }
      expect((world.cameras.isShaking, done), (true, false));
      for (var i = 0; i < 3; i++) {
        world.update(0.1);
      }
      expect((world.cameras.isShaking, done), (false, true));
    });

    test('a virtual camera that can\'t be followed is reported on its object, and the cameras go on', () async {
      final world = World();
      addTearDown(world.dispose);
      // The view's camera under a parent of no size: no position of its
      // own puts it where the virtual camera is.
      world.add(
        GameObject(
          transform: const Transform(scale: Vec3.zero),
          children: [
            GameObject(components: [Camera()]),
          ],
        ),
      );
      final there = world.add(
        GameObject(
          transform: Transform(position: const Vec3(1, 2, 3)),
          components: [VirtualCamera()],
        ),
      );
      final errors = await reported(world, 8);
      expect(errors, hasLength(8), reason: 'one each frame, never turned off: ${errors.map((e) => e.error)}');
      expect(errors.map((e) => e.gameObject).toSet(), {there});
      expect('${errors.last.error}', allOf(contains('VirtualCamera'), isNot(contains('_CameraWorld'))));
    });
  });

  test('F3: SplineAnimate holds still once its spline\'s object is destroyed', () async {
    final world = World();
    addTearDown(world.dispose);
    final track = world.add(
      GameObject(
        components: [
          SplineContainer([
            Spline.smooth([Vec3.zero, const Vec3(10, 0, 0)]),
          ]),
        ],
      ),
    );
    final cart = world.add(GameObject(components: [SplineAnimate(track.get<SplineContainer>()!, speed: 1)]));
    world.update(0.1);
    world.update(0.1);
    track.destroy();
    final at = cart.position;
    expect(await reported(world, 3), isEmpty);
    expect(cart.position, at);
  });

  test(
    'F4 and F9: a blend\'s name is the animator\'s current, and it says what it is where a clip is wanted',
    () async {
      final world = World();
      final fox = await Model.load(glb('Fox.glb'));
      addTearDown(() {
        world.dispose();
        fox.dispose();
      });
      final a = world.add(fox.instantiate()).animator!;
      a.blend('move', const ['Walk', 'Run'], at: 0.5, seconds: 0);
      world.update(0.1);
      expect(a.current, 'move');
      expect(a.isPlaying(a.current!), isTrue);
      expect(a.weight('move'), closeTo(1, 1e-6));
      expect(a.weight('Survey'), 0);
      expect(() => a.crossFade('move'), refused(["'move' is a blend", 'blend(']));
      expect(() => a.play('move'), refused(["'move' is a blend"]));
      expect(() => a.weight('Swim'), refused(["'Swim'", 'Survey, Walk, Run']));
      expect(() => a.crossFade('Run', seconds: double.infinity), refused(['seconds is Infinity']));
      expect(() => a.stop(seconds: -1), refused(['seconds is -1']));
      a.crossFade('Run', seconds: 0.2);
      a.stop(seconds: 0.1);
    },
  );

  test('F5: what was given is shown in the message, not "Instance of"', () {
    const source = '''
struct Values { count: i32, tint: vec3<f32>, pts: array<vec2<f32>, 4> }
fn surface(in: SurfaceInput) -> Surface { return standard(in); }
''';
    final shader = Shader(source, name: 'probe.wgsl');
    final m = Material(shader: shader);
    expect(() => m.set('tint', const Vec2(1, 1)), refused(['tint is Vec2(1.0, 1.0)', 'vec3<f32>']));
    expect(() => m.set('count', 1.5), refused(['count is 1.5']));
    expect(() => m.set('pts', List.filled(5, const Vec2(1, 1))), refused(['pts is [Vec2(1.0, 1.0)', 'up to 4']));
    expect(() => Material(values: {'a': 1.0}), refused(['{a: 1.0}']));
    final effect = Shader('fn effect(in: EffectInput) -> vec4<f32> { return picture(in.uv); }', name: 'fx.wgsl');
    expect(() => Material(shader: effect), refused(['fx.wgsl', 'ScreenEffect']));
    expect(() => ScreenEffect(shader), refused(['probe.wgsl', 'effect']));
    expect(() => Mesh.combine([]), refused(['Mesh.combine', 'parts is empty']));
    expect(() => Spline([const Knot(Vec3(double.nan, 0, 0))]), refused(['position is Vec3(NaN, 0.0, 0.0)']));
    expect(
      () => SplineInstantiate((i) => GameObject(), offset: const Vec3(double.nan, 0, 0)),
      refused(['offset is Vec3(NaN, 0.0, 0.0)']),
    );
    final world = World();
    addTearDown(world.dispose);
    final o = world.add(GameObject());
    expect(() => o.together(const []), refused(['parts is empty']));
  });

  test('F6 and F15: a particle setting that\'s wrong is named, with its value', () {
    final world = World();
    addTearDown(world.dispose);
    final ps = ParticleSystem();
    world.add(GameObject(components: [ps]));
    expect(() => ps.simulationSpeed = -1, refused(['simulationSpeed is -1']));
    expect(() => ps.startDelay = -1, refused(['startDelay is -1']));
    expect(() => ps.emission = const Emission(rateOverTime: -1), refused(['rateOverTime is -1']));
    expect(
      () => ps.emission = const Emission(bursts: [Burst(probability: 2)]),
      refused(['burst 0', 'probability is 2']),
    );
    expect(
      () => ps.emission = const Emission(bursts: [Burst(), Burst(cycles: 0, interval: 0)]),
      refused(['burst 1', 'interval is 0']),
    );
    expect(() => ps.emission = const Emission(bursts: [Burst(time: -1)]), refused(['time is -1']));
    expect(() => ps.emission = const Emission(bursts: [Burst(count: Range(-3))]), refused(['count is Range(-3']));
    expect(() => ps.shape = const ParticleShape.cone(angle: 25), refused(['angle is 25', '25.deg']));
    expect(() => ps.startRotation = const Range(double.nan), refused(['startRotation', 'Range(NaN']));
    expect(() => ps.startLifetime = const Range(-1), refused(['startLifetime is Range(-1']));
    expect(() => ps.startSize = const Range(-1), refused(['startSize is Range(-1']));
    expect(() => ps.startSpeed = const Range(5, 1), refused(['startSpeed is Range(5.0, 1.0)', 'Range(1.0, 5.0)']));
    // Unchanged by what it refused.
    expect((ps.simulationSpeed, ps.startSize, ps.startSpeed), (1.0, ps.startSize, ps.startSpeed));
    ps.startSpeed = const Range(1, 5);
    ps.startSize = const Range(0);
  });

  test('F7: Mesh.custom counts in the vectors it was given', () {
    expect(
      () => Mesh.custom(
        positions: [Vec3.zero, Vec3.right, Vec3.up, Vec3.one],
        uvs: [Vec2.zero, Vec2.one, Vec2.zero],
        indices: [0, 1, 2],
      ),
      refused(['uvs has 3', 'one for each of the 4 positions']),
    );
    expect(
      () => Mesh.custom(positions: [Vec3.zero, Vec3.right, Vec3.up], normals: [Vec3.up], indices: [0, 1, 2]),
      refused(['normals has 1', 'one for each of the 3 positions']),
    );
  });

  test('F8: a direction that is zero is named for what it was given as', () {
    expect(() => Ray(Vec3.zero, Vec3.zero), refused(['Ray', 'direction is Vec3(0.0, 0.0, 0.0)']));
    expect(() => Plane(Vec3.zero, Vec3.one), refused(['Plane', 'normal is Vec3(0.0, 0.0, 0.0)']));
    expect(() => Quat.axisAngle(Vec3.zero, 1), refused(['Quat.axisAngle', 'axis is Vec3(0.0, 0.0, 0.0)']));
    final world = World();
    addTearDown(world.dispose);
    final o = world.add(GameObject());
    expect(() => o.rotateBy(Vec3.zero, 1, seconds: 1), refused(['axis is Vec3(0.0, 0.0, 0.0)']));
    expect(() => o.rotate(Vec3.zero, 1), refused(['axis is Vec3(0.0, 0.0, 0.0)']));
    expect(
      () => MeshBuilder().addMesh(Mesh.cube(), scale: const Vec3(1, 0, 1)),
      refused(['scale is Vec3(1.0, 0.0, 1.0)']),
    );
  });

  group('F12: everyday helpers', () {
    test('Color.withAlpha keeps the colour', () {
      expect(const Color.rgb(1, 0.5, 0.25).withAlpha(0.5), const Color.rgb(1, 0.5, 0.25, 0.5));
    });

    test('Vec2 normalizes as Vec3 does', () {
      expect(const Vec2(3, 4).normalized, const Vec2(0.6, 0.8));
      expect(() => Vec2.zero.normalized, refused(['Vec2(0.0, 0.0) has no direction']));
      expect(Vec2.zero.normalizedOrZero, Vec2.zero);
      expect(const Vec2(0, 2).normalizedOrZero, const Vec2(0, 1));
      expect(const Vec2(1, 2).closeTo(const Vec2(1, 2 + 1e-9)), isTrue);
      expect(const Vec2(1, 2).closeTo(const Vec2(1, 2.1)), isFalse);
    });

    test('eulerAngles reads back what Quat.euler made, in its order', () {
      final random = math.Random(3);
      for (var i = 0; i < 200; i++) {
        final (pitch, yaw, roll) = (
          (random.nextDouble() * 2 - 1) * 89.deg,
          (random.nextDouble() * 2 - 1) * 179.deg,
          (random.nextDouble() * 2 - 1) * 179.deg,
        );
        final back = Quat.euler(pitch, yaw, roll).eulerAngles;
        expect(back.closeTo(Vec3(pitch, yaw, roll), 1e-6), isTrue, reason: '$back for ($pitch, $yaw, $roll)');
      }
      expect(Quat.euler(0, 90.deg, 0).eulerAngles.closeTo(Vec3(0, 90.deg, 0), 1e-9), isTrue);
      // Straight up or down: the same rotation back, its roll in its yaw.
      for (final pitch in [90.deg, -90.deg]) {
        final q = Quat.euler(pitch, 0.3, 0.2);
        final e = q.eulerAngles;
        expect(e.x, closeTo(pitch, 1e-6));
        expect(Quat.euler(e.x, e.y, e.z).closeTo(q, 1e-6), isTrue, reason: '$e');
      }
    });

    test('fromTo turns one direction to another, and rotateTowards goes there by steps', () {
      final q = Quat.fromTo(Vec3.forward, Vec3.right);
      expect(q.rotate(Vec3.forward).closeTo(Vec3.right, 1e-9), isTrue);
      expect(Quat.fromTo(Vec3.up, Vec3.up), Quat.identity);
      final back = Quat.fromTo(Vec3.up, Vec3.down);
      expect(back.rotate(Vec3.up).closeTo(Vec3.down, 1e-9), isTrue);
      expect(() => Quat.fromTo(Vec3.zero, Vec3.up), refused(['Quat.fromTo', 'from is Vec3(0.0, 0.0, 0.0)']));

      final to = Quat.euler(0, 90.deg, 0);
      final step = Quat.rotateTowards(Quat.identity, to, 30.deg);
      expect(step.angleTo(Quat.identity), closeTo(30.deg, 1e-9));
      expect(step.angleTo(to), closeTo(60.deg, 1e-9));
      expect(Quat.rotateTowards(step, to, 90.deg), to);
    });
  });

  test('F14: Mesh.lathe calls its slices segments, as the other round meshes do', () {
    final profile = [const Vec2(0.5, 0), const Vec2(0.5, 1)];
    expect(Mesh.lathe(profile, segments: 8).positions, isNotEmpty);
    expect(() => Mesh.lathe(profile, segments: 2), refused(['segments is 2']));
  });

  test('F16: fadeInOut names its numbers, and says when they overlap', () {
    final g = ColorGradient.fadeInOut(Color.white, fadeIn: 0.1, fadeOut: 0.3);
    expect(g.stops.map((s) => s.$1), [0, 0.1, 0.7, 1]);
    expect(
      () => ColorGradient.fadeInOut(Color.white, fadeIn: 0.6, fadeOut: 0.6),
      refused(['fadeIn + fadeOut is 0.6 + 0.6']),
    );
    expect(() => ColorGradient.fadeInOut(Color.white, fadeIn: -0.1), refused(['fadeIn is -0.1']));
  });

  test('F17: a distance along a spline must be a number', () {
    final s = Spline.smooth([Vec3.zero, const Vec3(10, 0, 0)]);
    expect(() => s.positionAt(double.nan), refused(['distance is NaN']));
    expect(() => s.rotationAt(double.infinity), refused(['distance is Infinity']));
    final world = World();
    addTearDown(world.dispose);
    final c = SplineContainer([s]);
    world.add(GameObject(components: [c]));
    expect(() => c.directionAt(double.nan), refused(['distance is NaN']));
  });

  test('F18: a lathe or sweep that makes no surface says why', () {
    expect(() => Mesh.lathe([Vec2.zero, const Vec2(0, 1)]), refused(['on the axis']));
    final path = Spline.smooth([Vec3.zero, const Vec3(5, 0, 0)]);
    expect(() => Mesh.sweep(path, Shape2D([Vec2.one, Vec2.one, Vec2.one])), refused(['no size']));
  });
}
