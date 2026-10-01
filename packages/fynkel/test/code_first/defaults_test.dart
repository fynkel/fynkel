// Code first, Tier 1 (docs/code-first.md, ADR-0026 §6): a built-in
// component constructed from code starts from the same defaults the
// Inspector's Add Component writes (the Rust `Default`), so a script names
// only what it changes — `RigidBody()`, `RigidBody(mass: 5)`.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    return w;
  }

  test('a crate from RigidBody() and Collider() falls and comes to rest on a floor', () {
    final w = fresh();
    w.spawn(name: 'floor').add(const Collider(size: Vec3(10, 0.5, 10), offset: Vec3(0, -0.5, 0)));
    final crate =
        w.spawn(
            name: 'crate',
            transform: const Pose(position: Vec3(0, 3, 0)),
          )
          ..add(const RigidBody())
          ..add(const Collider());
    for (var i = 0; i < 240; i++) {
      w.frame(1 / 60);
    }
    // Collider()'s box is 0.5 half-extents: resting on the floor's top at 0.
    expect(crate.localTransform.position.y, closeTo(0.5, 0.05));
    expect(crate.bodyState()!.sleeping, isTrue);
  });

  test('the defaults are the ones Add Component writes, and one argument changes one field', () {
    // fynkel_physics::components::RigidBody::default()
    const body = RigidBody(mass: 5);
    expect(body.bodyType, BodyType.dynamic);
    expect(body.mass, 5);
    expect(body.gravityScale, 1);
    expect(body.ccd, isFalse);
    expect(body, const RigidBody().copyWith(mass: 5));

    expect(const Pose(), identityTransform);
    expect(const MeshRenderer().mesh, AssetRef.none);
    expect(const MeshRenderer().castShadows, isTrue);
    expect(const Collider().shape, ColliderShape.box);
    expect(const Collider().isSensor, isFalse);
    expect(const PointLight().range, 10);
  });

  test('a component from its defaults reads back as written', () {
    final w = fresh();
    final light = w.spawn(name: 'lamp')..add(const PointLight(intensity: 400));
    w.frame(1 / 60);
    w.frame(1 / 60);
    final read = light.get<PointLight>()!;
    expect(read.intensity, 400);
    expect(read.range, 10);
    expect(read.color, const Vec3(1, 1, 1));
  });
}
