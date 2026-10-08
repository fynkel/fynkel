part of 'world.dart';

/// A shape to draw: triangles, with a normal, tangent and texture
/// coordinate at each vertex. A mesh can be used in any number of worlds;
/// each shares its data instead of copying it.
///
/// Each built-in shape is one [Mesh], so `Mesh.cube()` always returns the
/// same object. [Mesh.custom] and [Mesh.raw] make one from code; for a big
/// mesh, make it in [Mesh.inBackground].
final class Mesh implements Finalizable {
  Mesh._(this._pointer, this._name) {
    _finalizer.attach(this, _pointer.cast(), detach: this);
  }

  static final _finalizer = NativeFinalizer(
    Native.addressOf<NativeFunction<Void Function(Pointer<FyMesh>)>>(fy_mesh_free).cast(),
  );

  final Pointer<FyMesh> _pointer;
  final String _name;

  /// A 1×1×1 cube around the origin. Each face shows the whole texture,
  /// upright on the four sides.
  factory Mesh.cube() => _cube;

  /// A sphere of radius 0.5. The texture wraps around it once, with its
  /// top at the north pole.
  factory Mesh.sphere() => _sphere;

  /// A 1×1 plane facing +Y. Seen from above, the texture's top is towards
  /// -Z.
  factory Mesh.plane() => _plane;

  static final _cube = Mesh._shape(FY_MESH_CUBE, 'Mesh.cube()');
  static final _sphere = Mesh._shape(FY_MESH_SPHERE, 'Mesh.sphere()');
  static final _plane = Mesh._shape(FY_MESH_PLANE, 'Mesh.plane()');

  factory Mesh._shape(int shape, String name) {
    ensureEngine();
    final out = scratch.mesh;
    check(fy_mesh_new_shape(shape, out));
    return Mesh._(out.value, name);
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
  /// Throws a [FynkelException] ([FynkelStatus.invalidValue]) for a broken
  /// mesh, saying what's wrong. The lists are copied.
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
      return Mesh._(out.value, 'Mesh.custom');
    });
  }

  /// Runs [build] (such as `() => Mesh.custom(...)`) on a helper isolate,
  /// so a big mesh doesn't hold up the UI thread: generating tangents
  /// takes about a microsecond per triangle.
  static Future<Mesh> inBackground(Mesh Function() build) async {
    final (address, name) = await Isolate.run(() {
      final mesh = build();
      // This isolate ends next; the mesh now belongs to the caller's.
      _finalizer.detach(mesh);
      return (mesh._pointer.address, mesh._name);
    });
    return Mesh._(Pointer<FyMesh>.fromAddress(address), name);
  }

  /// How many vertices the mesh has. Can be more than were given, when
  /// generated tangents split some.
  int get vertexCount {
    check(fy_mesh_counts(_pointer, scratch.count, scratch.count2));
    return scratch.count.value;
  }

  /// Three per triangle.
  int get indexCount {
    check(fy_mesh_counts(_pointer, scratch.count, scratch.count2));
    return scratch.count2.value;
  }

  @override
  String toString() => _name;
}

/// How a surface looks under light. A material can be used in any number
/// of worlds, and changing it changes every object that uses it, in every
/// world.
final class Material {
  Material({this._color = Color.white, double metallic = 0, double roughness = 0.5})
    : _metallic = _unit(metallic, 'metallic'),
      _roughness = _unit(roughness, 'roughness');

  Color _color;
  double _metallic;
  double _roughness;

  /// The worlds that have this material. Weak, so a material kept in a
  /// global doesn't keep every world it was used in alive.
  final List<WeakReference<World>> _worlds = [];

  /// The base colour (albedo).
  Color get color => _color;
  set color(Color value) {
    _color = value;
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

  void _changed() {
    _worlds.removeWhere((ref) => ref.target?._updateMaterial(this) != true);
  }

  // The engine checks again; checking here too reports the mistake where
  // it was made, even before the material is used in a world.
  static double _unit(double value, String name) {
    if (!(value >= 0 && value <= 1)) throw RangeError.value(value, name, 'must be between 0 and 1');
    return value;
  }

  @override
  String toString() => 'Material(color: $color, metallic: $metallic, roughness: $roughness)';
}
