import 'dart:ffi';
import 'dart:io';
import 'dart:math' as math;

import 'package:ffi/ffi.dart';
import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/src/bindings.g.dart';
import 'package:test/test.dart';
import 'package:fynkel/src/world.dart' show WorldPointer;

/// The engine's own answers, read through its C interface, to compare with
/// what Dart keeps.
final class Engine {
  Engine(this.world);
  final World world;

  int parentOf(GameObject o) => using((arena) {
    final out = arena<Uint64>();
    expect(fy_parent_get(world.pointer, o.id, out), FyStatus.FY_STATUS_OK);
    return out.value;
  });

  List<int> childrenOf(GameObject o) => using((arena) {
    final ids = arena<Uint64>(256), count = arena<Size>();
    expect(fy_children_get(world.pointer, o.id, ids, 256, count), FyStatus.FY_STATUS_OK);
    return [for (var i = 0; i < count.value; i++) ids[i]];
  });

  bool shown(GameObject o) => using((arena) {
    final out = arena<Bool>();
    expect(fy_active_in_hierarchy_get(world.pointer, o.id, out), FyStatus.FY_STATUS_OK);
    return out.value;
  });

  bool partOf(GameObject o, GameObject other) => using((arena) {
    final out = arena<Bool>();
    expect(fy_is_part_of(world.pointer, o.id, other.id, out), FyStatus.FY_STATUS_OK);
    return out.value;
  });

  /// [o]'s world transform as the engine works it out (sending the moves
  /// first), from its matrix.
  Transform worldOf(GameObject o) => using((arena) {
    final id = arena<Uint64>()..value = o.id;
    final t = arena<Float>(10);
    world.readWorldTransforms(id, 1, t);
    return Transform(
      position: Vec3(t[0], t[1], t[2]),
      rotation: Quat(t[3], t[4], t[5], t[6]),
      scale: Vec3(t[7], t[8], t[9]),
    );
  });

  /// Every Dart answer about [objects] is the engine's.
  void agrees(List<GameObject> objects) {
    for (final o in objects) {
      expect(o.parent?.id ?? 0, parentOf(o), reason: 'the parent of $o');
      expect([for (final c in o.children) c.id], childrenOf(o), reason: 'the children of $o');
      expect(o.activeInHierarchy, shown(o), reason: '$o shown');
      for (final other in objects) {
        expect(o.isPartOf(other), partOf(o, other), reason: '$o part of $other');
      }
    }
  }
}

