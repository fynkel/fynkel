import 'dart:ffi';
import 'dart:math' as math;

import 'package:ffi/ffi.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/src/bindings.g.dart';
import 'package:fynkel/src/native.dart';
import 'package:test/test.dart';
import 'package:fynkel/src/world.dart' show WorldPointer;

Matcher throwsBadState(String message) => throwsA(isA<StateError>().having((e) => e.message, 'message', message));

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

void main() {
  late World world;
  setUp(() => world = World());
  tearDown(() => world.dispose());

  test("a destroyed object's id throws a clear error", () {
    final cube = world.add(GameObject(transform: Transform()));
    cube.destroy();
    expect(cube.isAlive, isFalse);
    expect(cube.isDestroyed, isTrue);
    expect(
      () => cube.transform,
      throwsBadState('GameObject#${cube.id} was destroyed, so its transform can\'t be read'),
    );
    expect(
      () => cube.transform = Transform(),
      throwsBadState('GameObject#${cube.id} was destroyed, so its transform can\'t be set'),
    );
    expect(
      () => cube.find('x'),
      throwsBadState('GameObject#${cube.id} was destroyed, so nothing can be found below it'),
    );
  });

  test('components come back as they went in', () {
    final transform = Transform(
      position: Vec3(1, 2, 3),
      rotation: Quat.axisAngle(Vec3.right, 0.5),
      scale: Vec3.one * 2,
    );
    final camera = Camera(fovY: 45.deg, near: 0.5, far: 50, clearColor: Color.rgb(0.25, 0.5, 0.75));
    final light = Light.ambient(color: Color.rgb(1, 0.5, 0), intensity: 0.25);
    final material = Material(color: Color(0xFFE0613A), metallic: 0.25, roughness: 0.75);
    final renderer = MeshRenderer(Mesh.sphere(), material);
    final o = world.add(GameObject(transform: transform, components: [camera, light, renderer]));

    expect(o.transform.position, transform.position);
    expect(o.transform.rotation.closeTo(transform.rotation), isTrue);
    expect(o.transform.scale, Vec3(2, 2, 2));
    expect(o.get<Camera>(), same(camera));
    expect(o.get<Light>(), same(light));
    expect(o.get<MeshRenderer>(), same(renderer));
    expect(o.get<MeshRenderer>()!.material, same(material));
  });

  test('a missing component is null, and removeComponent takes one away', () {
    final o = world.add(GameObject(transform: Transform(), components: [Light.sun()]));
    expect(o.get<Camera>(), isNull);
    final light = o.get<Light>()!;
    o.removeComponent(light);
    expect(o.get<Light>(), isNull);
    expect(() => o.removeComponent(light), throwsArgumentError);
  });

  test('an object refuses two components of one kind, and a refused component leaves no object behind', () {
    expect(() => GameObject(components: [Light.sun(), Light.ambient()]), throwsArgumentError);
    final before = world.add(GameObject());
    expect(
      () => world.add(
        GameObject(
          transform: Transform(),
          components: [Camera(fovY: 500.deg)],
        ),
      ),
      throwsRefusal(FynkelStatus.invalidValue, 'fov_y'),
    );
    final after = world.add(GameObject());
    for (var id = before.id + 1; id < after.id; id++) {
      expect(fy_exists(world.pointer, id), isFalse, reason: 'object#$id was despawned again');
    }
  });

  test('the engine refuses values that are not finite or not unit rotations', () {
    final o = world.add(GameObject());
    expect(() => o.transform = Transform(position: Vec3(double.nan, 0, 0)), throwsRefusal(FynkelStatus.invalidValue));
    expect(() => o.transform = Transform(rotation: Quat(0, 0, 0, 2)), throwsRefusal(FynkelStatus.invalidValue, 'unit'));
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
    o.translate(Vec3(1, 0, 0), relativeTo: Space.world);
    expect(o.transform.position, Vec3(1, 0, 0));
  });

  test('a disposed world refuses calls with a clear error, its objects are destroyed, and dispose twice is fine', () {
    final other = World();
    final o = other.add(GameObject(transform: Transform()));
    other.dispose();
    other.dispose();
    expect(o.isAlive, isFalse);
    expect(() => o.transform, throwsA(isA<StateError>().having((e) => e.message, 'message', contains('destroyed'))));
    expect(() => other.add(GameObject()), throwsStateError);
  });

  test('dispose destroys every object first: onDisable and onDestroy run, roots in order, children first', () {
    final other = World();
    final heard = <String>[];
    final a = other.add(_Noted('a', heard));
    a.add(_Noted('a.child', heard));
    final b = other.add(_Noted('b', heard));
    b.add(GameObject(components: [_NotedBehaviour('b.behaviour', heard)]));
    other.dispose();
    expect(heard, [
      'a.child disable',
      'a.child destroy',
      'a disable',
      'a destroy',
      'b.behaviour disable',
      'b.behaviour destroy',
      'b disable',
      'b destroy',
    ]);
    expect(a.isDestroyed, isTrue);
    expect(b.isDestroyed, isTrue);
  });

  test('an onDestroy that adds an object or disposes again, while its world is disposed, is fine', () {
    final other = World();
    final heard = <String>[];
    other.add(
      _Noted(
        'a',
        heard,
        onDestroyed: (o) {
          o.world.add(_Noted('late', heard));
          o.world.dispose();
        },
      ),
    );
    other.dispose();
    expect(heard, ['a disable', 'a destroy', 'late disable', 'late destroy']);
    expect(other.isDisposed, isTrue);
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
    expect(FynkelStatus.values.length, FyStatus.FY_STATUS_DEVICE_LOST + 1);
    expect(FynkelStatus.noObject.index, FyStatus.FY_STATUS_NO_OBJECT);
    expect(FynkelStatus.timeout.index, FyStatus.FY_STATUS_TIMEOUT);
    expect(FynkelStatus.poisoned.index, FyStatus.FY_STATUS_POISONED);
    expect(FynkelStatus.unsupported.index, FyStatus.FY_STATUS_UNSUPPORTED);
    expect(FynkelStatus.noTexture.index, FyStatus.FY_STATUS_NO_TEXTURE);
    expect(FynkelStatus.deviceLost.index, FyStatus.FY_STATUS_DEVICE_LOST);
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
      expect(() => a.parent = c, throwsRefusal(FynkelStatus.invalidValue, "can't be the parent"));
      expect(() => a.parent = a, throwsRefusal(FynkelStatus.invalidValue));
      expect(a.parent, isNull);
      expect(c.parent, b);

      final elsewhere = World();
      addTearDown(elsewhere.dispose);
      final stranger = elsewhere.add(GameObject());
      expect(() => a.parent = stranger, throwsRefusal(FynkelStatus.invalidValue, 'another world'));
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
      expect(
        () => grandchild.name,
        throwsBadState('GameObject#${grandchild.id} was destroyed, so its name can\'t be read'),
      );
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
      final child = world.add(GameObject(transform: Transform(position: Vec3(0, 0, -1))))
        ..setParent(parent, worldPositionStays: false);
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

class _Noted extends GameObject {
  _Noted(this.label, this.heard, {this.onDestroyed});
  final String label;
  final List<String> heard;
  final void Function(_Noted)? onDestroyed;

  @override
  void onDisable() {
    super.onDisable();
    heard.add('$label disable');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    heard.add('$label destroy');
    onDestroyed?.call(this);
  }
}

class _NotedBehaviour extends Behaviour {
  _NotedBehaviour(this.name, this.heard);
  final String name;
  final List<String> heard;

  @override
  void onDisable() {
    super.onDisable();
    heard.add('$name disable');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    heard.add('$name destroy');
  }
}
