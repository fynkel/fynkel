// What package:fynkel/extend.dart gives a system that follows objects
// (docs/stage-5.md §3.1, step 3): the transform version and the batched
// world transforms.

import 'dart:ffi';

import 'package:ffi/ffi.dart';
import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

void main() {
  test('the transform version goes up with a transform, a parent or a layer set, and only then', () {
    final world = World();
    addTearDown(world.dispose);
    final a = world.add(GameObject());
    final b = world.add(GameObject());
    world.update(1 / 60);
    var seen = world.transformVersion;
    void changed(String what, bool expected) {
      final now = world.transformVersion;
      expect(now != seen, expected, reason: what);
      seen = now;
    }

    world.update(1 / 60);
    a.worldTransform;
    a.transform;
    changed('frames and reads', false);
    a.position = Vec3(1, 2, 3);
    changed('a position', true);
    a.position = Vec3(1, 2, 4);
    changed('the same object again, before the moves are sent', true);
    b.parent = a;
    changed('a parent', true);
    b.layer = 5;
    changed('a layer', true);
    a.active = false;
    a.name = 'a';
    changed('active and name', false);
  });

  test('many world transforms are read in one call, after the moves are sent', () {
    final world = World();
    addTearDown(world.dispose);
    final parent = world.add(
      GameObject(
        transform: Transform(position: Vec3(1, 2, 3), rotation: Quat.axisAngle(Vec3.up, 0.7), scale: Vec3(1, 2, 3)),
      ),
    );
    final child = parent.add(GameObject(transform: Transform(position: Vec3(0, 1, -1))));
    final other = world.add(GameObject());
    other.position = Vec3(-4, 0, 0); // not sent yet
    final objects = [parent, child, other];
    final ids = malloc<Uint64>(3);
    final out = malloc<Float>(30);
    addTearDown(() {
      malloc
        ..free(ids)
        ..free(out);
    });
    for (final (i, o) in objects.indexed) {
      ids[i] = o.id;
    }
    world.readWorldTransforms(ids, 3, out);
    for (final (i, o) in objects.indexed) {
      final Transform(:position, :rotation, :scale) = o.worldTransform;
      final expected = [position.x, position.y, position.z, rotation.x, rotation.y, rotation.z, rotation.w];
      expected.addAll([scale.x, scale.y, scale.z]);
      for (var f = 0; f < 10; f++) {
        expect(out[i * 10 + f], closeTo(expected[f], 1e-6), reason: 'object $i, float $f');
      }
    }
    expect(out[20], -4, reason: 'the unsent move was sent first');

    child.destroy();
    world.update(1 / 60);
    expect(() => world.readWorldTransforms(ids, 3, out), throwsA(isA<FynkelException>()));
  });
}