void main() {
  late World world;
  setUp(() => world = World());
  tearDown(() => world.dispose());

  test('what Dart keeps of the hierarchy is the engine\'s, through a thousand random changes', () {
    final engine = Engine(world);
    final random = math.Random(6);
    final objects = <GameObject>[for (var i = 0; i < 12; i++) world.add(GameObject())];
    for (var step = 0; step < 1000; step++) {
      final o = objects[random.nextInt(objects.length)];
      switch (random.nextInt(6)) {
        case 0 || 1:
          final parent = random.nextInt(4) == 0 ? null : objects[random.nextInt(objects.length)];
          // Refused, by both, when it would make a loop.
          try {
            o.parent = parent;
          } on ArgumentError {
            expect(parent!.isPartOf(o), isTrue);
          }
        case 2:
          o.active = !o.active;
        case 3:
          objects.add(o.add(GameObject()..active = random.nextBool()));
        case 4 when objects.length > 6:
          final gone = [
            for (final x in objects)
              if (x.isPartOf(o)) x,
          ];
          o.destroy();
          objects.removeWhere(gone.contains);
        default:
          objects.add(world.add(GameObject()));
      }
      if (step % 50 == 0) engine.agrees(objects);
    }
    engine.agrees(objects);
  });

  test('the world transforms Dart works out are the engine\'s, and parent = keeps them', () {
    final engine = Engine(world);
    final random = math.Random(5);
    double r() => random.nextDouble() * 4 - 2;
    Transform any() => Transform(
      position: Vec3(r(), r(), r()),
      rotation: Quat.axisAngle(Vec3(r(), r(), r() + 5).normalized, r()),
      scale: Vec3.one * (0.5 + random.nextDouble()),
    );
    final objects = <GameObject>[for (var i = 0; i < 12; i++) world.add(GameObject(transform: any()))];
    for (var step = 0; step < 300; step++) {
      final o = objects[random.nextInt(objects.length)];
      switch (random.nextInt(4)) {
        case 0:
          final parent = random.nextInt(4) == 0 ? null : objects[random.nextInt(objects.length)];
          if (parent != null && parent.isPartOf(o)) continue;
          final before = o.worldTransform;
          o.parent = parent;
          expect(o.position.closeTo(before.position, 1e-6), isTrue, reason: '$o stays put');
          expect(o.rotation.closeTo(before.rotation, 1e-6), isTrue, reason: '$o stays turned');
        case 1:
          objects.add(o.add(GameObject(transform: any())));
        case 2:
          o.transform = any();
        default:
          o.position = Vec3(r(), r(), r());
          o.rotation = Quat.axisAngle(Vec3.up, r());
      }
      if (step % 25 != 0) continue;
      for (final x in objects) {
        final dart = x.worldTransform, theirs = engine.worldOf(x);
        expect(
          dart.position.closeTo(theirs.position, 1e-4),
          isTrue,
          reason: '$x at ${dart.position}, ${theirs.position}',
        );
        expect(dart.rotation.closeTo(theirs.rotation, 1e-4), isTrue, reason: '$x turned');
        expect(dart.scale.closeTo(theirs.scale, 1e-4), isTrue, reason: '$x sized');
      }
    }
  });

  test("a model's parts are read once, as the engine has them, and change through Dart from then on", () async {
    final engine = Engine(world);
    final file = File.fromUri(Directory.current.uri.resolve('../../tests/assets/gltf/CesiumMilkTruck.glb'));
    final truck = await Model.load(file.readAsBytesSync());
    final copy = world.add(truck.instantiate());
    final parts = <GameObject>[copy];
    for (var i = 0; i < parts.length; i++) {
      parts.addAll(parts[i].children);
    }
    expect(parts.length, greaterThan(3));
    engine.agrees(parts);

    final wheel = parts.last;
    wheel.parent = copy;
    parts.first.children.first.active = false;
    engine.agrees(parts);
    expect(wheel.activeInHierarchy, isTrue);
  });

  group('the hierarchy while objects go', () {
    test("an object being destroyed can't leave its parent in onDestroy: it goes with it", () async {
      final errors = <ObjectError>[];
      world.errors.listen(errors.add);
      final player = world.add(_LetsGoOfWeapon());
      final weapon = player.add(GameObject(components: [MeshRenderer(Mesh.cube(), Material())]));
      player.destroy();
      world.update(1 / 60);
      await pumpEventQueue();
      expect(errors.single.error, isA<StateError>());
      expect(weapon.isDestroyed, isTrue);
      // The engine let it go too: a ray through where it was finds nothing.
      expect(world.raycast(Ray(Vec3(0, 0, 5), Vec3(0, 0, -1))), isNull);
    });

    test("nothing can be put under an object being destroyed, in onDestroy: it stays where it was", () async {
      final engine = Engine(world);
      final errors = <ObjectError>[];
      world.errors.listen(errors.add);
      final grabber = world.add(GameObject());
      final dying = world.add(_Grabs(grabber));
      dying.destroy();
      world.update(1 / 60);
      await pumpEventQueue();
      expect(errors.single.error, isA<StateError>());
      expect(grabber.isDestroyed, isFalse);
      expect(grabber.isAlive, isTrue);
      grabber.position = Vec3(1, 0, 0);
      world.update(1 / 60);
      engine.agrees([grabber]);
    });

    test('a child that moves itself out as it hides leaves its next sibling hidden too', () {
      final parent = world.add(GameObject());
      final leaves = parent.add(_LeavesOnDisable());
      final stays = parent.add(_CountsUpdates());
      world.update(1 / 60);
      parent.active = false;
      expect(leaves.parent, isNull);
      expect(stays.activeInHierarchy, isFalse);
      expect(stays.disabled, 1);
      final before = stays.updates;
      world.update(1 / 60);
      expect(stays.updates, before);
    });

    test("a parent scaled to 0 on one side still gives its children a rotation, Dart's", () {
      final parent = world.add(
        GameObject(
          transform: Transform(rotation: Quat.axisAngle(Vec3.up, 0.5), scale: Vec3(0, 1, 1)),
        ),
      );
      final child = parent.add(
        GameObject(
          transform: Transform(position: Vec3(1, 2, 3), rotation: Quat.axisAngle(Vec3.right, 0.3)),
        ),
      );
      world.update(1 / 60);
      final theirs = Engine(world).worldOf(child);
      expect(theirs.rotation.closeTo(child.worldTransform.rotation, 1e-4), isTrue, reason: '${theirs.rotation}');
      expect(theirs.position.closeTo(child.worldTransform.position, 1e-4), isTrue, reason: '${theirs.position}');
      expect(theirs.scale.closeTo(child.worldTransform.scale, 1e-4), isTrue, reason: '${theirs.scale}');
    });
  });

  test('getInChildren and getInParent see the children given before the object is added (DC7)', () {
    final light = Light.ambient();
    final child = GameObject(components: [light]);
    final top = GameObject(children: [child]);
    expect(top.getInChildren<Light>(), same(light));
    expect(child.getInParent<Light>(), same(light));
    final camera = Camera();
    final above = GameObject(components: [camera], children: [top]);
    expect(child.getInParent<Camera>(), same(camera), reason: 'all the way up, before any is added');
    final world = World();
    addTearDown(world.dispose);
    world.add(above);
    expect(top.getInChildren<Light>(), same(light));
    expect(child.getInParent<Camera>(), same(camera));
  });
}

class _LetsGoOfWeapon extends GameObject {
  @override
  void onDestroy() {
    super.onDestroy();
    children.single.parent = null;
  }
}

class _Grabs extends GameObject {
  _Grabs(this.other);
  final GameObject other;

  @override
  void onDestroy() {
    super.onDestroy();
    other.parent = this;
  }
}

class _LeavesOnDisable extends GameObject {
  var _left = false;

  @override
  void onDisable() {
    super.onDisable();
    if (_left) return;
    _left = true;
    parent = null;
  }
}

class _CountsUpdates extends GameObject {
  var updates = 0, disabled = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    updates++;
  }

  @override
  void onDisable() {
    super.onDisable();
    disabled++;
  }
}
