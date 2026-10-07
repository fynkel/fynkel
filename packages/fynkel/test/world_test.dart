import 'dart:ffi';

import 'package:ffi/ffi.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/src/bindings.g.dart';
import 'package:test/test.dart';

Matcher throwsFynkel(FynkelStatus status, [Pattern? message]) => throwsA(
  isA<FynkelException>()
      .having((e) => e.status, 'status', status)
      .having((e) => e.message, 'message', message == null ? anything : contains(message)),
);

void main() {
  late World world;
  setUp(() => world = World());
  tearDown(() => world.dispose());

  test("a despawned object's id throws a clear error", () {
    final cube = world.spawn(Transform());
    cube.despawn();
    expect(cube.isAlive, isFalse);
    expect(() => cube.transform, throwsFynkel(FynkelStatus.noObject, 'object#${cube.id} does not exist'));
    expect(() => cube.transform = Transform(), throwsFynkel(FynkelStatus.noObject));
    expect(cube.despawn, throwsFynkel(FynkelStatus.noObject, '(despawned, or never spawned)'));
  });

  test('components come back as they went in', () {
    final transform = Transform(
      position: Vec3(1, 2, 3),
      rotation: Quat.axisAngle(Vec3.right, 0.5),
      scale: Vec3.one * 2,
    );
    const camera = Camera(fovY: 45, near: 0.5, far: 50, clearColor: Color.rgb(0.25, 0.5, 0.75));
    const light = Light.ambient(color: Color.rgb(1, 0.5, 0), intensity: 0.25);
    final material = Material(color: Color(0xFFE0613A), metallic: 0.25, roughness: 0.75);
    final renderer = MeshRenderer(Mesh.sphere(), material);
    final o = world.spawn(transform, camera, light, renderer);

    expect(o.transform.position, transform.position);
    expect(o.transform.rotation.closeTo(transform.rotation), isTrue);
    expect(o.transform.scale, Vec3(2, 2, 2));
    expect(o.camera, camera);
    expect(o.light, light);
    expect(o.meshRenderer, renderer, reason: 'the same Mesh and Material objects');
    expect(o.meshRenderer.material, same(material));
  });

  test('a missing component throws, and remove takes it away', () {
    final o = world.spawn(Transform(), Light.sun());
    expect(() => o.camera, throwsFynkel(FynkelStatus.noComponent, 'Camera'));
    o.remove<Light>();
    expect(() => o.light, throwsFynkel(FynkelStatus.noComponent));
    expect(() => o.remove<Light>(), throwsFynkel(FynkelStatus.noComponent));
    expect(() => o.remove<Component>(), throwsArgumentError);
  });

  test('spawn refuses two components of one kind, and a refused component leaves no object behind', () {
    expect(() => world.spawn(Transform(), Transform()), throwsArgumentError);
    final before = world.spawn();
    expect(() => world.spawn(Transform(), const Camera(fovY: 500)), throwsFynkel(FynkelStatus.invalidValue, 'fov_y'));
    final after = world.spawn();
    for (var id = before.id + 1; id < after.id; id++) {
      expect(fy_exists(world.pointer, id), isFalse, reason: 'object#$id was despawned again');
    }
  });

  test('the engine refuses values that are not finite or not unit rotations', () {
    final o = world.spawn();
    expect(() => o.transform = Transform(position: Vec3(double.nan, 0, 0)), throwsFynkel(FynkelStatus.invalidValue));
    expect(() => o.transform = Transform(rotation: Quat(0, 0, 0, 2)), throwsFynkel(FynkelStatus.invalidValue, 'unit'));
    expect(() => Material(roughness: 1.5), throwsRangeError);
    expect(() => Material().metallic = double.nan, throwsRangeError);
  });

  test('rotate turns around the object\'s own axis and stays a unit rotation', () {
    final o = world.spawn(Transform(rotation: Quat.axisAngle(Vec3.right, 0.3)));
    for (var i = 0; i < 10000; i++) {
      o.rotate(Vec3.up, 0.01);
    }
    final up = o.transform.rotation.rotate(Vec3.up);
    expect(
      up.closeTo(Quat.axisAngle(Vec3.right, 0.3).rotate(Vec3.up), 1e-3),
      isTrue,
      reason: 'spinning around its own up keeps it',
    );
    o.translate(Vec3(1, 0, 0));
    expect(o.transform.position, Vec3(1, 0, 0));
  });

  test('a disposed world refuses calls with a clear error, and dispose twice is fine', () {
    final other = World();
    final o = other.spawn(Transform());
    other.dispose();
    other.dispose();
    expect(o.isAlive, isFalse);
    expect(() => o.transform, throwsA(isA<StateError>().having((e) => e.message, 'message', contains('disposed'))));
    expect(other.spawn, throwsStateError);
  });

  test('a material used in two worlds changes in both, and a disposed world is forgotten', () {
    // What the engine itself holds for the material on [object].
    double engineGreen(GameObject object) {
      final renderer = malloc<FyMeshRenderer>();
      final material = malloc<FyMaterial>();
      try {
        expect(fy_mesh_renderer_get(object.world.pointer, object.id, renderer), FyStatus.FY_STATUS_OK);
        expect(fy_material_get(object.world.pointer, renderer.ref.material, material), FyStatus.FY_STATUS_OK);
        return material.ref.base_color.g;
      } finally {
        malloc.free(renderer);
        malloc.free(material);
      }
    }

    final second = World();
    final third = World();
    final material = Material(color: Color.black);
    final a = world.spawn(MeshRenderer(Mesh.cube(), material));
    final b = second.spawn(MeshRenderer(Mesh.cube(), material));
    third.spawn(MeshRenderer(Mesh.cube(), material));
    third.dispose();

    material.color = Color(0xFF00FF00);
    expect(engineGreen(a), 1);
    expect(engineGreen(b), 1);
    second.dispose();
    material.roughness = 1;
    expect(engineGreen(a), 1);
  });

  test("the status codes match the engine's", () {
    expect(FynkelStatus.values.length, FyStatus.FY_STATUS_POISONED + 1);
    expect(FynkelStatus.noObject.index, FyStatus.FY_STATUS_NO_OBJECT);
    expect(FynkelStatus.timeout.index, FyStatus.FY_STATUS_TIMEOUT);
    expect(FynkelStatus.poisoned.index, FyStatus.FY_STATUS_POISONED);
  });
}
