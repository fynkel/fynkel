import 'dart:ffi';
import 'dart:math' as math;

import 'package:ffi/ffi.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/src/bindings.g.dart';
import 'package:fynkel/src/native.dart';
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

  test("a destroyed object's id throws a clear error", () {
    final cube = world.add(GameObject(transform: Transform()));
    cube.destroy();
    expect(cube.isAlive, isFalse);
    expect(() => cube.transform, throwsFynkel(FynkelStatus.noObject, 'object#${cube.id} does not exist'));
    expect(() => cube.transform = Transform(), throwsFynkel(FynkelStatus.noObject));
    expect(() => cube.find('x'), throwsFynkel(FynkelStatus.noObject, '(despawned, or never spawned)'));
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
    final o = world.add(GameObject(transform: transform, components: [camera, light, renderer]));

    expect(o.transform.position, transform.position);
    expect(o.transform.rotation.closeTo(transform.rotation), isTrue);
    expect(o.transform.scale, Vec3(2, 2, 2));
    expect(o.camera, camera);
    expect(o.light, light);
    expect(o.meshRenderer, renderer, reason: 'the same Mesh and Material objects');
    expect(o.meshRenderer.material, same(material));
  });

  test('a missing component throws, and remove takes it away', () {
    final o = world.add(GameObject(transform: Transform(), components: [Light.sun()]));
    expect(() => o.camera, throwsFynkel(FynkelStatus.noComponent, 'Camera'));
    o.remove<Light>();
    expect(() => o.light, throwsFynkel(FynkelStatus.noComponent));
    expect(() => o.remove<Light>(), throwsFynkel(FynkelStatus.noComponent));
    expect(() => o.remove<Component>(), throwsArgumentError);
  });

  test('an object refuses two components of one kind, and a refused component leaves no object behind', () {
    expect(() => GameObject(components: [Light.sun(), Light.ambient()]), throwsArgumentError);
    final before = world.add(GameObject());
    expect(
      () => world.add(GameObject(transform: Transform(), components: [const Camera(fovY: 500)])),
      throwsFynkel(FynkelStatus.invalidValue, 'fov_y'),
    );
    final after = world.add(GameObject());
    for (var id = before.id + 1; id < after.id; id++) {
      expect(fy_exists(world.pointer, id), isFalse, reason: 'object#$id was despawned again');
    }
  });

  test('the engine refuses values that are not finite or not unit rotations', () {
    final o = world.add(GameObject());
    expect(() => o.transform = Transform(position: Vec3(double.nan, 0, 0)), throwsFynkel(FynkelStatus.invalidValue));
    expect(() => o.transform = Transform(rotation: Quat(0, 0, 0, 2)), throwsFynkel(FynkelStatus.invalidValue, 'unit'));
    expect(() => Material(roughness: 1.5), throwsRangeError);
    expect(() => Material().metallic = double.nan, throwsRangeError);
  });

  test('rotate turns around the object\'s own axis and stays a unit rotation', () {
    final o = world.add(GameObject(transform: Transform(rotation: Quat.axisAngle(Vec3.right, 0.3))));
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
    final o = other.add(GameObject(transform: Transform()));
    other.dispose();
    other.dispose();
    expect(o.isAlive, isFalse);
    expect(() => o.transform, throwsA(isA<StateError>().having((e) => e.message, 'message', contains('disposed'))));
    expect(() => other.add(GameObject()), throwsStateError);
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
    final a = world.add(GameObject(components: [MeshRenderer(Mesh.cube(), material)]));
    final b = second.add(GameObject(components: [MeshRenderer(Mesh.cube(), material)]));
    third.add(GameObject(components: [MeshRenderer(Mesh.cube(), material)]));
    third.dispose();

    material.color = Color(0xFF00FF00);
    expect(engineGreen(a), 1);
    expect(engineGreen(b), 1);
    second.dispose();
    material.roughness = 1;
    expect(engineGreen(a), 1);
  });

  test("the status codes match the engine's", () {
    expect(FynkelStatus.values.length, FyStatus.FY_STATUS_NO_TEXTURE + 1);
    expect(FynkelStatus.noObject.index, FyStatus.FY_STATUS_NO_OBJECT);
    expect(FynkelStatus.timeout.index, FyStatus.FY_STATUS_TIMEOUT);
    expect(FynkelStatus.poisoned.index, FyStatus.FY_STATUS_POISONED);
    expect(FynkelStatus.unsupported.index, FyStatus.FY_STATUS_UNSUPPORTED);
    expect(FynkelStatus.noTexture.index, FyStatus.FY_STATUS_NO_TEXTURE);
  });

  group('hierarchy', () {
    test('a parent lists its children in order, and moving one takes it from its old parent', () {
      final root = world.add(GameObject());
      final other = world.add(GameObject());
      // More children than the first buffer holds, so the call is repeated.
      final kids = [for (var i = 0; i < 40; i++) world.add(GameObject())];
      for (final kid in kids) {
        kid.parent = root;
      }
      expect(root.children, kids);
      expect(kids[3].parent, root);
      expect(root.parent, isNull);

      kids[3].parent = other;
      expect(root.children, [...kids.take(3), ...kids.skip(4)]);
      expect(other.children, [kids[3]]);
      kids[3].parent = null;
      expect(other.children, isEmpty);
      expect(() => root.children.add(other), throwsUnsupportedError);
    });

    test('a cycle throws and changes nothing; so does a parent from another world', () {
      final a = world.add(GameObject());
      final b = world.add(GameObject())..parent = a;
      final c = world.add(GameObject())..parent = b;
      expect(() => a.parent = c, throwsFynkel(FynkelStatus.invalidValue, "can't be the parent"));
      expect(() => a.parent = a, throwsFynkel(FynkelStatus.invalidValue));
      expect(a.parent, isNull);
      expect(c.parent, b);

      final elsewhere = World();
      addTearDown(elsewhere.dispose);
      final stranger = elsewhere.add(GameObject());
      expect(() => a.parent = stranger, throwsFynkel(FynkelStatus.invalidValue, 'another world'));
      expect(a.isPartOf(stranger), isFalse);
    });

    test('despawn takes the children along', () {
      final keep = world.add(GameObject());
      final root = world.add(GameObject())..parent = keep;
      final child = world.add(GameObject())..parent = root;
      final grandchild = world.add(GameObject())..parent = child;
      root.destroy();
      expect([root, child, grandchild].map((o) => o.isAlive), everyElement(isFalse));
      expect(keep.children, isEmpty);
      expect(() => grandchild.name, throwsFynkel(FynkelStatus.noObject));
    });

    test('isPartOf is true for the object itself and everything below it', () {
      final root = world.add(GameObject());
      final child = world.add(GameObject())..parent = root;
      final grandchild = world.add(GameObject())..parent = child;
      expect(root.isPartOf(root), isTrue);
      expect(grandchild.isPartOf(root), isTrue);
      expect(root.isPartOf(child), isFalse);
      expect(world.add(GameObject()).isPartOf(root), isFalse);
    });

    test('names are set, read, cleared and found depth first', () {
      final root = world.add(GameObject())..name = 'Bone';
      final arm = world.add(GameObject())
        ..parent = root
        ..name = 'Arm';
      final hand = world.add(GameObject())
        ..parent = arm
        ..name = 'Bone';
      final leg = world.add(GameObject())
        ..parent = root
        ..name = 'Leg';
      expect(root.find('Bone'), hand, reason: 'the root itself is not searched');
      expect(root.find('Leg'), leg);
      expect(root.find('Tail'), isNull);
      expect(hand.find('Bone'), isNull);

      // Longer than the first buffer, and not ASCII.
      final long = 'Größe · ${'ä' * 100} · 尾巴';
      leg.name = long;
      expect(leg.name, long);
      expect(root.find(long), leg);

      // A name set by the engine (as glTF names will be) can be longer than
      // the buffer Dart has so far, so reading it asks twice.
      final engineName = 'n' * (scratch.byteCapacity + 100);
      final bytes = engineName.toNativeUtf8();
      check(fy_name_set(world.pointer, leg.id, bytes.cast(), bytes.length));
      malloc.free(bytes);
      expect(leg.name, engineName);

      leg.name = null;
      expect(leg.name, isNull);
      arm.name = '';
      expect(arm.name, isNull, reason: 'an empty name is no name');
    });

    test('a child moves, turns and scales with its parent; its own transform stays as set', () {
      final parent = world.add(
        GameObject(
          transform: Transform(
            position: Vec3(1, 0, 0),
            rotation: Quat.axisAngle(Vec3.up, math.pi / 2),
            scale: Vec3.one * 2,
          ),
        ),
      );
      final child = world.add(GameObject(transform: Transform(position: Vec3(0, 0, -1))))..parent = parent;
      // One unit ahead (-Z), doubled, then turned a quarter to the left: two
      // units along -X from the parent.
      final t = child.worldTransform;
      expect(t.position.closeTo(Vec3(-1, 0, 0), 1e-5), isTrue, reason: '${t.position}');
      expect(t.rotation.closeTo(Quat.axisAngle(Vec3.up, math.pi / 2), 1e-5), isTrue);
      expect(t.scale.closeTo(Vec3.one * 2, 1e-5), isTrue);
      expect(child.transform.position, Vec3(0, 0, -1));

      parent.translate(Vec3(0, 3, 0));
      expect(child.worldTransform.position.closeTo(Vec3(-1, 3, 0), 1e-5), isTrue);
      expect(world.add(GameObject()).worldTransform, Transform(), reason: 'no transform and no parent: not moved');
    });
  });
}
