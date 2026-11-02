part of 'world.dart';

/// A shape to draw: triangles, with a normal, tangent, texture coordinate
/// and colour at each vertex. A mesh can be used in any number of worlds;
/// each shares its data instead of copying it.
///
/// Each built-in shape is one [Mesh], so `Mesh.cube()` always returns the
/// same object. [Mesh.custom] and [Mesh.raw] make one from code; for a big
/// mesh, make it in [Mesh.inBackground]. A world keeps a mesh while your
/// code or an object's [MeshRenderer] can reach it, and lets it go once
/// it's garbage-collected; [dispose] frees it sooner.
///
/// **A mesh can change** while the game runs, as Unity's: make an empty
/// one with [Mesh()] (or change one made from code), then [setPositions],
/// [setTriangles] and the rest. Each call copies what it's given, so the
/// same lists can be filled again and passed each frame without garbage.
/// The changes of a frame go to the worlds that have the mesh together,
/// at the end of the frame. Until the game sets its own, the normals and
/// tangents are worked out again when the positions, UVs or triangles
/// change (the tangents once it has UVs). For a mesh that changes every frame, call [markDynamic].
final class Mesh implements Finalizable {
  Mesh._(this._pointer, this._name, {this._shape = false, bool ownNormals = true, bool ownTangents = true})
    // Private fields can't be named parameters.
    // ignore: prefer_initializing_formals
    : _ownNormals = ownNormals,
      // ignore: prefer_initializing_formals
      _ownTangents = ownTangents {
    _finalizer.attach(this, _pointer.cast(), detach: this);
  }

  /// An empty mesh, to fill with [setPositions] and [setTriangles] (Unity's
  /// `new Mesh()`). It draws nothing until it has triangles.
  factory Mesh() {
    ensureEngine();
    final out = scratch.mesh;
    check(fy_mesh_new_empty(out));
    return Mesh._(out.value, 'Mesh()', ownNormals: false, ownTangents: false).._hasUvs = false;
  }

  static final _finalizer = NativeFinalizer(
    Native.addressOf<NativeFunction<Void Function(Pointer<FyMesh>)>>(fy_mesh_free).cast(),
  );

  Pointer<FyMesh> _pointer;
  final String _name;
  final bool _shape;

  // Whether the game set the normals or tangents (or they came with the
  // mesh), or they're worked out by themselves; whether those worked out
  // are out of date.
  bool _ownNormals, _ownTangents;
  bool _staleNormals = false, _staleTangents = false;
  // Whether it has UVs: tangents are worked out from them, so not before.
  bool _hasUvs = true;

  // Changed since the worlds that have it were last given it; how many
  // times they've been given a change; whether it's dynamic.
  bool _changed = false;
  int _version = 0;
  // Its positions or triangles changed since then; how many times the
  // worlds have been given such a change.
  bool _reshaped = false;
  int _shapeVersion = 0;
  bool _dynamic = false;

  // The meshes changed this frame, for the worlds that have them.
  static final _changedMeshes = FrameList<Mesh>();

  final _release = _Release(_Resource.mesh);

  Pointer<FyMesh> get _live {
    if (_pointer == nullptr) throw StateError('$this was disposed');
    return _pointer;
  }

  /// Whether [dispose] was called; a disposed mesh can't be used in a
  /// world (a StateError).
  bool get isDisposed => _pointer == nullptr;

  /// A 1×1×1 cube around the origin. Each face shows the whole texture,
  /// upright on the four sides.
  factory Mesh.cube() => _cube;

  /// A sphere of radius 0.5 (720 triangles, as light as Unity's). The
  /// texture wraps around it once, with its top at the north pole.
  ///
  /// With any setting given, a new sphere of its own: [radius], [segments]
  /// slices around (3 or more) and [rings] bands from pole to pole (2 or
  /// more).
  factory Mesh.sphere({double? radius, int? segments, int? rings}) {
    if (radius == null && segments == null && rings == null) return _sphere;
    return _made('Mesh.sphere()', (out) => fy_mesh_new_sphere(radius ?? 0.5, segments ?? 24, rings ?? 16, out));
  }

  /// A capsule along Y, 2 high from end to end and of radius 0.5: the
  /// shape of a default `CapsuleCollider` or `CharacterController`, as Unity's. The
  /// texture wraps around it once, its top at the top.
  ///
  /// With any setting given, a new capsule of its own: [height] from end
  /// to end (less than twice the [radius] is a sphere), [segments] slices
  /// around and [rings] bands in each round end.
  factory Mesh.capsule({double? radius, double? height, int? segments, int? rings}) {
    if (radius == null && height == null && segments == null && rings == null) return _capsule;
    return _made(
      'Mesh.capsule()',
      (out) => fy_mesh_new_capsule(radius ?? 0.5, height ?? 2, segments ?? 24, rings ?? 6, out),
    );
  }

  /// A cylinder along Y, 2 high and of radius 0.5, with flat ends, as
  /// Unity's. The texture wraps around its side once.
  ///
  /// With any setting given, a new cylinder of its own: [radius],
  /// [height], [segments] slices around, and its flat ends unless [caps]
  /// is false (an open tube).
  factory Mesh.cylinder({double? radius, double? height, int? segments, bool? caps}) {
    if (radius == null && height == null && segments == null && caps == null) return _cylinder;
    return _made(
      'Mesh.cylinder()',
      (out) => fy_mesh_new_cylinder(radius ?? 0.5, height ?? 2, segments ?? 24, caps ?? true, out),
    );
  }

  /// A 1×1 plane facing +Y. Seen from above, the texture's top is towards
  /// -Z. Unity's Plane primitive is 10×10: this one is Unity's Quad,
  /// lying face up. Scale it for a ground, or make a [Mesh.grid].
  factory Mesh.plane() => _plane;

  /// A 1×1 square standing in XY, facing +Z: a camera looking along its
  /// forward (-Z) sees its front, the texture upright. Its back isn't
  /// drawn unless the material is [Material.doubleSided].
  factory Mesh.quad() => _quad;

  /// A box of [size] around the origin, each face cut into squares:
  /// [segmentsX], [segmentsY] and [segmentsZ] along each axis. Its faces
  /// show the texture as [Mesh.cube]'s do, but by the metre: [uvPerMetre]
  /// repeats each metre, so a long wall and a short one tile alike.
  factory Mesh.box({
    Vec3 size = Vec3.one,
    int segmentsX = 1,
    int segmentsY = 1,
    int segmentsZ = 1,
    double uvPerMetre = 1,
  }) {
    return _made('Mesh.box()', (out) {
      return fy_mesh_new_box(scratch.vec3.ref..write(size), segmentsX, segmentsY, segmentsZ, uvPerMetre, out);
    });
  }

  /// A flat grid [width] along X by [depth] along Z, facing up and cut
  /// into [segmentsX] by [segmentsZ] squares: a ground, or a mesh to bend
  /// (water, a flag). Textured as [Mesh.plane], by the metre.
  factory Mesh.grid({
    double width = 10,
    double depth = 10,
    int segmentsX = 10,
    int segmentsZ = 10,
    double uvPerMetre = 1,
  }) {
    return _made('Mesh.grid()', (out) => fy_mesh_new_grid(width, depth, segmentsX, segmentsZ, uvPerMetre, out));
  }

  /// A cone along Y around the origin, [height] high with its point at the
  /// top and a base of [radius]: [segments] slices around, and a flat base
  /// unless [cap] is false. The texture wraps around its side once, its
  /// top at the point.
  factory Mesh.cone({double radius = 0.5, double height = 1, int segments = 24, bool cap = true}) {
    return _made('Mesh.cone()', (out) => fy_mesh_new_cone(radius, height, segments, cap, out));
  }

