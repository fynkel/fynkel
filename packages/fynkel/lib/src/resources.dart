part of 'world.dart';

/// A shape to draw: triangles, with a normal, tangent and texture
/// coordinate at each vertex. A mesh can be used in any number of worlds;
/// each shares its data instead of copying it.
///
/// Each built-in shape is one [Mesh], so `Mesh.cube()` always returns the
/// same object. [Mesh.custom] and [Mesh.raw] make one from code; for a big
/// mesh, make it in [Mesh.inBackground]. A world keeps a mesh while your
/// code or an object's [MeshRenderer] can reach it, and lets it go once
/// it's garbage-collected; [dispose] frees it sooner.
final class Mesh implements Finalizable {
  Mesh._(this._pointer, this._name, {this._shape = false}) {
    _finalizer.attach(this, _pointer.cast(), detach: this);
  }

  static final _finalizer = NativeFinalizer(
    Native.addressOf<NativeFunction<Void Function(Pointer<FyMesh>)>>(fy_mesh_free).cast(),
  );

  Pointer<FyMesh> _pointer;
  final String _name;
  final bool _shape;

  final _release = _Release(_Resource.mesh);

  Pointer<FyMesh> get _live {
    if (_pointer == nullptr) throw StateError('$this was disposed');
    return _pointer;
  }

  bool get isDisposed => _pointer == nullptr;

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
    final (address, name) = await inHelper(() {
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
    check(fy_mesh_counts(_live, scratch.count, scratch.count2));
    return scratch.count.value;
  }

  /// Three per triangle.
  int get indexCount {
    check(fy_mesh_counts(_live, scratch.count, scratch.count2));
    return scratch.count2.value;
  }

  /// Frees the mesh: every world that has it lets it go, and each view
  /// frees its GPU copy on its next frame. Throws a [FynkelException]
  /// ([FynkelStatus.inUse]) while an object's [MeshRenderer] uses it, and
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
  /// Throws a [FynkelException] ([FynkelStatus.invalidValue]) saying what's
  /// wrong for a broken or unsupported file, or one over 8192 pixels on a
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
    check(fy_texture_decode(Pointer<Uint8>.fromAddress(address), length, scratch.texture));
    final texture = Texture2D._sized(scratch.texture.value);
    // This isolate ends next; the texture now belongs to the caller's.
    _finalizer.detach(texture);
    return (texture._pointer.address, texture.width, texture.height);
  };

  /// A texture from [pixels]: 8-bit RGBA, `width × height × 4` bytes, rows
  /// from the top. Throws a [FynkelException]
  /// ([FynkelStatus.invalidValue]) when the length doesn't match or a side
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

  bool get isDisposed => _pointer == nullptr;

  /// Frees the texture: every world that has it lets it go, and each view
  /// frees its GPU copy on its next frame. Throws a [FynkelException]
  /// ([FynkelStatus.inUse]) while a material in a world uses it: set that
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

/// How a surface looks under light: glTF's metallic-roughness model, so a
/// loaded model's materials are these. A material can be used in any
/// number of worlds, and changing it changes every object that uses it, in
/// every world. Like a [Mesh], a world lets it go once nothing reaches it.
///
/// Each texture multiplies its value: [baseColorTexture] the [color],
/// [metallicRoughnessTexture] the [metallic] (blue) and [roughness]
/// (green), [emissiveTexture] the [emissive] light.
final class Material {
  Material({
    this.name,
    this._color = Color.white,
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
  }) : _metallic = _unit(metallic, 'metallic'),
       _roughness = _unit(roughness, 'roughness'),
       _emissiveIntensity = _atLeast0(emissiveIntensity, 'emissiveIntensity'),
       _uvScale = _uv(uvScale, 'uvScale'),
       _uvOffset = _uv(uvOffset, 'uvOffset'),
       _alphaCutoff = _unit(alphaCutoff, 'alphaCutoff'),
       _textures = [
         _usable(baseColorTexture),
         _usable(normalTexture),
         _usable(metallicRoughnessTexture),
         _usable(occlusionTexture),
         _usable(emissiveTexture),
       ];

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

  /// Base colour, normal, metallic-roughness, occlusion, emissive: the
  /// engine's order.
  final List<Texture2D?> _textures;

  final _release = _Release(_Resource.material);

  bool _disposed = false;

  bool get isDisposed => _disposed;

  /// Takes the material out of every world that has it, so the textures
  /// it uses can be disposed too. It can't be used in a world after this
  /// (a [StateError]). Throws a [FynkelException] ([FynkelStatus.inUse])
  /// while an object's [MeshRenderer] uses it: despawn the object or give
  /// it another material first. Calling it again does nothing.
  void dispose() {
    if (_disposed) return;
    for (final world in _release.worlds) {
      world._releaseMaterial(this);
    }
    _release.forget();
    _disposed = true;
  }

  /// The base colour (albedo). Its alpha matters only with [alphaCutoff].
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
  /// (Partly see-through surfaces aren't drawn yet.)
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

  bool _uses(Texture2D texture) => _textures.any((t) => identical(t, texture));

  void _changed() {
    _release.removeWhere((world) => !world._updateMaterial(this));
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
      ...textures,
    ];
    return 'Material(${fields.join(', ')})';
  }
}

enum _Resource { material, mesh, texture }

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
    _in.removeWhere((entry) => switch (entry.$1.target) {
      null => true,
      final world => test(world),
    });
    if (_in.isEmpty) _finalizer.detach(this);
  }

  /// [world] took the resource out, and may add it again.
  void drop(World world) => removeWhere((w) => identical(w, world));

  /// After the resource's `dispose`: no world has it any more.
  void forget() {
    _in.clear();
    _finalizer.detach(this);
  }
}
