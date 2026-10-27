part of 'world.dart';

/// A flat outline, with holes, for [Mesh.extrude] to make solid: a letter,
/// a window frame, a gear, a map's country. Points are in metres in the XY
/// plane; either way round is fine, for the outline and for each hole.
final class Shape2D {
  /// An outline through [outline]'s points, with [holes] cut out of it.
  /// Holes must lie inside the outline and not overlap; they may touch it,
  /// or each other, at a point, and the outline may touch itself. Filling
  /// it ([Mesh.extrude], a capped [Mesh.sweep]) refuses a hole outside.
  Shape2D(List<Vec2> outline, {List<List<Vec2>> holes = const []})
    : outline = List.unmodifiable(outline),
      holes = List.unmodifiable(holes.map(List<Vec2>.unmodifiable)) {
    if (outline.length < 3) throw ArgumentError.value(outline.length, 'outline.length', 'must be at least 3');
    for (final hole in holes) {
      if (hole.length < 3) throw ArgumentError.value(hole.length, 'hole.length', 'must be at least 3');
    }
  }

  /// A rectangle [width] by [height] around [centre].
  factory Shape2D.rect(double width, double height, {Vec2 centre = Vec2.zero}) {
    final (w, h) = (width / 2, height / 2);
    return Shape2D([
      Vec2(centre.x - w, centre.y - h),
      Vec2(centre.x + w, centre.y - h),
      Vec2(centre.x + w, centre.y + h),
      Vec2(centre.x - w, centre.y + h),
    ]);
  }

  /// A circle of [radius] around [centre], as [segments] straight sides.
  factory Shape2D.circle(double radius, {int segments = 32, Vec2 centre = Vec2.zero}) {
    if (segments < 3) throw ArgumentError.value(segments, 'segments', 'must be at least 3');
    return Shape2D([
      for (var i = 0; i < segments; i++)
        Vec2(
          centre.x + radius * math.cos(math.pi * 2 * i / segments),
          centre.y + radius * math.sin(math.pi * 2 * i / segments),
        ),
    ]);
  }

  /// A rectangle [width] by [height] around [centre] with its corners
  /// rounded to [radius], each corner [segments] straight pieces.
  factory Shape2D.rounded(double width, double height, double radius, {int segments = 6, Vec2 centre = Vec2.zero}) {
    if (segments < 1) throw ArgumentError.value(segments, 'segments', 'must be at least 1');
    final r = math.min(radius, math.min(width, height) / 2);
    final (w, h) = (width / 2 - r, height / 2 - r);
    final points = <Vec2>[];
    // Each corner's middle, and the angle its arc starts at.
    for (final (cx, cy, from) in [(w, -h, -math.pi / 2), (w, h, 0.0), (-w, h, math.pi / 2), (-w, -h, math.pi)]) {
      for (var i = 0; i <= segments; i++) {
        final a = from + math.pi / 2 * i / segments;
        points.add(Vec2(centre.x + cx + r * math.cos(a), centre.y + cy + r * math.sin(a)));
      }
    }
    return Shape2D(points);
  }

  /// An outline through [points]: [Shape2D()] without holes.
  factory Shape2D.polygon(List<Vec2> points) => Shape2D(points);

  /// Its outline's points.
  final List<Vec2> outline;

  /// Each hole's points.
  final List<List<Vec2>> holes;

  /// This shape with [hole]'s outline cut out of it as well.
  Shape2D withHole(Shape2D hole) => Shape2D(outline, holes: [...holes, hole.outline]);

  // Its points two floats each, the outline's then each hole's, and how
  // many each loop has: what the engine takes, made once.
  late final Float32List _packed = () {
    final out = Float32List(_sizes.fold(0, (n, size) => n + size) * 2);
    var i = 0;
    for (final loop in [outline, ...holes]) {
      for (final p in loop) {
        out[i++] = p.x;
        out[i++] = p.y;
      }
    }
    return out;
  }();
  late final Uint32List _sizes = Uint32List.fromList([outline.length, for (final hole in holes) hole.length]);

