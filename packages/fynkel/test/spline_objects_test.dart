// Splines on objects (docs/stage-13.md phase 3): a container's splines in
// the world, copies placed along one, and an object riding one.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

Matcher near(Vec3 v, [double within = 1e-5]) =>
    predicate<Vec3>((p) => (p - v).length <= within, 'within $within of $v');

void main() {
  late World world;
  setUp(() => world = World());
  tearDown(() => world.dispose());

  test('a container\'s splines are in its object\'s space', () {
    final track = world.add(
      GameObject(
        transform: Transform(position: const Vec3(0, 1, 0), rotation: Quat.axisAngle(Vec3.up, math.pi / 2)),
        components: [
          SplineContainer([
            Spline.linear([Vec3.zero, const Vec3(10, 0, 0)]),
          ]),
        ],
      ),
    );
    final c = track.get<SplineContainer>()!;
    world.update(1 / 60);
    // +X turned a quarter round Y is -Z.
    expect(c.positionAt(4), near(const Vec3(0, 1, -4)));
    expect(c.directionAt(4), near(Vec3.forward));
    expect(c.upAt(4), near(Vec3.up));
    expect(c.rotationAt(4).rotate(Vec3.forward), near(Vec3.forward));
    final hit = c.nearest(const Vec3(2, 5, -6));
    expect(hit.distance, closeTo(6, 1e-4));
    expect(hit.position, near(const Vec3(0, 1, -6), 1e-4));
    expect(() => c.positionAt(0, index: 1), throwsRangeError);
    expect(() => SplineContainer().spline, throwsStateError);
  });

  test('copies are placed a spacing apart, and again when the spline changes', () {
    final made = <int>[];
    final spline = Spline.linear([Vec3.zero, const Vec3(10, 0, 0)]);
    final fence = world.add(
      GameObject(
        components: [
          SplineContainer([spline]),
          SplineInstantiate((i) {
            made.add(i);
            return GameObject(name: 'post $i');
          }, spacing: 2.5),
        ],
      ),
    );
    world.update(1 / 60);
    final posts = fence.get<SplineInstantiate>()!;
    expect(posts.copies.length, 5);
    expect(made, [0, 1, 2, 3, 4]);
    expect([for (final p in posts.copies) p.position.x], [0, 2.5, 5, 7.5, 10]);
    expect(posts.copies.every((p) => identical(p.parent, fence)), isTrue);
    // Facing along it: -Z is +X.
    expect(posts.copies[1].rotation.rotate(Vec3.forward), near(Vec3.right));

    // Longer: more posts, the old ones kept and moved.
    final first = posts.copies.first;
    spline[1] = const Knot(Vec3(0, 0, 15), mode: TangentMode.linear);
    world.update(1 / 60);
    expect(posts.copies.length, 7);
    expect(identical(posts.copies.first, first), isTrue);
    expect(posts.copies.last.position, near(const Vec3(0, 0, 15)));

    // A count of them, end to end; fewer destroys the rest.
    final last = posts.copies.last;
    posts.count = 3;
    world.update(1 / 60);
    expect(posts.copies.length, 3);
    expect(last.isDestroyed, isTrue);
    expect(posts.copies[1].position, near(const Vec3(0, 0, 7.5)));
    expect(() => posts.spacing = 0, throwsArgumentError);
  });

  test('an offset and a closed spline', () {
    final ring = Spline.smooth([
      for (var i = 0; i < 8; i++) Vec3(4 * math.cos(i * math.pi / 4), 0, 4 * math.sin(i * math.pi / 4)),
    ], closed: true);
    final lamps = world.add(
      GameObject(
        components: [
          SplineContainer([ring]),
          SplineInstantiate((_) => GameObject(), count: 6, offset: const Vec3(0, 2, 0), align: false),
        ],
      ),
    );
    world.update(1 / 60);
    final copies = lamps.get<SplineInstantiate>()!.copies;
    expect(copies.length, 6, reason: 'none on top of the first at the end');
    for (final c in copies) {
      expect(c.position.y, closeTo(2, 1e-6));
      expect(c.rotation, Quat.identity);
    }
  });

  test('an object rides a spline at a speed, looping', () {
    final track = world.add(
      GameObject(
        components: [
          SplineContainer([
            Spline.linear([Vec3.zero, const Vec3(10, 0, 0)]),
          ]),
        ],
      ),
    );
    final c = track.get<SplineContainer>()!;
    final cart = world.add(GameObject(components: [SplineAnimate(c, speed: 4)]));
    final ride = cart.get<SplineAnimate>()!;
    // A metre each frame.
    world.update(0.25);
    expect(cart.position, near(Vec3.right));
    world.update(0.25);
    expect(cart.position, near(const Vec3(2, 0, 0)));
    expect(cart.rotation.rotate(Vec3.forward), near(Vec3.right));
    expect(ride.normalizedTime, closeTo(0.2, 1e-9));
    for (var i = 0; i < 10; i++) {
      world.update(0.25);
    }
    expect(cart.position, near(const Vec3(2, 0, 0)), reason: 'round again');
    ride.pause();
    world.update(0.25);
    expect(cart.position, near(const Vec3(2, 0, 0)));
    expect(ride.isPlaying, isFalse);
    ride.normalizedTime = 0.5;
    expect(cart.position, near(const Vec3(5, 0, 0)));
  });

  // Stage 14's DF6: a scaled container.
  test('along a scaled container, distances are metres in the world, and it faces the way it goes', () {
    // Scaled 2 evenly: a 10 m spline is 20 m long in the world.
    final track = world.add(
      GameObject(
        transform: Transform(scale: Vec3.all(2)),
        components: [
          SplineContainer([
            Spline.linear([Vec3.zero, const Vec3(10, 0, 0)]),
          ]),
        ],
      ),
    );
    final c = track.get<SplineContainer>()!;
    expect(c.length(), closeTo(20, 1e-9));
    expect(c.positionAt(5), near(const Vec3(5, 0, 0)));
    expect(c.nearest(const Vec3(6, 3, 0)).distance, closeTo(6, 1e-4));
    final cart = world.add(GameObject(components: [SplineAnimate(c, speed: 4)]));
    world.update(0.25);
    expect(cart.position, near(Vec3.right), reason: 'a metre a frame, as on one not scaled');
    expect(cart.get<SplineAnimate>()!.normalizedTime, closeTo(0.05, 1e-9));

    // Scaled unevenly: a diagonal leans over, and is longer.
    final leaning = world
        .add(
          GameObject(
            transform: Transform(scale: const Vec3(2, 1, 1)),
            components: [
              SplineContainer([
                Spline.linear([Vec3.zero, const Vec3(1, 0, -1), const Vec3(1, 0, -3)]),
              ]),
            ],
          ),
        )
        .get<SplineContainer>()!;
    final diagonal = math.sqrt(5);
    expect(leaning.length(), closeTo(diagonal + 2, 1e-9));
    expect(leaning.positionAt(diagonal / 2), near(const Vec3(1, 0, -0.5)));
    expect(leaning.positionAt(diagonal + 1), near(const Vec3(2, 0, -2)));
    expect(leaning.nearest(const Vec3(3, 0, -2)).distance, closeTo(diagonal + 1, 1e-4));
    final along = leaning.directionAt(1);
    expect(along, near(Vec3(2, 0, -1).normalized));
    expect(leaning.rotationAt(1).rotate(Vec3.forward), near(along));
    expect(leaning.rotationAt(1).rotate(Vec3.up), near(leaning.upAt(1)));
  });

  test('once stops at the end and says so; ping-pong comes back facing back', () {
    final c = world
        .add(
          GameObject(
            components: [
              SplineContainer([
                Spline.linear([Vec3.zero, const Vec3(10, 0, 0)]),
              ]),
            ],
          ),
        )
        .get<SplineContainer>()!;
    var done = 0;
    final once = world.add(
      GameObject(
        components: [SplineAnimate(c, seconds: 2, loop: SplineLoop.once, onComplete: () => done++)],
      ),
    );
    final bounce = world.add(GameObject(components: [SplineAnimate(c, seconds: 2, loop: SplineLoop.pingPong)]));
    expect(once.get<SplineAnimate>()!.speed, closeTo(5, 1e-9));
    for (var i = 0; i < 12; i++) {
      world.update(0.25);
    }
    expect(once.position, near(const Vec3(10, 0, 0)));
    expect(done, 1);
    expect(once.get<SplineAnimate>()!.isPlaying, isFalse);
    // 3 s at 5 m/s: to the end and 5 m back.
    expect(bounce.position, near(const Vec3(5, 0, 0)));
    expect(bounce.rotation.rotate(Vec3.forward), near(Vec3.left));
    expect(() => SplineAnimate(c), throwsArgumentError);
    expect(() => SplineAnimate(c, speed: 1, seconds: 1), throwsArgumentError);
  });
}
