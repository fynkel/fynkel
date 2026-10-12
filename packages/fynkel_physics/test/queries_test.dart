// Colliders and queries through the package (docs/stage-5.md, step 3):
// every query against each shape, with masks, triggers and scale, and
// colliders found where they were moved.

import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

const forward = Vec3(0, 0, 1);

World newWorld() {
  final world = World();
  addTearDown(world.dispose);
  return world;
}

GameObject at(World world, Vec3 position, Collider collider, {Vec3 scale = Vec3.one, String? name}) {
  final object = world.add(
    GameObject(
      name: name,
      transform: Transform(position: position, scale: scale),
      components: [collider],
    ),
  );
  world.update(1 / 60);
  return object;
}

/// A box 2 across at z 10, a sphere of radius 1 at x 5, and a capsule of
/// radius 1, 4 high, at x -5: each 9 from the origin's x along z.
({GameObject box, GameObject sphere, GameObject capsule}) three(World world) => (
  box: at(world, const Vec3(0, 0, 10), BoxCollider(size: const Vec3(2, 2, 2)), name: 'box'),
  sphere: at(world, const Vec3(5, 0, 10), SphereCollider(radius: 1), name: 'sphere'),
  capsule: at(world, const Vec3(-5, 0, 10), CapsuleCollider(radius: 1, height: 4), name: 'capsule'),
);

Matcher near(Vec3 v, [double tolerance = 1e-3]) =>
    predicate<Vec3>((got) => (got - v).length < tolerance, 'within $tolerance of $v');