  @override
  String toString() => 'Shape2D(${outline.length} points, ${holes.length} holes)';
}

/// A mesh and where it goes, for [Mesh.combine] (Unity's
/// `CombineInstance`): its position, rotation and scale in the combined
/// mesh's space.
final class CombineInstance {
  /// [mesh] at [position], turned by [rotation] and stretched by [scale].
  const CombineInstance(this.mesh, {this.position = Vec3.zero, this.rotation = Quat.identity, this.scale = Vec3.one});

  /// The mesh to put in.
  final Mesh mesh;

  /// Where its origin goes.
  final Vec3 position;

  /// How it's turned.
  final Quat rotation;

  /// How much it's stretched along each of its own axes; a negative one
  /// mirrors it.
  final Vec3 scale;
}

/// Builds a mesh a vertex and a triangle at a time, in plain Dart: a
/// house, a fence, stairs, a level from a map. Then [build] makes a new
/// [Mesh] of it, or [applyTo] puts it in one there is, such as one rebuilt
/// each time something changes.
///
/// [addTriangle] and [addQuad] add their own corners, each face flat;
/// [addVertex] and [addIndices] share vertices, so the faces around one
/// are shaded smoothly across it (unless [flat]). [submesh] chooses which
/// part the triangles after it go in, each drawn with its own material.
/// [color] colours the vertices added after it.
///
/// It keeps its lists between builds: [clear] it and build again, and once
/// they're big enough nothing is made for the garbage collector to take.
final class MeshBuilder {
  /// A builder; with [flat], every triangle gets its own corners when it's
  /// built, so the mesh is faceted even where [addIndices] shares them.
  MeshBuilder({this.flat = false});

  /// Whether every triangle is built with its own corners.
  final bool flat;

  /// The colour of the vertices added from now on: white leaves the
  /// material's colour as it is.
  Color get color => _color;
  set color(Color value) {
    _color = value;
    value.rgba8(_rgba, 0);
  }

  Color _color = Color.white;
  // The colour as four bytes, worked out once rather than for each vertex.
  final _rgba = Uint8List(4)..fillRange(0, 4, 255);

  var _positions = Float32List(_start * 3);
  var _normals = Float32List(_start * 3);
  var _uvs = Float32List(_start * 2);
  var _colors = Uint8List(_start * 4);
  var _count = 0;
  // Every vertex has a normal of its own: the mesh is given them. One
  // without has NaN in [_normals]; with some of each, those are worked out
  // here into [_filled] and the rest kept.
  var _allNormals = true;
  var _someNormals = false;
  var _filled = Float32List(0);
  final _parts = <_Indices>[_Indices()];
  var _part = 0;

  // Flat copies and the parts one after another, kept for the next build.
  var _flatPositions = Float32List(0), _flatUvs = Float32List(0);
  var _flatColors = Uint8List(0);
  var _indices = Uint32List(0);
  var _starts = Uint32List(0);

  static const _start = 64;

  /// How many vertices it has.
  int get vertexCount => _count;

  /// How many triangles it has, in every part.
  int get triangleCount => _parts.fold(0, (n, part) => n + part.length ~/ 3);

  /// The part triangles go in from now on: 0 at first. The mesh has a
  /// part for each up to the highest chosen.
  void submesh(int part) {
    if (part < 0) throw RangeError.value(part, 'part', 'must be 0 or more');
    while (_parts.length <= part) {
      _parts.add(_Indices());
    }
    _part = part;
  }

  /// Adds a vertex and returns its index, for [addIndices]. Without a
  /// [normal], one is worked out from the faces around it; the normals
  /// given to the others, and [addTriangle]'s and [addQuad]'s, are kept.
  int addVertex(Vec3 position, {Vec3? normal, Vec2 uv = Vec2.zero}) {
    if (normal == null) {
      _allNormals = false;
      _nx = double.nan;
      _ny = double.nan;
      _nz = double.nan;
    } else {
      _someNormals = true;
      _nx = normal.x;
      _ny = normal.y;
      _nz = normal.z;
    }
    return _add(position, uv.x, uv.y);
  }

