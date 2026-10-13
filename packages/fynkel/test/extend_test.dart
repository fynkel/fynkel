// What package:fynkel/extend.dart gives a system that follows objects
// (docs/stage-5.md §3.1, step 3): the transform version, the moved log
// and the batched world transforms.

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

  test('an object\'s stamp moves with its own transform or parent set, and the hierarchy version with parents', () {
    final world = World();
    addTearDown(world.dispose);
    final a = world.add(GameObject());
    final b = world.add(GameObject());
    world.update(1 / 60);
    final (stampA, stampB) = (a.transformStamp, b.transformStamp);
    a.position = Vec3(1, 0, 0);
    expect(a.transformStamp, world.transformVersion);
    expect(b.transformStamp, stampB, reason: 'not set');
    expect(a.transformStamp, isNot(stampA));
    final shape = world.hierarchyVersion;
    b.layer = 3;
    a.name = 'a';
    expect(b.transformStamp, stampB, reason: 'a layer moves nothing');
    expect(world.hierarchyVersion, shape);
    b.parent = a;
    expect(b.transformStamp, world.transformVersion);
    expect(world.hierarchyVersion, isNot(shape));
  });

  test('a moved reader reads each object set since it last read, once while it waits, until it closes', () {
    final world = World();
    addTearDown(world.dispose);
    final (a, b, c) = (world.add(GameObject()), world.add(GameObject()), world.add(GameObject()));
    a.position = Vec3(1, 0, 0);
    final moved = world.movedReader();
    final other = world.movedReader();
    List<GameObject> read(MovedReader reader) {
      final objects = [for (var i = 0; i < reader.length; i++) reader[i]];
      reader.markRead();
      return objects;
    }

    expect(read(moved), isEmpty, reason: 'it starts reading where it was made');

    a.position = Vec3(2, 0, 0);
    a.position = Vec3(3, 0, 0);
    b.parent = a;
    c.layer = 4;
    a.name = 'a';
    a.active = false;
    world.update(1 / 60);
    expect(read(moved), [a, b, c], reason: 'a transform, a parent and a layer; a once, however often it moved');

    b.position = Vec3(0, 1, 0);
    a.position = Vec3(4, 0, 0);
    expect(read(moved), [b, a], reason: 'moved again after the reader read it');
    expect(read(other), [a, b, c, b, a], reason: 'the other reader hadn\'t read: it sees them all, some twice');

    expect(() => moved[0], throwsRangeError, reason: 'nothing moved since');
    other.close();
    expect(other.length, 0);
    b.position = Vec3.zero;
    expect(read(moved), [b]);
    expect(other.length, 0, reason: 'closed');
    moved.close();
    world.dispose();
    expect(world.movedReader, throwsStateError);
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
    expect(() => world.readWorldTransforms(ids, 3, out), throwsArgumentError);
  });
}