  /// A ring around Y lying flat, as a doughnut: its tube's middle [radius]
  /// from the centre and the tube [tube] thick each way, with [segments]
  /// slices around Y and [sides] around the tube. The texture wraps once
  /// each way.
  factory Mesh.torus({double radius = 0.5, double tube = 0.15, int segments = 32, int sides = 16}) {
    return _made('Mesh.torus()', (out) => fy_mesh_new_torus(radius, tube, segments, sides, out));
  }

  /// A flat round disc of [radius] facing up, with [segments] around; a
  /// ring, with a hole of radius [inner], when that's more than 0. The
  /// texture lies across it as on [Mesh.plane].
  factory Mesh.disc({double radius = 0.5, double inner = 0, int segments = 32}) {
    return _made('Mesh.disc()', (out) => fy_mesh_new_disc(radius, inner, segments, out));
  }

  /// [shape] made solid: a slab [depth] thick along Z, around the origin,
  /// its front facing +Z, with [Shape2D.holes] going right through. The
  /// front and back show the texture flat, [uvPerMetre] repeats a metre;
  /// the sides wrap it along the outline. Corners sharper than 40° are
  /// sharp, the rest smooth, so a circle is round and a square square.
  /// Throws an [ArgumentError] for an outline of fewer than 3 points, or a
  /// hole that isn't inside the outline.
  factory Mesh.extrude(Shape2D shape, {double depth = 1, double uvPerMetre = 1}) {
    shape._checkSize('Mesh.extrude');
    final loops = [shape.outline, ...shape.holes];
    final count = loops.fold(0, (n, loop) => n + loop.length);
    return using((arena) {
      final points = arena<FyVec2>(count);
      final sizes = arena<Uint32>(loops.length);
      var i = 0;
      for (var k = 0; k < loops.length; k++) {
        sizes[k] = loops[k].length;
        for (final p in loops[k]) {
          points[i]
            ..x = p.x
            ..y = p.y;
          i++;
        }
      }
      return _made(
        'Mesh.extrude()',
        (out) => fy_mesh_extrude(points, count, sizes, loops.length, depth, uvPerMetre, out),
      );
    });
  }

  /// [profile] turned around Y with [segments] slices, as on a lathe: a
  /// vase, a bottle, a chess piece, a column. Each point is (distance from
  /// the Y axis, height). The outside is on the right of the way the
  /// profile goes, so a vase's goes from the middle of its base out, then
  /// up (and back down inside, for a thick wall). The texture wraps around
  /// once; corners sharper than 40° are sharp. Throws an [ArgumentError]
  /// for a profile all on the axis (x 0), which turns into no surface.
  factory Mesh.lathe(List<Vec2> profile, {int segments = 24}) {
    if (profile.length >= 2 && profile.every((p) => p.x == 0)) {
      throw ArgumentError(
        'Mesh.lathe: the profile is all on the axis (x = 0), so turning it makes no surface; each point is '
        '(distance from the Y axis, height)',
      );
    }
    return using((arena) {
      final points = arena<FyVec2>(profile.length);
      for (var i = 0; i < profile.length; i++) {
        points[i]
          ..x = profile[i].x
          ..y = profile[i].y;
      }
      return _made('Mesh.lathe()', (out) => fy_mesh_lathe(points, profile.length, segments, out));
    });
  }

  /// [text] in [font] made solid, as Unity's TextMesh but with depth: each
  /// letter a slab [depth] thick along Z, its front facing +Z, with the
  /// holes the font gives it (an o's middle). [size] is the font's em in
  /// metres (capitals are about 0.7 of it). The first line's letters stand
  /// on the X axis, `\n` starts a line [lineSpacing] times the font's line
  /// height below, and [align] puts each line's start, middle or end on
  /// the origin. Letters are kerned as the font says. The front and back
  /// show the texture flat across the letters, [uvPerMetre] repeats a
  /// metre. Text with nothing to draw (spaces) is an empty mesh; a letter
  /// the font hasn't got is its missing-letter box.
  ///
  /// For text that changes (a score), make a new mesh and give it to the
  /// [MeshRenderer]: a line of text takes a fraction of a millisecond.
  factory Mesh.text(
    String text, {
    required Font font,
    double size = 1,
    double depth = 0.2,
    TextAlignment align = TextAlignment.center,
    double lineSpacing = 1,
    double uvPerMetre = 1,
  }) {
    final shown = text.length > 24 ? '${text.substring(0, 24)}…' : text;
    return using((arena) {
      final utf8Text = utf8.encode(text);
      final chars = Font._utf8In(arena, utf8Text);
      return _made(
        'Mesh.text(${jsonEncode(shown)})',
        (out) => fy_mesh_text(
          font._bytes,
          font._length,
          chars,
          utf8Text.length,
          size,
          align._native,
          lineSpacing,
          depth,
          uvPerMetre,
          out,
        ),
      );
    });
  }

  /// [shape] swept along [path], as Unity's `SplineExtrude`: a road, a
  /// rope, a pipe, a rail. The shape is drawn across the path all along
  /// it, its X to the right and Y up seen going along, and its edges are
  /// joined from each sample to the next; a shape with holes is a pipe.
  ///
  /// The path is cut into [segmentsPerMetre] samples a metre and more
  /// where it bends (one every 8°), at most [maxSegments]. [caps] fills
  /// its ends (a closed spline has none). The texture repeats [uvPerMetre]
  /// times a metre around the shape (u) and along the path (v). Corners of
  /// the shape sharper than 40° are sharp, the rest smooth. For a path
  /// that changes, sweep again into the same mesh with [setSweep]. Throws
  /// an [ArgumentError] for a path with no length, or, with [caps], a hole
  /// that isn't inside the outline.
  factory Mesh.sweep(
    Spline path,
    Shape2D shape, {
    double segmentsPerMetre = 2,
    int maxSegments = 2000,
    bool caps = true,
    double uvPerMetre = 1,
  }) {
    shape._checkSize('Mesh.sweep');
    _sampled(path, segmentsPerMetre, maxSegments);
    final packed = shape._packed, sizes = shape._sizes;
    final count = path._pathCount, closed = path.closed;
    final mesh = _made(
      'Mesh.sweep()',
      (out) => fy_mesh_sweep(
        path._path.address,
        count,
        packed.address,
        packed.length ~/ 2,
        sizes.address,
        sizes.length,
        caps && !closed,
        uvPerMetre,
        out,
      ),
    );
    return mesh;
  }

  /// [Mesh.sweep] again, into this mesh in place of what it had: a rope
  /// that swings, a road whose knots are dragged. As the other changes, it
  /// goes to the worlds that have it at the end of the frame; mark a mesh
  /// swept every frame dynamic ([markDynamic]). With the same number of
  /// samples and the same shape the triangles are kept, and filling the
  /// spline with [Spline.setPositions] each frame makes no garbage.
  // Inlined where it's called, so a JIT build (the tests) passes its
  // settings on unboxed before it would have optimised this by itself.
  @pragma('vm:prefer-inline')
  void setSweep(
    Spline path,
    Shape2D shape, {
    double segmentsPerMetre = 2,
    int maxSegments = 2000,
    bool caps = true,
    double uvPerMetre = 1,
  }) {
    _reshape();
    shape._checkSize('setSweep');
    // The setting through a field, not passed on: a JIT build boxes a
    // double passed to a call, or kept for a throw. (It boxes one passed
    // to the engine too, unless it's a constant; a release build doesn't.)
    path._perMetre = segmentsPerMetre;
    _checkSweep(path, maxSegments);
    path._sample(maxSegments);
    final packed = shape._packed, sizes = shape._sizes;
    check(
      fy_mesh_set_sweep(
        _live,
        path._path.address,
        path._pathCount,
        packed.address,
        packed.length ~/ 2,
        sizes.address,
        sizes.length,
        caps && !path.closed,
        uvPerMetre,
      ),
    );
    // Its normals, tangents and UVs are the sweep's.
    _ownNormals = _ownTangents = _hasUvs = true;
    _staleNormals = _staleTangents = false;
  }

