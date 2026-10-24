// Ground dug every frame allocates nothing (docs/stage-13.md, phase 1):
// its MeshCollider follows the mesh each step, read into memory physics
// keeps, counted as alloc_test.dart counts.

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

import 'allocations.dart';

/// A grid of [size] × [size] squares whose middle rises and falls each frame.
final class _Ground extends GameObject {
  _Ground(this.size);

  final int size;
  late final _positions = Float32List((size + 1) * (size + 1) * 3);
  late final mesh = Mesh();
  var _frame = 0;

  @override
  void onLoad() {
    super.onLoad();
    final n = size + 1;
    final indices = Uint32List(size * size * 6);
    var at = 0;
    for (var z = 0; z < size; z++) {
      for (var x = 0; x < size; x++) {
        final a = z * n + x;
        indices.setAll(at, [a, a + n, a + 1, a + 1, a + n, a + n + 1]);
        at += 6;
      }
    }
    _shape();
    mesh
      ..setPositions(_positions)
      ..setTriangles(indices)
      ..markDynamic();
    addComponent(MeshCollider(mesh: mesh));
  }

  void _shape() {
    final n = size + 1;
    final depth = (_frame % 20) * 0.05;
    for (var z = 0; z < n; z++) {
      for (var x = 0; x < n; x++) {
        final i = (z * n + x) * 3;
        _positions[i] = x - size / 2;
        _positions[i + 1] = (x - size ~/ 2).abs() < 2 && (z - size ~/ 2).abs() < 2 ? -depth : 0;
        _positions[i + 2] = z - size / 2;
      }
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _frame++;
    _shape();
    mesh.setPositions(_positions);
  }
}

/// Casts [ray] each frame.
final class _Probe extends Behaviour {
  _Probe(this.ray);

  final Ray ray;

  @override
  void onUpdate() {
    super.onUpdate();
    world.physics.raycast(ray);
  }
}

void main() {
  final madeEachFrame = allocations();

  test('a mesh collider that follows its mesh each frame allocates nothing', () async {
    final world = World();
    addTearDown(world.dispose);
    final ground = world.add(_Ground(16));
    // And a query each frame, which follows the mesh first; it misses, as
    // a hit makes its RaycastHit.
    world.add(GameObject(components: [_Probe(Ray(const Vec3(100, 5, 0), const Vec3(0, -1, 0)))]));
    world.update(1 / 60);
    final version = ground.mesh.version;
    expect(await madeEachFrame(world, 20000), isEmpty);
    expect(ground.mesh.version, greaterThan(version));
    expect(world.physics.raycast(Ray(const Vec3(0, 5, 0), const Vec3(0, -1, 0)))!.point.y, lessThanOrEqualTo(0));
  });
}
