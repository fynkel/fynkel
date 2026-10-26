// Calls that can take milliseconds aren't leaf calls: a leaf call holds
// up garbage collection in every isolate while it runs (stage 14's BR3).
// Which are leaf is decided by tool/ffigen.dart, which says why sweeps
// and setPositions stay leaf; this reads what it made.

import 'dart:io';

import 'package:test/test.dart';

/// Each long call: a 2K sky (8 ms), shapes at 1024 segments (35–49 ms),
/// building from a shape, normals for a big mesh, a long simulate, and
/// checking a shader's WGSL.
const _long = {
  'fy_sky_image_decode',
  'fy_mesh_new_box',
  'fy_mesh_new_capsule',
  'fy_mesh_new_cone',
  'fy_mesh_new_cylinder',
  'fy_mesh_new_disc',
  'fy_mesh_new_grid',
  'fy_mesh_new_sphere',
  'fy_mesh_new_torus',
  'fy_mesh_extrude',
  'fy_mesh_lathe',
  'fy_mesh_combine',
  'fy_mesh_recalculate',
  'fy_particles_simulate',
  'fy_shader_new',
};

void main() {
  test('long native calls are not leaf calls', () {
    final bindings = File('lib/src/bindings.g.dart').readAsStringSync();
    final leaf = <String>[];
    for (final name in _long) {
      final at = bindings.indexOf('external int $name(');
      final atVoid = bindings.indexOf('external void $name(');
      final end = at >= 0 ? at : atVoid;
      expect(end, greaterThan(0), reason: '$name is in the bindings');
      final annotation = bindings.substring(bindings.lastIndexOf('@ffi.Native<', end), end);
      if (annotation.contains('isLeaf: true')) leaf.add(name);
    }
    expect(leaf, isEmpty, reason: 'add them to _blocking in tool/ffigen.dart');
  });
}