  /// [path]'s samples, after checking the settings.
  static void _sampled(Spline path, double segmentsPerMetre, int maxSegments) {
    ensureEngine();
    path._perMetre = segmentsPerMetre;
    _checkSweep(path, maxSegments);
    path._sample(maxSegments);
  }

  /// Throws for a sweep's settings that make no samples: [path]'s
  /// [Spline._perMetre] and [maxSegments].
  static void _checkSweep(Spline path, int maxSegments) {
    final perMetre = path._perMetre;
    if (!(perMetre > 0 && perMetre < double.infinity)) {
      throw ArgumentError.value(perMetre, 'segmentsPerMetre', 'must be more than 0');
    }
    if (maxSegments < 1) throw ArgumentError.value(maxSegments, 'maxSegments', 'must be at least 1');
  }

  /// [parts] as one mesh, each placed by its position, rotation and scale
  /// (Unity's `Mesh.CombineMeshes`): many things that never move apart,
  /// such as the houses of a town or the rocks of a field, drawn with one
  /// draw for each material instead of one each. Part n of each mesh goes
  /// into part n of the result, so houses with walls (0) and roofs (1)
  /// make a town that's two draws, with `MeshRenderer.materials` for each.
  /// Throws an [ArgumentError] for a skinned mesh, or for none.
  factory Mesh.combine(List<CombineInstance> parts) {
    if (parts.isEmpty) throw ArgumentError('Mesh.combine: parts is empty; it needs at least one mesh');
    return using((arena) {
      final meshes = arena<Pointer<FyMesh>>(parts.length);
      final transforms = arena<FyTransform>(parts.length);
      for (var i = 0; i < parts.length; i++) {
        final part = parts[i];
        part.mesh._settle();
        meshes[i] = part.mesh._live;
        transforms[i].position.write(part.position);
        transforms[i].rotation.write(part.rotation);
        transforms[i].scale.write(part.scale);
      }
      return _made('Mesh.combine()', (out) => fy_mesh_combine(meshes.cast(), transforms, parts.length, out));
    });
  }

  /// A mesh made by [make], with its normals and tangents its own.
  static Mesh _made(String name, int Function(Pointer<Pointer<FyMesh>> out) make) {
    ensureEngine();
    final out = scratch.mesh;
    check(make(out));
    return Mesh._(out.value, name);
  }

  static final _cube = Mesh._shape(FY_MESH_CUBE, 'Mesh.cube()');
  static final _sphere = Mesh._shape(FY_MESH_SPHERE, 'Mesh.sphere()');
  static final _capsule = Mesh._shape(FY_MESH_CAPSULE, 'Mesh.capsule()');
  static final _cylinder = Mesh._shape(FY_MESH_CYLINDER, 'Mesh.cylinder()');
  static final _plane = Mesh._shape(FY_MESH_PLANE, 'Mesh.plane()');
  static final _quad = Mesh._shape(FY_MESH_QUAD, 'Mesh.quad()');

  factory Mesh._shape(int shape, String name) {
    ensureEngine();
    final out = scratch.mesh;
    check(fy_mesh_new_shape(shape, out));
    return Mesh._(out.value, name, shape: true);
  }

  /// A mesh from code. Each triangle is three [indices] into [positions],
  /// counter-clockwise seen from its front.
  ///
  /// - Without [normals], each vertex gets the average of the faces around
  ///   it, weighted by their angle there. So triangles that share a vertex
  ///   are shaded smoothly across it, and a sharp edge needs a vertex for
  ///   each side.
  /// - [uvs] are texture coordinates, (0, 0) at the image's top-left.
  ///   Without them, every vertex has (0, 0).
  /// - Tangents (for normal maps) are generated from the UVs with
  ///   MikkTSpace, as glTF does. That can add a vertex where a texture is
  ///   mirrored. Give your own with [Mesh.raw].
  ///
  /// Throws an [ArgumentError] for a broken mesh, saying what's wrong. The lists are copied.
  factory Mesh.custom({
    required List<Vec3> positions,
    List<Vec3>? normals,
    List<Vec2>? uvs,
    required List<int> indices,
  }) {
    Float32List flat3(List<Vec3> vs) {
      final out = Float32List(vs.length * 3);
      for (var i = 0; i < vs.length; i++) {
        out[i * 3] = vs[i].x;
        out[i * 3 + 1] = vs[i].y;
        out[i * 3 + 2] = vs[i].z;
      }
      return out;
    }

    for (final (list, name) in [(normals, 'normals'), (uvs, 'uvs')]) {
      if (list != null && list.length != positions.length) {
        throw ArgumentError('$name has ${list.length}; it needs one for each of the ${positions.length} positions');
      }
    }

    Float32List? flat2(List<Vec2>? vs) {
      if (vs == null) return null;
      final out = Float32List(vs.length * 2);
      for (var i = 0; i < vs.length; i++) {
        out[i * 2] = vs[i].x;
        out[i * 2 + 1] = vs[i].y;
      }
      return out;
    }

    return Mesh.raw(
      flat3(positions),
      normals: normals == null ? null : flat3(normals),
      uvs: flat2(uvs),
      indices: indices is Uint32List ? indices : Uint32List.fromList(indices),
    );
  }

  /// [Mesh.custom] from packed lists, for meshes too big for a list of
  /// [Vec3]s: 3 floats per position and normal, 4 per tangent (a direction,
  /// then 1 or -1 for the side its bitangent is on), 2 per UV. A tangent
  /// points where u grows; `cross(normal, tangent) * w` points up the
  /// image.
  factory Mesh.raw(
    Float32List positions, {
    Float32List? normals,
    Float32List? tangents,
    Float32List? uvs,
    required Uint32List indices,
  }) {
    ensureEngine();
    if (positions.length % 3 != 0) {
      throw ArgumentError.value(positions.length, 'positions.length', 'must be a multiple of 3 (x, y, z)');
    }
    final count = positions.length ~/ 3;
    for (final (list, per, name) in [(normals, 3, 'normals'), (tangents, 4, 'tangents'), (uvs, 2, 'uvs')]) {
      if (list != null && list.length != count * per) {
        throw ArgumentError.value(list.length, '$name.length', 'must be $per per position: ${count * per}');
      }
    }
    return using((arena) {
      Pointer<Float> copy(Float32List? list) {
        if (list == null) return nullptr;
        final p = arena<Float>(list.length);
        p.asTypedList(list.length).setAll(0, list);
        return p;
      }

      final data = arena<FyMeshData>();
      data.ref
        ..positions = copy(positions)
        ..normals = copy(normals)
        ..tangents = copy(tangents)
        ..uvs = copy(uvs)
        ..vertex_count = count
        ..indices = arena<Uint32>(indices.length)
        ..index_count = indices.length;
      data.ref.indices.asTypedList(indices.length).setAll(0, indices);
      final out = arena<Pointer<FyMesh>>();
      check(fy_mesh_new(data, out));
      return Mesh._(out.value, 'Mesh.custom', ownNormals: normals != null, ownTangents: tangents != null)
        .._hasUvs = uvs != null;
    });
  }

  /// Runs [build] (such as `() => Mesh.custom(...)`) on a helper isolate,
  /// so a big mesh doesn't hold up the UI thread: generating tangents
  /// takes about a microsecond per triangle.
  static Future<Mesh> inBackground(Mesh Function() build) async {
    final (address, name, (ownNormals, ownTangents), (hasUvs, dynamic)) = await inHelper(() {
      final mesh = build();
      mesh._settle();
      // This isolate ends next; the mesh now belongs to the caller's.
      _finalizer.detach(mesh);
      return (mesh._pointer.address, mesh._name, (mesh._ownNormals, mesh._ownTangents), (mesh._hasUvs, mesh._dynamic));
    });
    return Mesh._(Pointer<FyMesh>.fromAddress(address), name, ownNormals: ownNormals, ownTangents: ownTangents)
      .._hasUvs = hasUvs
      .._dynamic = dynamic;
  }

