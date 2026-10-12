import 'dart:ffi';
import 'dart:io';
import 'dart:math' as math;

import 'package:ffi/ffi.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/src/bindings.g.dart';
import 'package:test/test.dart';

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
          } on FynkelException catch (e) {
            expect(e.status, FynkelStatus.invalidValue);
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
}
