/// A mesh made in code (ADR-0026 §4): what `World.createMesh` draws.
library;

import 'dart:math' as math;
import 'dart:typed_data';

import '../math.dart';

/// A triangle mesh made in code (Unity's `new Mesh { vertices, triangles }`):
/// positions, a triangle list of indices into them, and optional
/// per-vertex normals, two uv sets, colours and tangents. Give it to
/// `World.createMesh` for an `AssetRef` that a `MeshRenderer`'s `mesh` (and
/// a `shape: mesh` `Collider`'s) takes like a cooked one.
///
/// Missing normals are computed smooth (each vertex the area-weighted
/// average of its triangles'; split the vertices for hard edges), missing
/// tangents from the first uv set when there is one, missing uvs are zero
/// and missing colours white. Triangles wind counter-clockwise seen from
/// their front, as glTF's do. Throws [ArgumentError] for an index out of
/// range, a triangle list that is not whole triangles, an attribute list
/// whose length is not the positions', or a value that is not finite.
class MeshData {
  MeshData({
    required List<Vec3> positions,
    required List<int> triangles,
    List<Vec3>? normals,
    List<Vec2>? uvs,
    List<Vec2>? uvs2,
    List<Vec4>? colors,
    List<Vec4>? tangents,
  })  : positions = List.unmodifiable(positions),
        triangles = List.unmodifiable(triangles),
        normals = normals == null ? null : List.unmodifiable(normals),
        uvs = uvs == null ? null : List.unmodifiable(uvs),
        uvs2 = uvs2 == null ? null : List.unmodifiable(uvs2),
        colors = colors == null ? null : List.unmodifiable(colors),
        tangents = tangents == null ? null : List.unmodifiable(tangents) {
    final n = positions.length;
    if (triangles.isEmpty || triangles.length % 3 != 0) {
      throw ArgumentError.value(triangles.length, 'triangles', 'needs whole triangles (a multiple of 3 indices, at least 3)');
    }
    for (final i in triangles) {
      if (i < 0 || i >= n) throw ArgumentError.value(i, 'triangles', 'index out of range for $n positions');
    }
    void same(List<Object>? list, String name) {
      if (list != null && list.length != n) {
        throw ArgumentError.value(list.length, name, 'needs one per position ($n)');
      }
    }

    same(normals, 'normals');
    same(uvs, 'uvs');
    same(uvs2, 'uvs2');
    same(colors, 'colors');
    same(tangents, 'tangents');
    final finite = positions.every((p) => p.isFinite) &&
        (normals?.every((v) => v.isFinite) ?? true) &&
        (uvs?.every((v) => v.isFinite) ?? true) &&
        (uvs2?.every((v) => v.isFinite) ?? true) &&
        (colors?.every((v) => v.isFinite) ?? true) &&
        (tangents?.every((v) => v.isFinite) ?? true);
    if (!finite) throw ArgumentError('MeshData: a value is not finite');
  }

  /// A flat grid in the XZ plane, centred on the origin, [width] × [depth]
  /// metres in [xSegments] × [zSegments] quads, facing up — with [height]
  /// giving each vertex its y (a height field: terrain, water). Uvs run
  /// 0…1 across it.
  factory MeshData.grid({
    double width = 1,
    double depth = 1,
    int xSegments = 1,
    int zSegments = 1,
    double Function(double x, double z)? height,
  }) {
    if (xSegments < 1 || zSegments < 1) {
      throw ArgumentError('MeshData.grid: at least one segment each way');
    }
    final positions = <Vec3>[];
    final uvs = <Vec2>[];
    for (var j = 0; j <= zSegments; j++) {
      for (var i = 0; i <= xSegments; i++) {
        final u = i / xSegments, v = j / zSegments;
        final x = (u - 0.5) * width, z = (v - 0.5) * depth;
        positions.add(Vec3(x, height?.call(x, z) ?? 0, z));
        uvs.add(Vec2(u, v));
      }
    }
    final triangles = <int>[];
    final row = xSegments + 1;
    for (var j = 0; j < zSegments; j++) {
      for (var i = 0; i < xSegments; i++) {
        final a = j * row + i, b = a + 1, c = a + row, d = c + 1;
        triangles.addAll([a, c, b, b, c, d]);
      }
    }
    return MeshData(positions: positions, triangles: triangles, uvs: uvs);
  }

  final List<Vec3> positions;

  /// Three indices into [positions] per triangle.
  final List<int> triangles;
  final List<Vec3>? normals;
  final List<Vec2>? uvs;
  final List<Vec2>? uvs2;

  /// Linear RGBA, multiplied into the material's base colour.
  final List<Vec4>? colors;

  /// xyz along +u, w the bitangent's handedness (±1): for normal maps.
  final List<Vec4>? tangents;