  // --- Changing it ---

  /// The vertices' positions, 3 floats each (x, y, z), in the mesh's own
  /// space (Unity's `SetVertices`). A different count from the mesh's
  /// resets the UVs to (0, 0) and the colours to white, and needs
  /// triangles that point at no vertex past the last: set fewer triangles
  /// first, or [clear]. Throws an [ArgumentError], changing nothing, for a
  /// length that isn't a multiple of 3 or a value that isn't finite.
  void setPositions(Float32List positions) {
    _reshape();
    if (positions.length % 3 != 0) {
      throw ArgumentError.value(positions.length, 'positions.length', 'must be a multiple of 3 (x, y, z)');
    }
    check(fy_mesh_set_positions(_live, positions.address, positions.length ~/ 3));
    _staleNormals = !_ownNormals;
    _staleTangents = !_ownTangents;
  }

  /// The vertices' normals, 3 floats each, one per vertex; each is made
  /// unit length. From then on they're the game's: changing the positions
  /// no longer works them out again (call [recalculateNormals]).
  void setNormals(Float32List normals) {
    _changeable();
    check(fy_mesh_set_normals(_live, normals.address, _per(normals.length, 3, 'normals')));
    _ownNormals = true;
    _staleNormals = false;
    _staleTangents = !_ownTangents;
  }

  /// The vertices' tangents, for normal maps: 4 floats each, a direction
  /// along u then 1 or -1 (see [Mesh.raw]). From then on they're the
  /// game's, as [setNormals].
  void setTangents(Float32List tangents) {
    _changeable();
    check(fy_mesh_set_tangents(_live, tangents.address, _per(tangents.length, 4, 'tangents')));
    _ownTangents = true;
    _staleTangents = false;
  }

  /// The vertices' texture coordinates, 2 floats each, (0, 0) the image's
  /// top-left (Unity's `SetUVs`).
  void setUvs(Float32List uvs) {
    _changeable();
    check(fy_mesh_set_uvs(_live, uvs.address, _per(uvs.length, 2, 'uvs')));
    _hasUvs = true;
    _staleTangents = !_ownTangents;
  }

  /// The vertices' colours, 4 bytes each: red, green and blue as a colour
  /// picker gives them (sRGB, 0–255), then alpha (Unity's `SetColors`
  /// with `Color32`). They multiply the material's colour, so white, as
  /// at first, leaves it as it is. [Color.rgba8] fills four bytes from a
  /// [Color].
  void setColors(Uint8List colors) {
    _changeable();
    check(fy_mesh_set_colors(_live, colors.address, _per(colors.length, 4, 'colors')));
  }

  /// The triangles: three indices into the positions each,
  /// counter-clockwise seen from the front (Unity's `SetTriangles`).
  ///
  /// For a mesh of several parts, each drawn with its own material
  /// ([MeshRenderer.materials]), give every part's triangles one after
  /// another and, in [submeshStarts], where each part after the first
  /// starts: `[0, 1, 2, 2, 3, 0]` with `submeshStarts: [3]` is two parts
  /// of a triangle each. None is allowed: the mesh then draws nothing.
  /// Throws an [ArgumentError], changing nothing, for an index past the
  /// last vertex or a start out of order or not on a triangle.
  void setTriangles(Uint32List indices, {Uint32List? submeshStarts}) {
    _reshape();
    final starts = submeshStarts ?? _noStarts;
    check(fy_mesh_set_triangles(_live, indices.address, indices.length, starts.address, starts.length));
    _staleNormals = !_ownNormals;
    _staleTangents = !_ownTangents;
  }

  static final _noStarts = Uint32List(0);

  /// Takes every vertex and triangle away (Unity's `Clear`): the mesh draws
  /// nothing until it's given more. Its normals and tangents are worked
  /// out by themselves again.
  void clear() {
    _reshape();
    check(fy_mesh_set_triangles(_live, nullptr, 0, nullptr, 0));
    check(fy_mesh_set_positions(_live, nullptr, 0));
    _ownNormals = _ownTangents = _staleNormals = _staleTangents = _hasUvs = false;
  }

  /// Works out each vertex's normal from the faces around it, weighted by
  /// their size (Unity's `RecalculateNormals`). Quicker than the angle
  /// weighting [Mesh.custom] uses, for a mesh that changes each frame.
  /// Not needed while the game hasn't set normals of its own.
  void recalculateNormals() {
    _changeable();
    check(fy_mesh_recalculate(_live, FY_MESH_NORMALS));
    _staleNormals = false;
  }

  /// Works out each vertex's tangent from its UVs, for normal maps
  /// (Unity's `RecalculateTangents`), without splitting any vertex: where
  /// a texture is mirrored the two sides average, which [Mesh.custom]'s
  /// MikkTSpace splits instead.
  void recalculateTangents() {
    _changeable();
    check(fy_mesh_recalculate(_live, FY_MESH_TANGENTS));
    _staleTangents = false;
  }

  /// Says the mesh changes often, as Unity's `MarkDynamic`: each view
  /// keeps it on the GPU with room to grow (half again) and writes a change
  /// where it is, rather than making it again. For a mesh changed every
  /// frame: a flag, water, ground being dug. Its outline, if its material
  /// has one, follows its own normals.
  void markDynamic() {
    _changeable();
    if (_dynamic) return;
    check(fy_mesh_mark_dynamic(_live));
    _dynamic = true;
  }

  /// Whether [markDynamic] was called.
  bool get isDynamic => _dynamic;

  void _changeable() {
    if (_shape) throw StateError('$this is built in and can\'t change; make a Mesh() of your own');
    _live;
    if (!_changed) {
      _changed = true;
      _changedMeshes.add(this);
    }
  }

  /// [_changeable], for a change to its positions or triangles: what a
  /// collider made of it collides as.
  void _reshape() {
    _changeable();
    _reshaped = true;
  }

  int _per(int length, int per, String name) {
    if (length % per != 0) throw ArgumentError.value(length, '$name.length', 'must be a multiple of $per');
    return length ~/ per;
  }

  /// Works out the normals and tangents out of date. [later] owes the
  /// normals instead, when no tangents need them now: each view works
  /// them out as it uploads the mesh, and reading them does, so a mesh
  /// changed every frame doesn't cost the game's thread its normals
  /// (stage 16's phase 4: 0.8 ms on the Note10+ for the lab's water).
  void _settle({bool later = false}) {
    if (!_staleNormals && !_staleTangents) return;
    // Without UVs tangents mean nothing: they wait for them.
    final tangents = _staleTangents && _hasUvs;
    final normals = !_staleNormals ? 0 : (later && !tangents ? FY_MESH_NORMALS_LATER : FY_MESH_NORMALS);
    final what = normals | (tangents ? FY_MESH_TANGENTS : 0);
    _staleNormals = _staleTangents = false;
    if (what != 0) check(fy_mesh_recalculate(_live, what));
  }

  /// Gives every changed mesh to the worlds that have it, at the end of a
  /// frame.
  static void _publishChanges() {
    final changed = _changedMeshes;
    if (changed.isEmpty) return;
    for (var i = 0; i < changed.length; i++) {
      final mesh = changed[i];
      mesh._changed = false;
      if (mesh.isDisposed) continue;
      mesh._settle(later: true);
      mesh._version++;
      if (mesh._reshaped) {
        mesh._reshaped = false;
        mesh._shapeVersion++;
      }
      mesh._release.updateMesh(mesh);
    }
    changed.clear();
  }