void main() {
  test('every query finds each shape where it is', () {
    final world = newWorld();
    final shapes = three(world);
    final physics = world.physics;
    for (final (x, object) in [(0.0, shapes.box), (5.0, shapes.sphere), (-5.0, shapes.capsule)]) {
      final name = object.name;
      final ray = Ray(Vec3(x, 0, 0), forward);
      final hit = physics.raycast(ray)!;
      expect(hit.gameObject, same(object), reason: name);
      expect(hit.collider, same(object.get<Collider>()), reason: name);
      expect(hit.distance, closeTo(9, 1e-4), reason: name);
      expect(hit.point, near(Vec3(x, 0, 9)), reason: name);
      expect(hit.normal, near(const Vec3(0, 0, -1)), reason: name);
      expect(physics.raycastAll(ray).single.gameObject, same(object), reason: name);

      final casts = [
        physics.sphereCast(ray, 0.5),
        physics.boxCast(Vec3(x, 0, 0), const Vec3(0.5, 0.5, 0.5), forward),
        physics.capsuleCast(Vec3(x, -0.25, 0), Vec3(x, 0.25, 0), 0.5, forward),
      ];
      for (final (i, cast) in casts.indexed) {
        expect(cast?.gameObject, same(object), reason: '$name, cast $i');
        expect(cast!.distance, closeTo(8.5, 1e-3), reason: '$name, cast $i');
        expect(cast.normal, near(const Vec3(0, 0, -1), 1e-2), reason: '$name, cast $i');
        expect(cast.point.z, closeTo(9, 1e-3), reason: '$name, cast $i');
      }
      expect(physics.sphereCastAll(ray, 0.5).single.gameObject, same(object), reason: name);
      expect(physics.boxCastAll(Vec3(x, 0, 0), const Vec3(0.5, 0.5, 0.5), forward), hasLength(1), reason: name);
      expect(physics.capsuleCastAll(Vec3(x, -0.2, 0), Vec3(x, 0.2, 0), 0.5, forward), hasLength(1), reason: name);

      // Touching its near face, and just short of it.
      final touching = Vec3(x, 0, 8.5);
      expect(physics.overlapSphere(touching, 0.6).single.gameObject, same(object), reason: name);
      expect(physics.overlapSphere(touching, 0.4), isEmpty, reason: name);
      expect(physics.overlapBox(touching, const Vec3(0.6, 0.6, 0.6)).single.gameObject, same(object), reason: name);
      expect(physics.overlapBox(touching, const Vec3(0.4, 0.4, 0.4)), isEmpty, reason: name);
      final (a, b) = (Vec3(x - 0.2, 0, 8.5), Vec3(x + 0.2, 0, 8.5));
      expect(physics.overlapCapsule(a, b, 0.6).single.gameObject, same(object), reason: name);
      expect(physics.checkSphere(touching, 0.6), isTrue, reason: name);
      expect(physics.checkSphere(touching, 0.4), isFalse, reason: name);
      expect(physics.checkBox(touching, const Vec3(0.6, 0.6, 0.6)), isTrue, reason: name);
      expect(physics.checkBox(touching, const Vec3(0.4, 0.4, 0.4)), isFalse, reason: name);
      expect(physics.checkCapsule(a, b, 0.6), isTrue, reason: name);
      expect(physics.checkCapsule(a, b, 0.4), isFalse, reason: name);
    }
    // Between them, nothing.
    expect(physics.raycast(Ray(const Vec3(2.5, 0, 0), forward)), isNull);
    expect(physics.raycastAll(Ray(const Vec3(2.5, 0, 0), forward)), isEmpty);
    expect(physics.sphereCast(Ray(const Vec3(2.5, 0, 0), forward), 0.4), isNull);
    expect(physics.overlapSphere(const Vec3(2.5, 0, 10), 0.4), isEmpty);
  });

  test('max distance stops a query short', () {
    final world = newWorld();
    three(world);
    final physics = world.physics;
    final ray = Ray(Vec3.zero, forward);
    expect(physics.raycast(ray, maxDistance: 8.9), isNull);
    expect(physics.raycast(ray, maxDistance: 9.1), isNotNull);
    expect(physics.sphereCast(ray, 0.5, maxDistance: 8.4), isNull);
    expect(physics.sphereCast(ray, 0.5, maxDistance: 8.6), isNotNull);
    expect(() => physics.raycast(ray, maxDistance: -1), throwsA(isA<PhysicsException>()));
  });

  test('layers: a query finds only the layers in its mask, and follows a layer set later', () {
    final world = newWorld();
    final shapes = three(world);
    final physics = world.physics;
    final ray = Ray(Vec3.zero, forward);
    shapes.box.layer = 3;
    expect(physics.raycast(ray, layers: LayerMask.of([3]))?.gameObject, same(shapes.box));
    expect(physics.raycast(ray, layers: LayerMask.except([3])), isNull);
    expect(physics.raycast(ray), isNotNull, reason: 'every layer unless given');
    expect(physics.overlapSphere(const Vec3(0, 0, 10), 8, layers: LayerMask.of([3])), hasLength(1));
    expect(physics.overlapSphere(const Vec3(0, 0, 10), 8, layers: LayerMask.except([3])), hasLength(2));
    expect(physics.checkSphere(const Vec3(0, 0, 10), 1, layers: LayerMask.none), isFalse);
    expect(physics.sphereCast(ray, 0.5, layers: LayerMask.of([0])), isNull);
    shapes.box.layer = 0;
    expect(physics.raycast(ray, layers: LayerMask.of([0]))?.gameObject, same(shapes.box));
  });

  test('triggers: found unless a query ignores them, or the global says so', () {
    final world = newWorld();
    final shapes = three(world);
    final physics = world.physics;
    final ray = Ray(Vec3.zero, forward);
    final box = shapes.box.get<BoxCollider>()!..isTrigger = true;
    expect(physics.raycast(ray)?.collider, same(box), reason: 'queries hit triggers unless told not to');
    expect(physics.raycast(ray, triggers: QueryTriggers.ignore), isNull);
    physics.queriesHitTriggers = false;
    expect(physics.raycast(ray), isNull);
    expect(physics.overlapSphere(const Vec3(0, 0, 10), 1), isEmpty);
    expect(physics.raycast(ray, triggers: QueryTriggers.collide)?.collider, same(box));
    expect(physics.checkSphere(const Vec3(0, 0, 10), 1, triggers: QueryTriggers.collide), isTrue);
    box.isTrigger = false;
    expect(physics.raycast(ray, triggers: QueryTriggers.ignore)?.collider, same(box));
  });

  test('colliders scale with their objects as Unity\'s do', () {
    final world = newWorld();
    final physics = world.physics;
    // A sphere takes the largest axis; a box each axis; a capsule's height
    // its own axis and its radius the larger of the others; the center
    // is scaled too.
    at(world, const Vec3(0, 0, 10), SphereCollider(radius: 1), scale: const Vec3(1, 3, 2));
    expect(physics.raycast(Ray(Vec3.zero, forward))!.distance, closeTo(7, 1e-4));
    at(world, const Vec3(10, 0, 10), BoxCollider(size: const Vec3(2, 2, 2)), scale: const Vec3(1, 1, 4));
    expect(physics.raycast(Ray(const Vec3(10, 0, 0), forward))!.distance, closeTo(6, 1e-4));
    at(
      world,
      const Vec3(-10, 0, 10),
      CapsuleCollider(radius: 1, height: 4, direction: CapsuleDirection.z),
      scale: const Vec3(0.5, 2, 2),
    );
    // Height 8 along z, radius 2: its near end at 10 - 4.
    expect(physics.raycast(Ray(const Vec3(-10, 0, 0), forward))!.distance, closeTo(6, 1e-4));
    expect(physics.raycast(Ray(const Vec3(-11.9, 0, 0), forward)), isNotNull, reason: 'radius 2');
    at(world, const Vec3(20, 0, 10), SphereCollider(radius: 1, center: const Vec3(0, 0, -1)), scale: Vec3.one * 2);
    // Centre at z 8, radius 2.
    expect(physics.raycast(Ray(const Vec3(20, 0, 0), forward))!.distance, closeTo(6, 1e-4));
  });

  test('colliders follow their objects: moved, turned, scaled, reparented, with no frame between', () {
    final world = newWorld();
    final physics = world.physics;
    final ball = at(world, const Vec3(0, 0, 10), SphereCollider(radius: 1));
    final ray = Ray(Vec3.zero, forward);
    expect(physics.raycast(ray)!.distance, closeTo(9, 1e-4));

    ball.position = const Vec3(0, 0, 20);
    expect(physics.raycast(ray)!.distance, closeTo(19, 1e-4));
    ball.transform = const Transform(position: Vec3(0, 0, 20), scale: Vec3(3, 3, 3));
    expect(physics.raycast(ray)!.distance, closeTo(17, 1e-4));

    // A parent moves its children's colliders.
    final parent = world.add(GameObject(transform: const Transform(position: Vec3(0, 0, -5))));
    ball.parent = parent;
    final now = ball.worldTransform.position.z;
    expect(physics.raycast(Ray(const Vec3(0, 0, -100), forward))!.point.z, closeTo(now - 3, 1e-3));
    parent.position = const Vec3(100, 0, 0);
    expect(physics.raycast(ray), isNull);
    expect(physics.raycast(Ray(const Vec3(100, 0, 0), forward)), isNotNull);

    // A turned box.
    final box = at(world, const Vec3(-10, 0, 10), BoxCollider(size: const Vec3(1, 1, 6)));
    final side = Ray(const Vec3(-20, 0, 12), const Vec3(1, 0, 0));
    expect(physics.raycast(side)?.gameObject, same(box));
    box.rotation = Quat.axisAngle(Vec3.up, 1.5707963);
    expect(physics.raycast(side), isNull);
  });

  test('a collider is found while it\'s enabled and its object shown, and only then', () {
    final world = newWorld();
    final physics = world.physics;
    final collider = SphereCollider(radius: 1);
    final ball = at(world, const Vec3(0, 0, 10), collider);
    final ray = Ray(Vec3.zero, forward);
    expect(physics.colliderCount, 1);

    collider.enabled = false;
    expect(physics.raycast(ray), isNull);
    expect(physics.colliderCount, 0);
    expect(() => collider.closestPoint(Vec3.zero), throwsStateError);
    collider.enabled = true;
    expect(physics.raycast(ray)?.collider, same(collider));

    ball.active = false;
    world.update(1 / 60);
    expect(physics.raycast(ray), isNull);
    ball.active = true;
    world.update(1 / 60);
    expect(physics.raycast(ray)?.collider, same(collider));

    ball.removeComponent(collider);
    world.update(1 / 60);
    expect(physics.raycast(ray), isNull);
    final again = SphereCollider(radius: 2);
    ball.addComponent(again);
    world.update(1 / 60);
    expect(physics.raycast(ray)?.collider, same(again));

    ball.destroy();
    world.update(1 / 60);
    expect(physics.raycast(ray), isNull);
    expect(physics.colliderCount, 0);
  });

  test('colliders removed from the middle leave the rest followed', () {
    final world = newWorld();
    final physics = world.physics;
    final balls = [for (var i = 0; i < 4; i++) at(world, Vec3(i * 10.0, 0, 10), SphereCollider(radius: 1))];
    GameObject? along(double x) => physics.raycast(Ray(Vec3(x, 0, 0), forward))?.gameObject;

    balls[0].get<SphereCollider>()!.enabled = false;
    balls[3].position = const Vec3(30, 0, 20);
    expect(along(30)?.position.z, 20, reason: 'the last one, put where the first was, still follows');
    balls[3].get<SphereCollider>()!.enabled = false;
    balls[1].position = const Vec3(10, 0, 30);
    expect([along(0), along(10), along(20), along(30)], [null, balls[1], balls[2], null]);
    expect(physics.raycast(Ray(const Vec3(10, 0, 0), forward))!.distance, closeTo(29, 1e-4));
    expect(physics.colliderCount, 2);
  });

  test('changing a collider\'s shape changes what\'s found', () {
    final world = newWorld();
    final physics = world.physics;
    final sphere = SphereCollider(radius: 1);
    final capsule = CapsuleCollider(radius: 0.5, height: 1);
    final box = BoxCollider();
    at(world, const Vec3(0, 0, 10), sphere);
    at(world, const Vec3(10, 0, 10), capsule);
    at(world, const Vec3(20, 0, 10), box);
    double distanceAt(double x) => physics.raycast(Ray(Vec3(x, 0, 0), forward))!.distance;

    sphere.radius = 3;
    expect(distanceAt(0), closeTo(7, 1e-4));
    sphere.center = const Vec3(0, 0, 2);
    expect(distanceAt(0), closeTo(9, 1e-4));
    capsule.direction = CapsuleDirection.z;
    capsule.height = 6;
    expect(distanceAt(10), closeTo(7, 1e-4));
    box.size = const Vec3(1, 1, 10);
    expect(distanceAt(20), closeTo(5, 1e-4));
    expect(() => box.size = const Vec3(1, 0, 1), throwsArgumentError);
    expect(() => SphereCollider(radius: -1), throwsArgumentError);
    expect(() => CapsuleCollider(height: double.nan), throwsArgumentError);
  });

  test('all hits come nearest first; a cast\'s all gives what it starts in at 0', () {
    final world = newWorld();
    final physics = world.physics;
    final far = at(world, const Vec3(0, 0, 20), SphereCollider(radius: 1));
    final middle = at(world, const Vec3(0, 0, 10), SphereCollider(radius: 1));
    final around = at(world, Vec3.zero, SphereCollider(radius: 1));
    final ray = Ray(Vec3.zero, forward);
    expect(physics.raycastAll(ray).map((h) => h.gameObject), [middle, far], reason: 'not the one it starts in');
    expect(physics.raycast(ray)?.gameObject, same(middle));
    final all = physics.sphereCastAll(ray, 0.5);
    expect(all.map((h) => h.gameObject), [around, middle, far]);
    expect(all.first.distance, 0);
    expect(all.first.point, Vec3.zero);
    expect(all.first.normal, near(const Vec3(0, 0, -1)));
    expect(physics.sphereCast(ray, 0.5)?.gameObject, same(middle), reason: 'a single cast skips what it starts in');
  });

  test('a collider\'s own closest point and ray', () {
    final world = newWorld();
    final collider = BoxCollider(size: const Vec3(2, 2, 2));
    at(world, const Vec3(0, 0, 10), collider);
    final other = at(world, const Vec3(0, 0, 5), SphereCollider(radius: 1));
    expect(collider.closestPoint(Vec3.zero), near(const Vec3(0, 0, 9)));
    expect(collider.closestPoint(const Vec3(0.5, 0, 10)), near(const Vec3(0.5, 0, 10)), reason: 'inside');
    final hit = collider.raycast(Ray(Vec3.zero, forward))!;
    expect(hit.collider, same(collider), reason: 'whatever is in front: ${other.name}');
    expect(hit.distance, closeTo(9, 1e-4));
    expect(collider.raycast(Ray(Vec3.zero, forward), maxDistance: 8), isNull);
    expect(collider.raycast(Ray(const Vec3(0, 0, 10), forward)), isNull, reason: 'from inside');
  });

  test('drawColliders draws each one with the debug lines', () {
    final world = newWorld();
    three(world);
    final physics = world.physics;
    // Made after the physics, so it runs after it in the frame and sees
    // the lines before they're sent.
    final lines = world.system(_Lines.new);
    world.update(1 / 60);
    expect(lines.seen, 0);
    physics.drawColliders = true;
    world.update(1 / 60);
    final drawn = lines.seen;
    // A box's 12 edges, a sphere's 3 circles of 32, and a capsule's lines.
    expect(drawn, greaterThan(12 + 96 + 4));
    world.update(1 / 60);
    expect(lines.seen, drawn, reason: 'again each frame, not more');
    world.debug.enabled = false;
    world.update(1 / 60);
    expect(lines.seen, 0);
  });

  test('a world disposed with colliders in it frees its physics', () {
    final world = World();
    three(world);
    world.dispose();
  });

  test('a Physics kept past its world throws when used, writing into nothing freed', () {
    final world = World();
    three(world);
    final physics = world.physics; // as a HUD's State might keep it
    world.dispose();
    final disposed = throwsA(isA<StateError>().having((e) => e.message, 'message', contains('disposed World')));
    expect(() => physics.gravity = const Vec3(0, -1, 0), disposed);
    expect(() => physics.defaultSolverIterations = 8, disposed);
    expect(() => physics.raycast(Ray(Vec3.zero, Vec3.forward)), disposed);
    expect(() => physics.overlapSphere(Vec3.zero, 1), disposed);
    expect(physics.gravity, const Vec3(0, -9.81, 0), reason: 'the failed set changed nothing');
  });
}

/// Counts the debug lines at the end of each frame's late update.
class _Lines extends WorldSystem {
  _Lines(super.world);

  var seen = 0;

  @override
  void afterLateUpdate() {
    super.afterLateUpdate();
    seen = world.debug.count;
  }
}