  /// Adds a triangle of vertices already added, counter-clockwise seen from
  /// its front.
  void addIndices(int a, int b, int c) {
    RangeError.checkValidIndex(a, null, 'a', _count);
    RangeError.checkValidIndex(b, null, 'b', _count);
    RangeError.checkValidIndex(c, null, 'c', _count);
    _parts[_part]
      ..add(a)
      ..add(b)
      ..add(c);
  }

  /// Adds a flat triangle with corners of its own, counter-clockwise seen
  /// from its front. The texture's bottom left is at [a], its bottom right
  /// at [b] and its top middle at [c].
  void addTriangle(Vec3 a, Vec3 b, Vec3 c) {
    _someNormals = true;
    _faceNormal(a, b, c);
    final i = _add(a, 0, 1);
    _add(b, 1, 1);
    _add(c, 0.5, 0);
    _parts[_part]
      ..add(i)
      ..add(i + 1)
      ..add(i + 2);
  }

  /// Adds a flat four-sided face with corners of its own, [a] to [d]
  /// counter-clockwise seen from its front, showing the whole texture: [a]
  /// is its bottom left, [b] bottom right, [c] top right, [d] top left.
  void addQuad(Vec3 a, Vec3 b, Vec3 c, Vec3 d) {
    _someNormals = true;
    _faceNormal(a, b, c);
    final i = _add(a, 0, 1);
    _add(b, 1, 1);
    _add(c, 1, 0);
    _add(d, 0, 0);
    _parts[_part]
      ..add(i)
      ..add(i + 1)
      ..add(i + 2)
      ..add(i)
      ..add(i + 2)
      ..add(i + 3);
  }

  /// Adds every triangle of [mesh], placed by [position], [rotation] and
  /// [scale], into the current part, its vertices coloured as it has them
  /// times [color]. Reads the mesh back, so it's for building, not for
  /// each frame.
  void addMesh(Mesh mesh, {Vec3 position = Vec3.zero, Quat rotation = Quat.identity, Vec3 scale = Vec3.one}) {
    final (p, n, uv, colors, indices) = (mesh.positions, mesh.normals, mesh.uvs, mesh.colors, mesh.indices);
    final base = _count;
    final tint = color;
    if (p.isNotEmpty) _someNormals = true;
    for (var i = 0; i < p.length ~/ 3; i++) {
      final at = rotation.rotate(Vec3(p[i * 3] * scale.x, p[i * 3 + 1] * scale.y, p[i * 3 + 2] * scale.z)) + position;
      // Normals stretch the other way from positions.
      final normal = rotation
          .rotate(Vec3(n[i * 3] / scale.x, n[i * 3 + 1] / scale.y, n[i * 3 + 2] / scale.z))
          .normalized;
      color = Color.rgb(
        tint.r * colors[i * 4] / 255,
        tint.g * colors[i * 4 + 1] / 255,
        tint.b * colors[i * 4 + 2] / 255,
        tint.a * colors[i * 4 + 3] / 255,
      );
      _nx = normal.x;
      _ny = normal.y;
      _nz = normal.z;
      _add(at, uv[i * 2], uv[i * 2 + 1]);
    }
    color = tint;
    final mirrored = scale.x * scale.y * scale.z < 0;
    final part = _parts[_part];
    for (var t = 0; t < indices.length; t += 3) {
      part
        ..add(base + indices[t])
        ..add(base + indices[t + (mirrored ? 2 : 1)])
        ..add(base + indices[t + (mirrored ? 1 : 2)]);
    }
  }