  /// How many vertices the mesh has. Can be more than were given, when
  /// generated tangents split some.
  int get vertexCount {
    _settle();
    check(fy_mesh_counts(_live, scratch.count, scratch.count2));
    return scratch.count.value;
  }

  /// Three per triangle.
  int get indexCount {
    check(fy_mesh_counts(_live, scratch.count, scratch.count2));
    return scratch.count2.value;
  }

  /// The vertices' normals, 3 floats each: a new copy each time, with
  /// every change so far. Unity's `Mesh.normals`.
  Float32List get normals => _readVertices(3, (p, n) => fy_mesh_read_vertices(_live, p, nullptr, nullptr, nullptr, n));

  /// The vertices' tangents, 4 floats each: a new copy each time.
  Float32List get tangents => _readVertices(4, (p, n) => fy_mesh_read_vertices(_live, nullptr, p, nullptr, nullptr, n));

  /// The vertices' texture coordinates, 2 floats each: a new copy each
  /// time. Unity's `Mesh.uv`.
  Float32List get uvs => _readVertices(2, (p, n) => fy_mesh_read_vertices(_live, nullptr, nullptr, p, nullptr, n));

  /// The vertices' colours, 4 bytes each (sRGB red, green, blue, then
  /// alpha): a new copy each time. White unless set. Unity's
  /// `Mesh.colors32`.
  Uint8List get colors {
    final count = vertexCount;
    return using((arena) {
      final p = arena<Uint8>(count * 4);
      check(fy_mesh_read_vertices(_live, nullptr, nullptr, nullptr, p, count));
      return Uint8List.fromList(p.asTypedList(count * 4));
    });
  }

  Float32List _readVertices(int per, int Function(Pointer<Float>, int) read) {
    final count = vertexCount;
    return using((arena) {
      final p = arena<Float>(count * per);
      check(read(p, count));
      return Float32List.fromList(p.asTypedList(count * per));
    });
  }

  /// How many parts it has, each drawn with its own material
  /// ([setTriangles]' `submeshStarts`): 1 for a mesh that doesn't split.
  /// Unity's `subMeshCount`.
  int get submeshCount {
    check(fy_mesh_submesh_count(_live, scratch.count));
    return scratch.count.value;
  }

  /// Where part [n]'s triangles start in [indices], and how many indices
  /// it has.
  ({int start, int count}) submesh(int n) {
    final count = submeshCount;
    RangeError.checkValidIndex(n, null, 'n', count);
    return using((arena) {
      final (starts, counts) = (arena<Uint32>(count), arena<Uint32>(count));
      check(fy_mesh_read_submeshes(_live, starts, counts, count));
      return (start: starts[n], count: counts[n]);
    });
  }

  /// The vertices' positions, 3 floats each (x, y, z), in the mesh's own
  /// space: a new copy each time. Unity's `Mesh.vertices`.
  Float32List get positions {
    final count = vertexCount * 3;
    return using((arena) {
      final p = arena<Float>(count);
      check(fy_mesh_read(_live, p, count ~/ 3, nullptr, 0));
      return Float32List.fromList(p.asTypedList(count));
    });
  }

  /// Three indices into [positions] per triangle, counter-clockwise seen
  /// from its front: a new copy each time. Unity's `Mesh.triangles`.
  Uint32List get indices {
    final count = indexCount;
    return using((arena) {
      final p = arena<Uint32>(count);
      check(fy_mesh_read(_live, nullptr, 0, p, count));
      return Uint32List.fromList(p.asTypedList(count));
    });
  }

  /// Frees the mesh: every world that has it lets it go, and each view
  /// frees its GPU copy on its next frame. Throws a [StateError] while an
  /// object's [MeshRenderer] uses it, and also
  /// a [StateError] for a built-in shape, which lives as long as the
  /// program. Calling it again does nothing.
  void dispose() {
    if (_pointer == nullptr) return;
    if (_shape) throw StateError('$this is built in and can\'t be disposed');
    for (final world in _release.worlds) {
      world._releaseMesh(this);
    }
    _release.forget();
    _finalizer.detach(this);
    fy_mesh_free(_pointer);
    _pointer = nullptr;
  }

  @override
  String toString() => _name;
}

/// What packages read of a [Mesh] (package:fynkel/extend.dart).
extension MeshVersions on Mesh {
  /// How many changes the worlds that have it have been given.
  int get version => _version;

  /// How many of those changed its positions or triangles: a collider made
  /// of it is made again when this moves on, not for its colours, UVs,
  /// normals or tangents.
  int get shapeVersion => _shapeVersion;

  /// Copies its positions, 3 floats a vertex, into [positions] and its
  /// indices into [indices], as many as each has room for, making nothing
  /// to collect: what [Mesh.positions] and [Mesh.indices] copy.
  void readInto(Pointer<Float> positions, int vertexCount, Pointer<Uint32> indices, int indexCount) {
    check(fy_mesh_read(_live, positions, vertexCount, indices, indexCount));
  }
}

/// An image for [Material]s to use, decoded once and shared, not copied,
/// by every world that uses it. Load a PNG or JPEG with [Texture2D.load];
/// make one from pixels with [Texture2D.rgba].
///
/// The material slot decides how its values are read: colour slots
/// ([Material.baseColorTexture], [Material.emissiveTexture]) as sRGB
/// colour, the others as linear data. Each view uploads it with mipmaps
/// the first frame a material uses it. Like a [Mesh], it's freed once
/// nothing reaches it; [dispose] frees it sooner.
final class Texture2D implements Finalizable {
  Texture2D._(this._pointer, this.width, this.height, this._name) {
    _finalizer.attach(this, _pointer.cast(), detach: this);
  }

  static final _finalizer = NativeFinalizer(
    Native.addressOf<NativeFunction<Void Function(Pointer<FyTexture>)>>(fy_texture_free).cast(),
  );

  /// Decodes a PNG or JPEG on a helper isolate, so the UI thread never
  /// waits for it: a big image takes tens of milliseconds. Any PNG layout
  /// (grey, palette, 16-bit) becomes 8-bit RGBA.
  ///
  /// Throws a [FormatException] saying what's wrong for a broken or
  /// unsupported file, or one over 8192 pixels on a
  /// side. [bytes] is copied.
  static Future<Texture2D> load(Uint8List bytes) async {
    ensureEngine();
    // Into native memory, and only its address to the helper: sending the
    // list itself copies it on this isolate, about 4× slower than this.
    // At least a byte: malloc(0) may be null, which isn't an empty image.
    final copy = malloc<Uint8>(bytes.isEmpty ? 1 : bytes.length);
    copy.asTypedList(bytes.length).setAll(0, bytes);
    try {
      final (address, width, height) = await inHelper(_decodeAt(copy.address, bytes.length));
      return Texture2D._(Pointer<FyTexture>.fromAddress(address), width, height, 'Texture2D($width × $height)');
    } finally {
      malloc.free(copy);
    }
  }

  /// What the helper isolate runs. A function of its own, so the closure
  /// it returns holds just two numbers.
  static (int, int, int) Function() _decodeAt(int address, int length) => () {
    ensureEngine();
    checkFile(fy_texture_decode(Pointer<Uint8>.fromAddress(address), length, scratch.texture));
    final texture = Texture2D._sized(scratch.texture.value);
    // This isolate ends next; the texture now belongs to the caller's.
    _finalizer.detach(texture);
    return (texture._pointer.address, texture.width, texture.height);
  };

