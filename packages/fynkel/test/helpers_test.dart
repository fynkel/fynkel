// Hardening G5: the helpers the examples wrote for themselves.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

void main() {
  test('Vec3: withX, withY, withZ, flat and all', () {
    const v = Vec3(1, 2, 3);
    expect(v.withX(5), Vec3(5, 2, 3));
    expect(v.withY(5), Vec3(1, 5, 3));
    expect(v.withZ(5), Vec3(1, 2, 5));
    expect(v.flat, Vec3(1, 0, 3), reason: 'on the ground: y = 0');
    expect(const Vec3.all(2), Vec3(2, 2, 2));
  });

  test('Transform.lookAt: at eye, looking at target', () {
    final t = Transform.lookAt(Vec3(0, 1.5, 4), Vec3.zero);
    expect(t.position, Vec3(0, 1.5, 4));
    expect(t.rotation, Quat.lookRotation(Vec3.zero - Vec3(0, 1.5, 4)));
    expect(t.scale, Vec3.one);
    expect(Transform.lookAt(Vec3(0, 0, 1), Vec3.zero, scale: Vec3.all(2)).scale, Vec3.all(2));
  });

  test('Light.sun(direction:) turns its object to shine that way', () {
    final world = World();
    addTearDown(world.dispose);
    const way = Vec3(-0.4, -1, -0.3);
    final sun = world.add(GameObject(components: [Light.sun(direction: way, intensity: 2)]));
    expect(sun.forward.closeTo(way.normalized, 1e-6), isTrue, reason: '${sun.forward}');
    expect(sun.get<Light>()!.intensity, 2);
    final later = world.add(GameObject())..addComponent(Light.sun(direction: Vec3.down));
    expect(later.forward.closeTo(Vec3.down, 1e-6), isTrue);
    expect(() => Light.sun(direction: Vec3.zero), throwsArgumentError);
  });

  test('Signal<void>.emit() needs no null; a signal of values still needs one', () {
    final died = Signal<void>();
    var heard = 0;
    died.listen((_) => heard++);
    died.emit();
    expect(heard, 1);
    final score = Signal<int>();
    expect(() => score.emit(), throwsArgumentError);
    final maybe = Signal<int?>();
    int? got = 1;
    maybe.listen((v) => got = v);
    maybe.emit();
    expect(got, isNull);
  });

  test('a ViewPoint as a Vec2', () {
    expect(const ViewPoint(3, 4, width: 10, height: 10).toVec2(), Vec2(3, 4));
  });

  test('addWith adds a child holding the component and gives the component back (hardening G6)', () {
    final world = World();
    addTearDown(world.dispose);
    final root = world.add(GameObject());
    final Light sun = root.addWith(
      Light.sun(),
      name: 'sun',
      transform: Transform(position: Vec3(0, 5, 0)),
      layer: 3,
    );
    expect(sun.gameObject.parent, same(root));
    expect(sun.gameObject.name, 'sun');
    expect(sun.gameObject.position, Vec3(0, 5, 0));
    expect(sun.gameObject.layer, 3);
    expect(root.find('sun')!.get<Light>(), same(sun));
    final renderer = root.addWith(MeshRenderer(), components: [Light.ambient()]);
    expect(renderer.gameObject.get<Light>()!.kind, LightKind.ambient, reason: 'the others go on it too');

    final unadded = GameObject();
    final camera = unadded.addWith(Camera());
    world.add(unadded);
    expect(world.camera, same(camera));
  });
}