  /// A copy with the given lists replaced.
  MeshData copyWith({
    List<Vec3>? positions,
    List<int>? triangles,
    List<Vec3>? normals,
    List<Vec2>? uvs,
    List<Vec2>? uvs2,
    List<Vec4>? colors,
    List<Vec4>? tangents,
  }) =>
      MeshData(
        positions: positions ?? this.positions,
        triangles: triangles ?? this.triangles,
        normals: normals ?? this.normals,
        uvs: uvs ?? this.uvs,
        uvs2: uvs2 ?? this.uvs2,
        colors: colors ?? this.colors,
        tangents: tangents ?? this.tangents,
      );

  /// The axis-aligned bounds of [positions]: `(min, max)`.
  (Vec3, Vec3) get bounds {
    var (lx, ly, lz) = (double.infinity, double.infinity, double.infinity);
    var (hx, hy, hz) = (-double.infinity, -double.infinity, -double.infinity);
    for (final p in positions) {
      lx = math.min(lx, p.x);
      ly = math.min(ly, p.y);
      lz = math.min(lz, p.z);
      hx = math.max(hx, p.x);
      hy = math.max(hy, p.y);
      hz = math.max(hz, p.z);
    }
    return (Vec3(lx, ly, lz), Vec3(hx, hy, hz));
  }

  /// The normals the mesh is drawn with: [normals], or computed smooth.
  List<Vec3> resolvedNormals() {
    final given = normals;
    if (given != null) return given;
    final sum = List<Vec3>.filled(positions.length, Vec3.zero);
    for (var t = 0; t < triangles.length; t += 3) {
      final (a, b, c) = (triangles[t], triangles[t + 1], triangles[t + 2]);
      // Unnormalised: longer for a bigger triangle, so it weighs more.
      final face = (positions[b] - positions[a]).cross(positions[c] - positions[a]);
      sum[a] += face;
      sum[b] += face;
      sum[c] += face;
    }
    return [for (final n in sum) n.lengthSquared == 0 ? const Vec3(0, 1, 0) : n.normalized];
  }

  /// The tangents the mesh is drawn with: [tangents], computed from
  /// [uvs] and the normals, or zero without uvs.
  List<Vec4> resolvedTangents(List<Vec3> normals) {
    final given = tangents;
    if (given != null) return given;
    final uv = uvs;
    if (uv == null) return List.filled(positions.length, const Vec4(0, 0, 0, 0));
    final tan = List<Vec3>.filled(positions.length, Vec3.zero);
    final bitan = List<Vec3>.filled(positions.length, Vec3.zero);
    for (var t = 0; t < triangles.length; t += 3) {
      final (a, b, c) = (triangles[t], triangles[t + 1], triangles[t + 2]);
      final e1 = positions[b] - positions[a], e2 = positions[c] - positions[a];
      final d1 = uv[b] - uv[a], d2 = uv[c] - uv[a];
      final det = d1.x * d2.y - d2.x * d1.y;
      if (det == 0) continue;
      final r = 1 / det;
      final s = (e1 * d2.y - e2 * d1.y) * r;
      final u = (e2 * d1.x - e1 * d2.x) * r;
      for (final i in [a, b, c]) {
        tan[i] += s;
        bitan[i] += u;
      }
    }
    return [
      for (var i = 0; i < positions.length; i++)
        () {
          final n = normals[i];
          final t = tan[i] - n * n.dot(tan[i]);
          if (t.lengthSquared == 0) return const Vec4(0, 0, 0, 0);
          final tn = t.normalized;
          final w = n.cross(tn).dot(bitan[i]) < 0 ? -1.0 : 1.0;
          return Vec4(tn.x, tn.y, tn.z, w);
        }(),
    ];
  }

  /// The static `MESH` payload `fynkel_world_set_mesh` takes: u32 vertex
  /// count, u32 index count, six f32 bounds, 18 f32 per vertex (position,
  /// normal, tangent, uv0, uv1, colour), the u32 indices; little-endian.
  Uint8List toPayload() {
    const stride = 72;
    final n = positions.length;
    final out = ByteData(32 + n * stride + triangles.length * 4);
    out
      ..setUint32(0, n, Endian.little)
      ..setUint32(4, triangles.length, Endian.little);
    final (lo, hi) = bounds;
    var at = 8;
    void f(double v) {
      out.setFloat32(at, v, Endian.little);
      at += 4;
    }

    for (final v in [lo.x, lo.y, lo.z, hi.x, hi.y, hi.z]) {
      f(v);
    }
    final ns = resolvedNormals();
    final ts = resolvedTangents(ns);
    for (var i = 0; i < n; i++) {
      final p = positions[i], nn = ns[i], t = ts[i];
      final uv = uvs?[i] ?? Vec2.zero, uv2 = uvs2?[i] ?? Vec2.zero;
      final c = colors?[i] ?? const Vec4(1, 1, 1, 1);
      for (final v in [p.x, p.y, p.z, nn.x, nn.y, nn.z, t.x, t.y, t.z, t.w, uv.x, uv.y, uv2.x, uv2.y, c.x, c.y, c.z, c.w]) {
        f(v);
      }
    }
    for (final i in triangles) {
      out.setUint32(at, i, Endian.little);
      at += 4;
    }
    return out.buffer.asUint8List();
  }
}
