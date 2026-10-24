// A frame that changes a 10,000-vertex mesh allocates nothing in Dart: the
// game fills the same lists each frame, each `set` copies from them into
// the engine, and the frame's end gives the worlds the change
// (docs/stage-13.md phase 1). Counted by the VM service's allocation
// profile (see allocations.dart).

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';

void main() {
  final madeEachFrame = allocations();

  test('a frame that changes a 10,000-vertex dynamic mesh, in two worlds, allocates nothing', () async {
    final world = World(), other = World();
    addTearDown(world.dispose);
    addTearDown(other.dispose);
    final water = Water();
    world.add(water);
    other.add(GameObject(components: [MeshRenderer(water.mesh, water.material)]));
    expect(water.mesh.vertexCount, 100 * 100);

    expect(await madeEachFrame(world, 2000), isEmpty);
  });
}

/// A 100 × 100 grid of water rippling each frame: its heights and colours
/// in lists made once, its normals and tangents worked out by the engine.
class Water extends GameObject {
  static const n = 100;
  final mesh = Mesh()..markDynamic();
  final material = Material(color: Color(0xFF3388CC));
  final _positions = Float32List(n * n * 3);
  final _colors = Uint8List(n * n * 4);
  var _t = 0.0;

  @override
  void onLoad() {
    super.onLoad();
    final indices = Uint32List((n - 1) * (n - 1) * 6);
    for (var z = 0, k = 0; z < n - 1; z++) {
      for (var x = 0; x < n - 1; x++, k += 6) {
        final i = z * n + x;
        indices.setAll(k, [i, i + n, i + 1, i + 1, i + n, i + n + 1]);
      }
    }
    _ripple();
    mesh.setTriangles(indices);
    addComponent(MeshRenderer(mesh, material));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _t += 1 / 60;
    _ripple();
  }

  void _ripple() {
    for (var z = 0, i = 0; z < n; z++) {
      for (var x = 0; x < n; x++, i++) {
        final h = math.sin(x * 0.3 + _t * 2) * math.cos(z * 0.2 + _t) * 0.2;
        _positions[i * 3] = x * 0.1;
        _positions[i * 3 + 1] = h;
        _positions[i * 3 + 2] = z * 0.1;
        final shade = (128 + h * 600).toInt().clamp(0, 255);
        _colors[i * 4] = shade;
        _colors[i * 4 + 1] = shade;
        _colors[i * 4 + 2] = 255;
        _colors[i * 4 + 3] = 255;
      }
    }
    mesh.setPositions(_positions);
    mesh.setColors(_colors);
  }
}
