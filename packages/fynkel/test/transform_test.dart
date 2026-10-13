// Transforms kept in Dart and sent in one call (docs/stage-3.md, step 3b).

import 'dart:ffi';
import 'dart:io';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/src/bindings.g.dart';
import 'package:fynkel/src/native.dart';
import 'package:test/test.dart';
import 'package:fynkel/src/world.dart' show WorldPointer;

/// What the engine refusing with [status] throws in Dart: an
/// [ArgumentError] for a bad value, a [StateError] for something gone or in
/// use, a [FynkelException] for the engine's own failures.
Matcher throwsRefusal(FynkelStatus status, [Pattern? message]) => throwsA(
  switch (status) {
    FynkelStatus.invalidValue => isA<ArgumentError>(),
    FynkelStatus.noObject ||
    FynkelStatus.noComponent ||
    FynkelStatus.noMesh ||
    FynkelStatus.noMaterial ||
    FynkelStatus.noTexture ||
    FynkelStatus.inUse => isA<StateError>(),
    _ => isA<FynkelException>().having((e) => e.status, 'status', status),
  }.having((e) => '$e', 'text', message == null ? anything : contains(message)),
);

/// The engine's own transform for [object], read past the Dart cache.
(Vec3, Quat, Vec3) engine(GameObject object) => using((arena) {
  final out = arena<FyTransform>();
  check(fy_transform_get(object.world.pointer, object.id, out));
  return (out.ref.position.read(), out.ref.rotation.read(), out.ref.scale.read());
});

/// [t] as the engine keeps it, in 32-bit floats.
(Vec3, Quat, Vec3) parts(Transform t) {
  double f(double x) => (Float32List(1)..[0] = x)[0];
  final Transform(position: p, rotation: r, scale: s) = t;
  return (Vec3(f(p.x), f(p.y), f(p.z)), Quat(f(r.x), f(r.y), f(r.z), f(r.w)), Vec3(f(s.x), f(s.y), f(s.z)));
}

/// What the engine says when given [value] directly.
String engineRefusal(World world, Transform value) => using((arena) {
  final id = arena<Uint64>();
  check(fy_spawn(world.pointer, id));
  final t = arena<FyTransform>();
  t.ref.position.write(value.position);
  t.ref.rotation.write(value.rotation);
  t.ref.scale.write(value.scale);
  final status = fy_transform_set(world.pointer, id.value, t);
  expect(status, FyStatus.FY_STATUS_INVALID_VALUE);
  return fy_last_error().cast<Utf8>().toDartString();
});