  /// A texture from [pixels]: 8-bit RGBA, `width × height × 4` bytes, rows
  /// from the top. Throws an [ArgumentError] when the length doesn't match
  /// or a side
  /// is 0 or over 8192. [pixels] is copied.
  factory Texture2D.rgba(int width, int height, Uint8List pixels) {
    ensureEngine();
    return using((arena) {
      final copy = arena<Uint8>(pixels.isEmpty ? 1 : pixels.length);
      copy.asTypedList(pixels.length).setAll(0, pixels);
      check(fy_texture_new_rgba(width, height, copy, pixels.length, scratch.texture));
      return Texture2D._sized(scratch.texture.value);
    });
  }

  factory Texture2D._sized(Pointer<FyTexture> pointer) {
    check(fy_texture_size(pointer, scratch.width, scratch.height));
    final (width, height) = (scratch.width.value, scratch.height.value);
    return Texture2D._(pointer, width, height, 'Texture2D($width × $height)');
  }

  Pointer<FyTexture> _pointer;
  final String _name;

  /// In pixels.
  final int width, height;

  final _release = _Release(_Resource.texture);

  Pointer<FyTexture> get _live {
    if (_pointer == nullptr) throw StateError('$this was disposed');
    return _pointer;
  }

  /// Whether [dispose] was called; a disposed texture can't be used in a
  /// material (a StateError).
  bool get isDisposed => _pointer == nullptr;

  /// Frees the texture: every world that has it lets it go, and each view
  /// frees its GPU copy on its next frame. Throws a [StateError] while a
  /// material in a world uses it: set that
  /// slot to null first. Calling it again does nothing.
  void dispose() {
    if (_pointer == nullptr) return;
    for (final world in _release.worlds) {
      world._releaseTexture(this);
    }
    _release.forget();
    _finalizer.detach(this);
    fy_texture_free(_pointer);
    _pointer = nullptr;
  }

  @override
  String toString() => _name;
}

/// How a [Material]'s surface covers what's behind it.
///
/// See-through surfaces are drawn after the solid ones, the furthest
/// object first, by its middle. Within one object the order is its
/// triangles', so an object that overlaps itself (a glass jar's back and
/// front) can draw its far side over its near one: split it into two
/// objects if that shows. They receive light and fog, and cast no shadow.
enum Blending {
  /// Hides what's behind it; with an [Material.alphaCutoff], where its
  /// alpha reaches the cutoff.
  opaque,

  /// Laid over what's behind it by its alpha ([Material.opacity]), as
  /// glass, water or a fading object is: at 0.3, a third of it and two
  /// thirds of what's behind. Its highlights and glow fade with it.
  /// glTF's BLEND.
  transparent,

  /// Adds its light, times its alpha, to what's behind it: for glows,
  /// flames, sparks and beams. Black adds nothing, so its [Material.emissive]
  /// is what shows.
  additive,
}

/// A line drawn around an object, as in a cartoon ([Material.outline]):
/// its mesh drawn again behind it, inside out and [width] metres bigger
/// all round, in [color], lit by nothing. It moves and bends as the object
/// does, by its skeleton and its shader's `vertex`. Opaque materials only:
/// a cut-out or see-through one has none. A mesh whose faces don't share
/// their corners' normals, a cube's, shows gaps at its corners.
final class Outline {
  /// [width] is 0 to 100 metres: out of range or NaN throws a
  /// [RangeError] where it's used.
  const Outline({this.width = 0.02, this.color = Color.black});

  /// How far out it reaches, in metres.
  final double width;

  /// Its colour, picked in sRGB; alpha is ignored.
  final Color color;

  @override
  bool operator ==(Object other) => other is Outline && other.width == width && other.color == color;

  @override
  int get hashCode => Object.hash(width, color);

  @override
  String toString() => 'Outline(width: $width, color: $color)';
}

/// How a surface looks under light: glTF's metallic-roughness model, so a
/// loaded model's materials are these. A material can be used in any
/// number of worlds, and changing it changes every object that uses it, in
/// every world. Like a [Mesh], a world lets it go once nothing reaches it.
///
/// Each texture multiplies its value: [baseColorTexture] the [color],
/// [metallicRoughnessTexture] the [metallic] (blue) and [roughness]
/// (green), [emissiveTexture] the [emissive] light.
final class Material {
  /// A material, white, not metallic, half rough unless given. [opacity]
  /// replaces [color]'s alpha; below 1 it makes the material
  /// [Blending.transparent] unless [blending] says otherwise. [opacity],
  /// [metallic], [roughness] and [alphaCutoff] are 0 to 1,
  /// [emissiveIntensity] 0 to 1,000,000, [uvScale] and [uvOffset] -10,000
  /// to 10,000 each way, [reach] 0 to 10,000, an [outline]'s width 0 to
  /// 100: out of range or NaN throws a [RangeError]. A disposed texture
  /// throws a [StateError].
  Material({
    this.name,
    Color color = Color.white,
    double? opacity,
    Blending? blending,
    double metallic = 0,
    double roughness = 0.5,
    this._emissive = Color.black,
    double emissiveIntensity = 1,
    Vec2 uvScale = Vec2.one,
    Vec2 uvOffset = Vec2.zero,
    double alphaCutoff = 0,
    this._doubleSided = false,
    Texture2D? baseColorTexture,
    Texture2D? normalTexture,
    Texture2D? metallicRoughnessTexture,
    Texture2D? occlusionTexture,
    Texture2D? emissiveTexture,
    Shader? shader,
    Map<String, Object> values = const {},
    double reach = 0,
    Outline? outline,
  }) : _metallic = _unit(metallic, 'metallic'),
       _roughness = _unit(roughness, 'roughness'),
       _emissiveIntensity = _atLeast0(emissiveIntensity, 'emissiveIntensity'),
       _uvScale = _uv(uvScale, 'uvScale'),
       _uvOffset = _uv(uvOffset, 'uvOffset'),
       _alphaCutoff = _unit(alphaCutoff, 'alphaCutoff'),
       _color = opacity == null ? color : color.withAlpha(_unit(opacity, 'opacity')),
       _blending = blending ?? (opacity != null && opacity < 1 ? Blending.transparent : Blending.opaque),
       _textures = [
         _usable(baseColorTexture),
         _usable(normalTexture),
         _usable(metallicRoughnessTexture),
         _usable(occlusionTexture),
         _usable(emissiveTexture),
       ],
       _shader = _usableShader(shader),
       _reach = _metres(reach, 'reach'),
       _outline = _usableOutline(outline) {
    if (values.isNotEmpty && shader == null) {
      throw ArgumentError('values is $values, but the material has no shader: values are a shader\'s inputs');
    }
    values.forEach(_setValue);
    _pack();
  }

  /// What it's called, for messages and finding it again: a loaded
  /// model's materials have the names the file gives them. Needn't be
  /// unique.
  String? name;

  Color _color;
  double _metallic;
  double _roughness;
  Color _emissive;
  double _emissiveIntensity;
  Vec2 _uvScale;
  Vec2 _uvOffset;
  double _alphaCutoff;
  bool _doubleSided;
  Blending _blending;

  /// Base colour, normal, metallic-roughness, occlusion, emissive: the
  /// engine's order.
  final List<Texture2D?> _textures;

  Shader? _shader;
  double _reach;
  Outline? _outline;

  /// What [set] set, by name: numbers, vectors, colours and textures.
  final _values = <String, Object>{};

  /// [_values] laid out as [_shader]'s `Values`, and the layout they were
  /// laid out for: a shader's new code lays them out again.
  Uint8List? _bytes;

  /// [_bytes], to write one value into.
  ByteData? _data;
  _ShaderLayout? _packedFor;

  final _release = _Release(_Resource.material);

  bool _disposed = false;

  /// Whether [dispose] was called; a disposed material can't be used in a
  /// world (a StateError).
  bool get isDisposed => _disposed;

