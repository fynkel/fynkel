// Playtest games 2 and 4: a model's size, and where its pivot is in it,
// were found by reading the .glb by hand: game 4's fence, pivoted on one
// edge, swung a metre across the board when turned and scaled.

import 'dart:io';
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

Matcher near(Vec3 expected) => predicate<Vec3>((v) => (v - expected).length < 1e-5, 'near $expected');

void main() {
  test('a box lined up with the axes: its middle, size, corners, and what it holds', () {
    final box = Bounds(const Vec3(-1, 0, 2), const Vec3(3, 2, 4));
    expect((box.center, box.size), (const Vec3(1, 1, 3), const Vec3(4, 2, 2)));
    expect(box.corners, hasLength(8));
    expect(box.corners, containsAll([const Vec3(-1, 0, 2), const Vec3(3, 2, 4), const Vec3(3, 0, 2)]));
    expect(
      (box.contains(const Vec3(0, 1, 3)), box.contains(const Vec3(3, 2, 4)), box.contains(const Vec3(0, 3, 3))),
      (true, true, false),
    );
    expect(
      box.union(Bounds(const Vec3(5, -1, 3), const Vec3(6, 0, 3))),
      Bounds(const Vec3(-1, -1, 2), const Vec3(6, 2, 4)),
    );
    expect(
      Bounds.around([const Vec3(1, 5, -2), const Vec3(-3, 0, 7)]),
      Bounds(const Vec3(-3, 0, -2), const Vec3(1, 5, 7)),
    );
    expect(() => Bounds(const Vec3(1, 0, 0), Vec3.zero), throwsArgumentError);
    expect(() => Bounds.around([]), throwsArgumentError);
  });

  test("a mesh's bounds are the box round it, in its own space", () {
    expect(Mesh.cube().bounds, Bounds(const Vec3(-0.5, -0.5, -0.5), const Vec3(0.5, 0.5, 0.5)));
    final mesh = Mesh()..setPositions(Float32List.fromList([0, 0, 0, 2, 1, 0, 0, 0, -3]));
    expect(mesh.bounds, Bounds(const Vec3(0, 0, -3), const Vec3(2, 1, 0)));
    expect(Mesh().bounds, Bounds(Vec3.zero, Vec3.zero), reason: 'empty');
  });

  test("an object's bounds are the box in the world round what it and its children draw", () {
    final world = World();
    addTearDown(world.dispose);
    final material = Material();
    final root = world.add(GameObject(transform: Transform(position: const Vec3(10, 0, 0))));
    expect(root.bounds, isNull, reason: 'it draws nothing');
    // A 2 × 1 × 1 box turned a quarter about up: 1 × 1 × 2 in the world.
    root.add(
      GameObject(
        transform: Transform(
          position: const Vec3(0, 1, 0),
          rotation: Quat.axisAngle(Vec3.up, 1.5707963267948966),
          scale: const Vec3(2, 1, 1),
        ),
        components: [MeshRenderer(Mesh.cube(), material)],
      ),
    );
    final hidden = root.add(
      GameObject(
        transform: Transform(position: const Vec3(0, 50, 0)),
        components: [MeshRenderer(Mesh.cube(), material)],
      ),
    )..active = false;
    final box = root.bounds!;
    expect(box.min, near(const Vec3(9.5, 0.5, -1)));
    expect(box.max, near(const Vec3(10.5, 1.5, 1)));
    hidden.active = true;
    expect(root.bounds!.max.y, closeTo(50.5, 1e-5), reason: 'shown, it counts');
  });

  test("a model's copy has the bounds of its meshes, placed as it is", () async {
    final assets = Directory.current.uri.resolve('../../tests/assets/gltf/');
    final world = World(source: (name) => File.fromUri(assets.resolve(name)).readAsBytes());
    addTearDown(world.dispose);
    final copy = await world.assets.instantiate('Box.glb');
    world.add(copy).position = const Vec3(0, 3, 0);
    final box = copy.bounds!;
    expect(box.size, near(const Vec3(1, 1, 1)));
    expect(box.center, near(const Vec3(0, 3, 0)));
  });
}