  /// Takes everything away to build again, keeping the room it has. The
  /// part goes back to 0 and the colour to white.
  void clear() {
    _count = 0;
    _allNormals = true;
    _someNormals = false;
    for (var k = 0; k < _parts.length; k++) {
      _parts[k].length = 0;
    }
    _part = 0;
    _color = Color.white;
    _rgba.fillRange(0, 4, 255);
  }

  /// A new mesh of what it has. Throws a [StateError] when it has no
  /// triangles.
  Mesh build() {
    if (triangleCount == 0) throw StateError('a MeshBuilder with no triangles has nothing to build');
    final mesh = Mesh();
    applyTo(mesh);
    return mesh;
  }

  /// Puts what it has into [mesh], in place of what that had, as one
  /// change: the worlds that have it draw it from the next frame, and a
  /// `MeshCollider` of it follows. An empty builder empties it.
  void applyTo(Mesh mesh) {
    mesh._changeable();
    final live = mesh._live;
    // The parts one after another, and where each after the first starts.
    var total = 0;
    for (var k = 0; k < _parts.length; k++) {
      total += _parts[k].length;
    }
    if (_indices.length < total) _indices = Uint32List(_grow(total));
    if (_starts.length < _parts.length - 1) _starts = Uint32List(_parts.length - 1);
    var at = 0;
    for (var k = 0; k < _parts.length; k++) {
      if (k > 0) _starts[k - 1] = at;
      final part = _parts[k];
      _indices.setRange(at, at + part.length, part.indices);
      at += part.length;
    }
    var positions = _positions, uvs = _uvs, colors = _colors, count = _count;
    if (flat) {
      // Each corner its own vertex, in triangle order.
      if (_flatPositions.length < total * 3) {
        _flatPositions = Float32List(_grow(total) * 3);
        _flatUvs = Float32List(_grow(total) * 2);
        _flatColors = Uint8List(_grow(total) * 4);
      }
      for (var i = 0; i < total; i++) {
        final v = _indices[i];
        _flatPositions.setRange(i * 3, i * 3 + 3, _positions, v * 3);
        _flatUvs.setRange(i * 2, i * 2 + 2, _uvs, v * 2);
        _flatColors.setRange(i * 4, i * 4 + 4, _colors, v * 4);
        _indices[i] = i;
      }
      positions = _flatPositions;
      uvs = _flatUvs;
      colors = _flatColors;
      count = total;
    }
    // Fewer vertices than now: no triangle may point past the last.
    check(fy_mesh_counts(live, scratch.count, scratch.count2));
    if (count < scratch.count.value) check(fy_mesh_set_triangles(live, nullptr, 0, nullptr, 0));
    check(fy_mesh_set_positions(live, positions.address, count));
    // Given every normal, or some: then the others are worked out here.
    final ownNormals = (_allNormals || _someNormals) && !flat;
    if (ownNormals) check(fy_mesh_set_normals(live, (_allNormals ? _normals : _fill(total)).address, count));
    check(fy_mesh_set_uvs(live, uvs.address, count));
    check(fy_mesh_set_colors(live, colors.address, count));
    check(fy_mesh_set_triangles(live, _indices.address, total, _starts.address, _parts.length - 1));
    mesh
      .._ownNormals = ownNormals
      .._ownTangents = false
      .._hasUvs = true
      .._staleNormals = !ownNormals
      .._staleTangents = true;
  }