  /// Takes the material out of every world that has it, so the textures
  /// it uses can be disposed too. It can't be used in a world after this
  /// (a [StateError]). Throws a [StateError] too while an object's [MeshRenderer] uses it: destroy the object or give
  /// it another material first. Calling it again does nothing.
  void dispose() {
    if (_disposed) return;
    for (final world in _release.worlds) {
      world._releaseMaterial(this);
    }
    _release.forget();
    _disposed = true;
  }

  /// A new material with the same values and textures, and [name] if
  /// given: to change it for some objects and not the others that share
  /// this one. For a colour alone, [MeshRenderer.color] needs no copy.
  Material copy({String? name}) => Material(
    name: name ?? this.name,
    color: _color,
    metallic: _metallic,
    roughness: _roughness,
    emissive: _emissive,
    emissiveIntensity: _emissiveIntensity,
    uvScale: _uvScale,
    uvOffset: _uvOffset,
    alphaCutoff: _alphaCutoff,
    doubleSided: _doubleSided,
    blending: _blending,
    baseColorTexture: _textures[0],
    normalTexture: _textures[1],
    metallicRoughnessTexture: _textures[2],
    occlusionTexture: _textures[3],
    emissiveTexture: _textures[4],
    shader: _shader,
    values: _values,
    reach: _reach,
    outline: _outline,
  );

  /// Its own shader, or null for the engine's: see [Shader]. A new one
  /// keeps the values [set] gave that it has too. A disposed shader throws
  /// a [StateError].
  Shader? get shader => _shader;
  set shader(Shader? value) {
    _shader = _usableShader(value);
    if (value == null) _values.clear();
    _pack();
    _changed();
  }

  /// How far, in metres, its [shader]'s `vertex` can move a vertex from
  /// where the mesh has it: grass that sways 0.3 m needs 0.3. The engine
  /// grows each object's bounds by this much when it decides what's in
  /// view, so something that moved into view isn't left out. 0 (the
  /// default) for a shader that doesn't move its vertices; 0 to 1e4.
  double get reach => _reach;
  set reach(double value) {
    _reach = _metres(value, 'reach');
    _changed();
  }

  /// The line drawn around every object using it, or null (the default)
  /// for none: `Outline(width: 0.02, color: Color(0xFF202020))`. Opaque
  /// materials only. An outline's width out of 0 to 100, or NaN, throws a
  /// [RangeError].
  Outline? get outline => _outline;
  set outline(Outline? value) {
    _outline = _usableOutline(value);
    _changed();
  }

  /// Sets its [shader]'s value or texture [name] for every object using
  /// the material: a number (`double`, or `int` for an `i32` or `u32`), a
  /// [Vec2], a [Vec3], a [Color] (for a `vec4<f32>`, or a `vec3<f32>`
  /// without its alpha; picked in sRGB, as [color] is, and given to the
  /// shader linear), a list of numbers, or a [Texture2D]. An array
  /// (`array<vec3<f32>, 8>`) takes a list of those, up to its length; the
  /// rest are 0. Throws an [ArgumentError] for a name the shader doesn't
  /// have or a value that isn't its kind, and a [StateError] without a
  /// shader. To tween one:
  /// `tween(0.0, 1.0, seconds: 1, onUpdate: (v) => material.set('dissolve', v))`.
  void set(String name, Object value) {
    final shader = _shader;
    if (shader == null) throw StateError('$this has no shader to set $name for');
    // One value of the shader as it is now: written in place, and only its
    // bytes sent on (stage 14's DF7a).
    final data = _data;
    if (value is! Texture2D && data != null && identical(_packedFor, shader._layout)) {
      final v = shader._layout.value(name);
      if (v != null && v.write(data, value, shader)) {
        _values[name] = value;
        _release.updateValues(this, v.offset, v.parts * 4);
        return;
      }
    }
    _setValue(name, value);
    _pack();
    _changed();
  }

  /// What [set] last gave [name], or null.
  Object? value(String name) => _values[name];

  void _setValue(String name, Object value) {
    final shader = _shader!;
    final layout = shader._layout;
    if (value is Texture2D) {
      if (layout.texture(name) < 0) {
        throw ArgumentError.value(name, 'name', 'is no texture of $shader, which has: ${layout.names}');
      }
      _values[name] = _usable(value)!;
      return;
    }
    final v = layout.value(name);
    if (v == null) throw ArgumentError.value(name, 'name', 'is no value of $shader, which has: ${layout.names}');
    v.parseAll(value, shader);
    _values[name] = value;
  }

  /// Lays [_values] out as the shader's `Values` are now. What its new
  /// code doesn't have, or has as another kind, is left out.
  void _pack() {
    final shader = _shader;
    final bytes = _bytes = shader?._layout.pack(_values, shader);
    _data = bytes == null ? null : ByteData.sublistView(bytes);
    _packedFor = shader?._layout;
  }

  /// The texture its shader's texture [index] is given, or null: also
  /// when what it has by that name is a value, from before a reload.
  Texture2D? _shaderTexture(int index) {
    final layout = _shader?._layout;
    if (layout == null || index >= layout.textures.length) return null;
    return switch (_values[layout.textures[index].name]) {
      final Texture2D texture => texture,
      _ => null,
    };
  }

  /// The base colour (albedo). Its alpha is the [opacity].
  Color get color => _color;
  set color(Color value) {
    _color = value;
    _changed();
  }

  /// How much of the surface there is, 0 to 1: [color]'s alpha, times
  /// [baseColorTexture]'s. It shows when the material is
  /// [Blending.transparent] or [Blending.additive], or cut out at
  /// [alphaCutoff]; an opaque one ignores it. Setting it changes the alpha
  /// alone, not [blending].
  double get opacity => _color.a;
  set opacity(double value) {
    _color = _color.withAlpha(_unit(value, 'opacity'));
    _changed();
  }

  /// Whether what's behind the surface shows through, and how:
  /// [Blending.opaque] unless given, or unless the material was made with
  /// an [opacity] below 1.
  Blending get blending => _blending;
  set blending(Blending value) {
    _blending = value;
    _changed();
  }

  /// 0 for paint, plastic or wood; 1 for bare metal.
  double get metallic => _metallic;
  set metallic(double value) {
    _metallic = _unit(value, 'metallic');
    _changed();
  }

  /// 0 for a mirror finish, 1 for fully matte.
  double get roughness => _roughness;
  set roughness(double value) {
    _roughness = _unit(value, 'roughness');
    _changed();
  }

  /// Light the surface gives off whatever lights it, like a screen or a
  /// lamp: black for none. Alpha is ignored.
  Color get emissive => _emissive;
  set emissive(Color value) {
    _emissive = value;
    _changed();
  }

  /// Multiplies [emissive], as a light's intensity does its colour.
  double get emissiveIntensity => _emissiveIntensity;
  set emissiveIntensity(double value) {
    _emissiveIntensity = _atLeast0(value, 'emissiveIntensity');
    _changed();
  }

  /// How many times the textures repeat across a mesh whose texture
  /// coordinates go from 0 to 1, such as `Mesh.plane()`, each way:
  /// `Vec2(10, 10)` tiles a ground texture ten times. A negative value
  /// flips the textures. -1e4 to 1e4.
  Vec2 get uvScale => _uvScale;
  set uvScale(Vec2 value) {
    _uvScale = _uv(value, 'uvScale');
    _changed();
  }

  /// Added to texture coordinates after [uvScale], so a part of a texture
  /// fills the mesh: `uvScale: Vec2(0.5, 0.5), uvOffset: Vec2(0.5, 0)`
  /// shows its top-right quarter. -1e4 to 1e4.
  Vec2 get uvOffset => _uvOffset;
  set uvOffset(Vec2 value) {
    _uvOffset = _uv(value, 'uvOffset');
    _changed();
  }

