// Mesh colliders through the package (docs/stage-5.md, step 6): a mesh's
// triangles or hull, from a Mesh, a MeshRenderer or a model's parts, hit
// by rays and by bodies, each mesh given to physics once.

import 'dart:io';

import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

World newWorld() {
  final world = World();
  addTearDown(world.dispose);
  return world;
}

/// A V-shaped trough along Z: its bottom on the Z axis, its sides rising
/// to y = 2 at x = ±2. Its hull is a prism with a flat top at y = 2.
Mesh trough() => Mesh.custom(
  positions: const [Vec3(-2, 2, -1), Vec3(0, 0, -1), Vec3(2, 2, -1), Vec3(-2, 2, 1), Vec3(0, 0, 1), Vec3(2, 2, 1)],
  indices: const [0, 3, 1, 1, 3, 4, 1, 4, 2, 2, 4, 5],
);

/// How high a ray straight down at [x] hits, or null.
double? height(World world, double x) => world.physics.raycast(Ray(Vec3(x, 10, 0), const Vec3(0, -1, 0)))?.point.y;

void run(World world, double seconds) {
  for (var t = 0.0; t < seconds; t += 1 / 60) {
    world.update(1 / 60);
  }
}

void main() {
  test('a ray hits a mesh\'s triangles, or its hull when convex', () {
    final world = newWorld();
    final mesh = trough();
    world.add(GameObject(components: [MeshCollider(mesh: mesh)]));
    final hull = world.add(
      GameObject(
        transform: Transform(position: const Vec3(10, 0, 0)),
        components: [MeshCollider(mesh: mesh, convex: true)],
      ),
    );
    world.update(1 / 60);
    expect(height(world, 0), closeTo(0, 1e-3));
    expect(height(world, 1), closeTo(1, 1e-3));
    expect(height(world, 2.5), isNull);
    expect(height(world, 10), closeTo(2, 1e-3));
    expect(world.physics.meshCount, 1, reason: 'one mesh, given once');

    hull.get<MeshCollider>()!.convex = false;
    expect(height(world, 10), closeTo(0, 1e-3));
  });

  test('a mesh is scaled on each axis by its object', () {
    final world = newWorld();
    final object = world.add(
      GameObject(
        transform: Transform(scale: const Vec3(2, 0.5, 1)),
        components: [MeshCollider(mesh: trough())],
      ),
    );
    world.update(1 / 60);
    expect(height(world, 2), closeTo(0.5, 1e-3));
    object.transform = Transform(scale: const Vec3(1, 1, 1));
    expect(height(world, 2), closeTo(2, 1e-3));
  });

  test('a ball rolls down into the bottom of a trough', () {
    final world = newWorld();
    world.add(GameObject(components: [MeshCollider(mesh: trough())]));
    final ball = world.add(
      GameObject(
        transform: Transform(position: const Vec3(1.2, 3, 0)),
        components: [SphereCollider(radius: 0.25), Rigidbody()],
      ),
    );
    run(world, 6);
    expect(ball.position.x, closeTo(0, 0.05));
    expect(ball.position.y, closeTo(0.25 * 1.41421356, 0.02));
  });

  test('on a dynamic rigidbody a mesh collides as its hull, and lands on it', () {
    final world = newWorld();
    world.add(
      GameObject(
        transform: Transform(position: const Vec3(0, -0.5, 0)),
        components: [BoxCollider(size: const Vec3(20, 1, 20))],
      ),
    );
    final body = world.add(
      GameObject(
        transform: Transform(position: const Vec3(0, 5, 0), rotation: Quat.euler(0, 0, 180)),
        components: [
          MeshCollider(mesh: trough()),
          Rigidbody(),
        ],
      ),
    );
    run(world, 4);
    // Upside down, it rests on the hull's flat top.
    expect(body.position.y, closeTo(2, 0.02));
  });

  test('without a mesh it takes its MeshRenderer\'s, and a model\'s parts get one each', () async {
    final world = newWorld();
    final mesh = trough();
    final drawn = world.add(GameObject(components: [MeshRenderer(mesh), MeshCollider()]));
    world.update(1 / 60);
    expect(height(world, 1), closeTo(1, 1e-3));
    expect(drawn.get<MeshCollider>()!.mesh, isNull);
    drawn.get<MeshRenderer>()!.mesh = Mesh.cube();
    drawn.get<MeshCollider>()!.convex = true;
    expect(height(world, 0), closeTo(0.5, 1e-3), reason: 'read again when it changes');

    final box = await Model.load(File('../../tests/assets/gltf/Box.glb').readAsBytesSync());
    final model = world.add(box.instantiate(transform: Transform(position: const Vec3(10, 0, 0))));
    final parts = MeshCollider.addToParts(model);
    expect(parts, isNotEmpty);
    expect(height(world, 10), closeTo(0.5, 1e-3));

    // With neither, it says so in the world's errors, as what onEnable
    // throws does, and isn't in the world.
    final errors = <Object>[];
    world.errors.listen((e) => errors.add(e.error));
    final none = world.add(GameObject(components: [MeshCollider()]));
    await pumpEventQueue();
    expect(errors, [isA<StateError>().having((e) => e.message, 'message', contains('no mesh'))]);
    expect(() => none.get<MeshCollider>()!.closestPoint(Vec3.zero), throwsStateError);
  });

  test('a mesh is given to physics once, and let go when no collider uses it', () {
    final world = newWorld();
    final (a, b) = (trough(), Mesh.cube());
    final one = world.add(GameObject(components: [MeshCollider(mesh: a)]));
    final two = world.add(GameObject(components: [MeshCollider(mesh: a, convex: true)]));
    world.update(1 / 60);
    final physics = world.physics;
    expect(physics.meshCount, 1);
    one.get<MeshCollider>()!.mesh = b;
    expect(physics.meshCount, 2);
    two.get<MeshCollider>()!.mesh = b;
    expect(physics.meshCount, 1);
    one.get<MeshCollider>()!.enabled = false;
    expect(physics.meshCount, 1);
    two.destroy();
    world.update(1 / 60);
    expect(physics.meshCount, 0);
    one.get<MeshCollider>()!.enabled = true;
    expect(physics.meshCount, 1);
  });

  test('drawColliders draws a mesh collider\'s edges', () {
    final world = newWorld();
    world.add(GameObject(components: [MeshCollider(mesh: trough())]));
    world.add(GameObject(components: [MeshCollider(mesh: Mesh.cube(), convex: true)]));
    final physics = world.physics..drawColliders = true;
    final lines = world.system(_Lines.new);
    world.update(1 / 60);
    // The trough's 9 edges and the cube's 12 or more (its hull's faces
    // may be split in triangles).
    expect(lines.seen, inInclusiveRange(9 + 12, 9 + 18));
    physics.drawColliders = false;
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