void main() {
  late World world;
  setUp(() => world = World());
  tearDown(() => world.dispose());

  test('a set transform reads back at once, and reaches the engine when something reads the engine', () {
    final a = world.add(GameObject());
    final b = a.add(GameObject(transform: Transform(position: Vec3(0, 1, 0))));
    world.raycast(Ray(Vec3.zero, Vec3.forward));
    expect(engine(b).$1, Vec3(0, 1, 0), reason: 'an added object is sent like any move');

    a.transform = Transform(position: Vec3(5, 0, 0));
    b.rotate(Vec3.up, 0.5);
    expect(a.transform.position, Vec3(5, 0, 0));
    expect(engine(a).$1, Vec3.zero, reason: 'not sent yet');
    expect(b.worldTransform.position, Vec3(5, 1, 0), reason: 'worked out in Dart');
    expect(engine(a).$1, Vec3.zero, reason: 'so still not sent');
    world.raycast(Ray(Vec3.zero, Vec3.forward));
    expect(engine(a), parts(a.transform));
    expect(engine(b), parts(b.transform));

    a.transform = Transform(position: Vec3(0, 0, -5));
    world.raycast(Ray(Vec3.zero, Vec3.forward));
    expect(engine(a).$1, Vec3(0, 0, -5), reason: 'a raycast sends them');
  });

  test("a view's frame sends them, all in one go, however many", () {
    final view = HeadlessView(world, width: 8, height: 8);
    addTearDown(view.dispose);
    final objects = [for (var i = 0; i < 100; i++) world.add(GameObject())];
    for (final (i, o) in objects.indexed) {
      o.transform = Transform(position: Vec3(i.toDouble(), 0, 0));
      o.transform = o.transform.translated(Vec3(0, 1, 0));
    }
    expect(() => engine(objects.last), throwsRefusal(FynkelStatus.noComponent), reason: 'not even the first one yet');
    view.frame();
    for (final (i, o) in objects.indexed) {
      expect(engine(o).$1, Vec3(i.toDouble(), 1, 0));
    }
  });

  test('an object set many times in a frame is sent once, with its last value', () {
    final o = world.add(GameObject());
    world.raycast(Ray(Vec3.zero, Vec3.forward));
    final before = scratch.idCapacity;
    for (var i = 1; i <= before * 4; i++) {
      o.transform = Transform(position: Vec3(i.toDouble(), 0, 0));
    }
    world.raycast(Ray(Vec3.zero, Vec3.forward));
    expect(scratch.idCapacity, before, reason: 'one id to send, not one per set');
    expect(engine(o).$1, Vec3(before * 4.0, 0, 0));
  });

  test('it reads back exactly as it was set; the engine keeps it in 32-bit floats', () {
    final o = world.add(GameObject(transform: Transform(position: Vec3(0.1, 0, 0))));
    expect(o.transform.position.x, 0.1);
    world.raycast(Ray(Vec3.zero, Vec3.forward));
    expect(engine(o).$1.x, (Float32List(1)..[0] = 0.1)[0]);
  });

  test('a value the engine would refuse throws when set, with the engine\'s own words, and changes nothing', () {
    final o = world.add(GameObject(transform: Transform(position: Vec3(1, 2, 3))));
    final bad = <Transform>[
      Transform(position: Vec3(double.nan, 0, 0)),
      Transform(position: Vec3(1e39, 0, 0)), // finite here, infinite as a 32-bit float
      Transform(position: Vec3(0, 0, 3.4028236e38)), // rounds up to infinity
      Transform(scale: Vec3(1, double.infinity, 1)),
      Transform(rotation: const Quat(0, 0, 0, 2)),
      Transform(rotation: const Quat(0, 0, 0, 1.002)),
      Transform(rotation: const Quat(double.nan, 0, 0, 1)),
      // A bad rotation is reported before a bad scale, as the engine checks.
      Transform(rotation: const Quat(0, 0, 0, 2), scale: Vec3(double.nan, 1, 1)),
    ];
    for (final value in bad) {
      expect(() => o.transform = value, throwsRefusal(FynkelStatus.invalidValue, engineRefusal(world, value)));
    }
    expect(o.transform.position, Vec3(1, 2, 3));
    expect(() => world.add(GameObject(transform: bad.first)), throwsRefusal(FynkelStatus.invalidValue));
    expect(world.all<GameObject>(), [o], reason: 'refused before it is spawned');
    o.transform = Transform(position: Vec3(0, -3.4028235e38, 0));
    expect(o.transform.position.y, -3.4028235e38, reason: 'rounds to the largest 32-bit float, so it is finite');
    o.transform = Transform(rotation: const Quat(0, 0, 0, 1.0009));
    expect(o.transform.rotation.w, closeTo(1.0009, 1e-6), reason: 'near enough to a unit, as the engine allows');
  });

  test('a destroyed object is not sent, and its transform is gone', () {
    final view = HeadlessView(world, width: 8, height: 8);
    addTearDown(view.dispose);
    final keep = world.add(GameObject());
    final gone = world.add(GameObject());
    keep.transform = Transform(position: Vec3(1, 0, 0));
    gone.transform = Transform(position: Vec3(2, 0, 0));
    gone.destroy();
    view.frame();
    expect(engine(keep).$1, Vec3(1, 0, 0));
    expect(() => gone.transform, throwsA(isA<StateError>()));
    expect(() => gone.transform = Transform(), throwsA(isA<StateError>()));
  });

  test("a model's root and parts read their transforms from the engine once, then keep them", () async {
    final truck = await Model.load(File('../../tests/assets/gltf/CesiumMilkTruck.glb').readAsBytesSync());
    addTearDown(truck.dispose);
    final copy = world.add(truck.instantiate(transform: Transform(position: Vec3(-3, 0, 0))));
    expect(copy.transform.position, Vec3(-3, 0, 0));
    final part = copy.find('Cesium_Milk_Truck')!;
    expect(parts(part.transform), engine(part));
    part.transform = part.transform.translated(Vec3(0, 1, 0));
    world.raycast(Ray(Vec3.zero, Vec3.forward));
    expect(engine(part), parts(part.transform));
  });

  test('a view keeps working after its world is disposed with moves unsent', () {
    final other = World();
    final view = HeadlessView(other, width: 8, height: 8);
    addTearDown(view.dispose);
    final o = other.add(GameObject());
    o.transform = Transform(position: Vec3(1, 0, 0));
    other.dispose();
    expect(view.frame, returnsNormally);
    expect(() => o.transform, throwsStateError);
    expect(() => o.transform = Transform(), throwsStateError);
  });
}