  /// 0 for a solid surface. Otherwise a cut-out, for leaves and fences:
  /// the surface is there only where the alpha of [color] times
  /// [baseColorTexture] reaches this, and gone elsewhere. 0.5 is usual.
  /// With [blending], what's left is see-through too.
  double get alphaCutoff => _alphaCutoff;
  set alphaCutoff(double value) {
    _alphaCutoff = _unit(value, 'alphaCutoff');
    _changed();
  }

  /// Draw both sides of each triangle, the back lit like the front: for
  /// leaves, paper and flags, whose triangles are seen from behind.
  /// Otherwise a triangle seen from behind isn't drawn.
  bool get doubleSided => _doubleSided;
  set doubleSided(bool value) {
    _doubleSided = value;
    _changed();
  }

  /// Multiplies [color]. Read as sRGB colour.
  Texture2D? get baseColorTexture => _textures[0];
  set baseColorTexture(Texture2D? value) => _setTexture(0, value);

  /// A tangent-space normal map, as glTF stores them: red along the
  /// image's right, green up it. Read as linear data.
  Texture2D? get normalTexture => _textures[1];
  set normalTexture(Texture2D? value) => _setTexture(1, value);

  /// Roughness in green and metallic in blue, multiplying [roughness] and
  /// [metallic] (glTF's packing). Read as linear data.
  Texture2D? get metallicRoughnessTexture => _textures[2];
  set metallicRoughnessTexture(Texture2D? value) => _setTexture(2, value);

  /// Ambient occlusion in red: how much of the light from all around
  /// reaches each point. Read as linear data.
  Texture2D? get occlusionTexture => _textures[3];
  set occlusionTexture(Texture2D? value) => _setTexture(3, value);

  /// Multiplies [emissive]. Read as sRGB colour.
  Texture2D? get emissiveTexture => _textures[4];
  set emissiveTexture(Texture2D? value) => _setTexture(4, value);

  void _setTexture(int slot, Texture2D? value) {
    _textures[slot] = _usable(value);
    _changed();
  }

  bool _uses(Texture2D texture) =>
      _textures.any((t) => identical(t, texture)) || _values.values.any((t) => identical(t, texture));

  void _changed() {
    _release.updateMaterial(this);
  }

  // The engine checks again; checking here too reports the mistake where
  // it was made, even before the material is used in a world.
  static double _unit(double value, String name) {
    if (!(value >= 0 && value <= 1)) throw RangeError.value(value, name, 'must be between 0 and 1');
    return value;
  }

  static double _atLeast0(double value, String name) {
    if (!(value >= 0 && value <= 1e6)) throw RangeError.value(value, name, 'must be between 0 and 1e6');
    return value;
  }

  static Vec2 _uv(Vec2 value, String name) {
    bool fits(double x) => x >= -1e4 && x <= 1e4;
    if (!fits(value.x) || !fits(value.y)) throw RangeError('$name must be between -1e4 and 1e4 each way, not $value');
    return value;
  }

  static double _metres(double value, String name) {
    if (!(value >= 0 && value <= 1e4)) throw RangeError.value(value, name, 'must be between 0 and 1e4');
    return value;
  }

  static Outline? _usableOutline(Outline? outline) {
    if (outline != null && !(outline.width >= 0 && outline.width <= 100)) {
      throw RangeError.value(outline.width, 'outline.width', 'must be between 0 and 100');
    }
    return outline;
  }

  static Shader? _usableShader(Shader? shader) {
    if (shader != null && shader.isDisposed) throw StateError('$shader was disposed');
    if (shader != null && shader.isEffect) {
      throw ArgumentError('shader is $shader, a screen effect: give it to a ScreenEffect, not a Material');
    }
    return shader;
  }

  static Texture2D? _usable(Texture2D? texture) {
    if (texture != null && texture.isDisposed) throw StateError('$texture was disposed');
    return texture;
  }

  @override
  String toString() {
    final textures = [
      for (final (i, name) in ['baseColor', 'normal', 'metallicRoughness', 'occlusion', 'emissive'].indexed)
        if (_textures[i] != null) '${name}Texture: ${_textures[i]}',
    ];
    if (name case final name? when name.isNotEmpty) return "Material('$name')";
    final fields = [
      'color: $color, metallic: $metallic, roughness: $roughness',
      if (emissive != Color.black) 'emissive: $emissive × $emissiveIntensity',
      if (uvScale != Vec2.one) 'uvScale: $uvScale',
      if (uvOffset != Vec2.zero) 'uvOffset: $uvOffset',
      if (alphaCutoff != 0) 'alphaCutoff: $alphaCutoff',
      if (doubleSided) 'doubleSided',
      if (blending != Blending.opaque) 'blending: $blending',
      ...textures,
      if (_shader case final shader?) 'shader: $shader',
      if (_reach != 0) 'reach: $_reach',
      if (_outline case final outline?) 'outline: $outline',
    ];
    return 'Material(${fields.join(', ')})';
  }
}

enum _Resource { material, mesh, texture, shader }

/// Which worlds have a mesh, material or texture, and its id in each.
/// When the resource is garbage-collected, each world that's still there
/// takes it out. So it mustn't reach the resource, or that would never be
/// collected; and it holds the worlds weakly, so a resource kept in a
/// global doesn't keep every world it was used in alive.
final class _Release {
  _Release(this.kind);

  final _Resource kind;
  final _in = <(WeakReference<World>, int)>[];

  static final _finalizer = Finalizer<_Release>((release) {
    for (final (world, id) in release._in) {
      world.target?._collect(release.kind, id);
    }
  });

  Iterable<World> get worlds => _in.map((entry) => entry.$1.target).nonNulls;

  void add(Object resource, World world, int id) {
    if (_in.isEmpty) _finalizer.attach(resource, this, detach: this);
    _in.add((WeakReference(world), id));
  }

  void removeWhere(bool Function(World world) test) {
    _in.removeWhere(
      (entry) => switch (entry.$1.target) {
        null => true,
        final world => test(world),
      },
    );
    if (_in.isEmpty) _finalizer.detach(this);
  }

  /// Writes [mesh], this one's, as it is now to each world that has it,
  /// and forgets those gone or that no longer have it. In place, with no
  /// closure: a mesh can change every frame.
  void updateMesh(Mesh mesh) {
    final entries = _in;
    var kept = 0;
    for (var i = 0; i < entries.length; i++) {
      final entry = entries[i];
      final world = entry.$1.target;
      if (world != null && world._updateMesh(mesh, entry.$2)) entries[kept++] = entry;
    }
    if (kept == entries.length) return;
    entries.length = kept;
    if (kept == 0) _finalizer.detach(this);
  }

  /// Writes [material], this one's, to each world that has it, and forgets
  /// those gone or that no longer have it. In place, with no closure: a
  /// tween changes a material every frame.
  void updateMaterial(Material material) {
    final entries = _in;
    var kept = 0;
    for (var i = 0; i < entries.length; i++) {
      final entry = entries[i];
      final world = entry.$1.target;
      if (world != null && world._updateMaterial(material)) entries[kept++] = entry;
    }
    if (kept == entries.length) return;
    entries.length = kept;
    if (kept == 0) _finalizer.detach(this);
  }

  /// Writes [length] bytes of [material]'s values, this one's, from
  /// [offset] to each world that has it, as [updateMaterial] does.
  void updateValues(Material material, int offset, int length) {
    final entries = _in;
    var kept = 0;
    for (var i = 0; i < entries.length; i++) {
      final entry = entries[i];
      final world = entry.$1.target;
      if (world != null && world._updateValues(material, offset, length)) entries[kept++] = entry;
    }
    if (kept == entries.length) return;
    entries.length = kept;
    if (kept == 0) _finalizer.detach(this);
  }

  /// [world] took the resource out, and may add it again.
  void drop(World world) => removeWhere((w) => identical(w, world));

  /// After the resource's `dispose`: no world has it any more.
  void forget() {
    _in.clear();
    _finalizer.detach(this);
  }
}