  /// [_normals] with each one not given worked out from the faces around
  /// it, weighted by their area, as the engine does: into [_filled]. One
  /// no face with an area touches faces +Y.
  Float32List _fill(int total) {
    final n = _count * 3;
    if (_filled.length < n) _filled = Float32List(_grow(_count) * 3);
    final out = _filled, p = _positions;
    for (var i = 0; i < n; i++) {
      out[i] = _normals[i].isNaN ? 0 : _normals[i];
    }
    for (var t = 0; t + 2 < total; t += 3) {
      final a = _indices[t] * 3, b = _indices[t + 1] * 3, c = _indices[t + 2] * 3;
      final ux = p[b] - p[a], uy = p[b + 1] - p[a + 1], uz = p[b + 2] - p[a + 2];
      final vx = p[c] - p[a], vy = p[c + 1] - p[a + 1], vz = p[c + 2] - p[a + 2];
      final fx = uy * vz - uz * vy, fy = uz * vx - ux * vz, fz = ux * vy - uy * vx;
      _addFace(out, a, fx, fy, fz);
      _addFace(out, b, fx, fy, fz);
      _addFace(out, c, fx, fy, fz);
    }
    for (var v = 0; v < n; v += 3) {
      if (!_normals[v].isNaN) continue;
      final length = math.sqrt(out[v] * out[v] + out[v + 1] * out[v + 1] + out[v + 2] * out[v + 2]);
      if (length > 0 && length.isFinite) {
        out[v] /= length;
        out[v + 1] /= length;
        out[v + 2] /= length;
      } else {
        out[v] = 0;
        out[v + 1] = 1;
        out[v + 2] = 0;
      }
    }
    return out;
  }

  /// Adds the face fx, fy, fz to [out]'s normal at [v], if not given.
  @pragma('vm:prefer-inline')
  void _addFace(Float32List out, int v, double fx, double fy, double fz) {
    if (!_normals[v].isNaN) return;
    out[v] += fx;
    out[v + 1] += fy;
    out[v + 2] += fz;
  }

  /// Adds a vertex at [p] with the normal in [_nx], [_ny] and [_nz].
  @pragma('vm:prefer-inline')
  int _add(Vec3 p, double u, double v) {
    final i = _count;
    if (i == _positions.length ~/ 3) {
      final room = _grow(i + 1);
      _positions = Float32List(room * 3)..setAll(0, _positions);
      _normals = Float32List(room * 3)..setAll(0, _normals);
      _uvs = Float32List(room * 2)..setAll(0, _uvs);
      _colors = Uint8List(room * 4)..setAll(0, _colors);
    }
    _positions[i * 3] = p.x;
    _positions[i * 3 + 1] = p.y;
    _positions[i * 3 + 2] = p.z;
    _normals[i * 3] = _nx;
    _normals[i * 3 + 1] = _ny;
    _normals[i * 3 + 2] = _nz;
    _uvs[i * 2] = u;
    _uvs[i * 2 + 1] = v;
    _colors[i * 4] = _rgba[0];
    _colors[i * 4 + 1] = _rgba[1];
    _colors[i * 4 + 2] = _rgba[2];
    _colors[i * 4 + 3] = _rgba[3];
    _count = i + 1;
    return i;
  }

  // The normal of the vertices being added.
  double _nx = 0, _ny = 1, _nz = 0;

  /// Works out the unit normal of the triangle a, b, c into [_nx], [_ny]
  /// and [_nz]; up for one with no area.
  @pragma('vm:prefer-inline')
  void _faceNormal(Vec3 a, Vec3 b, Vec3 c) {
    final ux = b.x - a.x, uy = b.y - a.y, uz = b.z - a.z;
    final vx = c.x - a.x, vy = c.y - a.y, vz = c.z - a.z;
    final nx = uy * vz - uz * vy, ny = uz * vx - ux * vz, nz = ux * vy - uy * vx;
    final length = math.sqrt(nx * nx + ny * ny + nz * nz);
    if (length == 0) {
      _nx = 0;
      _ny = 1;
      _nz = 0;
    } else {
      _nx = nx / length;
      _ny = ny / length;
      _nz = nz / length;
    }
  }

  static int _grow(int atLeast) => math.max(atLeast, _start) * 3 ~/ 2;
}

/// One part's indices, in a list that grows.
final class _Indices {
  var indices = Uint32List(96);
  var length = 0;

  void add(int i) {
    if (length == indices.length) indices = Uint32List(indices.length * 2)..setAll(0, indices);
    indices[length++] = i;
  }
}
