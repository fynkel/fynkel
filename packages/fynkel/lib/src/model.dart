part of 'world.dart';

/// A 3D model from a `.glb` file (binary glTF 2.0, which Blender and most
/// tools export): its meshes, textures and materials, and the tree of
/// nodes that places them. Load it once with [Model.load], then add as
/// many copies as you like ([instantiate]); every copy shares the same
/// data.
///
/// Its animations are [clips], which a copy's [GameObject.animator]
/// plays. What the engine can't draw yet (skins, morph targets, ...) is
/// skipped, and [warnings] says what. [dispose] frees
/// the model; otherwise it's freed when it's garbage collected. Its
/// meshes, textures and materials live on in the worlds that have copies.
final class Model implements Finalizable {
  Model._(this._pointer) {
    _finalizer.attach(this, _pointer.cast(), detach: this);
    check(fy_model_counts(_pointer, scratch.modelCounts));
    final counts = scratch.modelCounts.ref;
    final (meshCount, textureCount, materialCount, warningCount, clipCount) = (
      counts.meshes,
      counts.textures,
      counts.materials,
      counts.warnings,
      counts.clips,
    );
    meshes = List.unmodifiable([
      for (var i = 0; i < meshCount; i++)
        Mesh._(_take(fy_model_mesh(_pointer, i, scratch.mesh), scratch.mesh), 'Model mesh $i'),
    ]);
    textures = List.unmodifiable([
      for (var i = 0; i < textureCount; i++)
        Texture2D._sized(_take(fy_model_texture(_pointer, i, scratch.texture), scratch.texture)),
    ]);
    materials = List.unmodifiable([for (var i = 0; i < materialCount; i++) _material(i)]);
    clips = List.unmodifiable([
      for (var i = 0; i < clipCount; i++)
        AnimationClip._(
          _string((buf, cap, len) => fy_model_clip(_pointer, i, buf, cap, len, scratch.number)),
          scratch.number.value,
        ),
    ]);
    warnings = List.unmodifiable([
      for (var i = 0; i < warningCount; i++) _string((buf, cap, len) => fy_model_warning(_pointer, i, buf, cap, len)),
    ]);
  }

  static final _finalizer = NativeFinalizer(
    Native.addressOf<NativeFunction<Void Function(Pointer<FyModel>)>>(fy_model_free).cast(),
  );

  /// Reads a `.glb` on a helper isolate, so the UI thread never waits for
  /// it: decoding a model's images and preparing its meshes takes tens of
  /// milliseconds or more.
  ///
  /// Most games want `world.assets.instantiate('ship.glb')` instead
  /// ([Assets.instantiate]): it loads the model once, shares it, and frees
  /// it when the last copy is destroyed. This is the way by hand: read the
  /// bytes yourself (`rootBundle.load('assets/ship.glb')`), and [dispose]
  /// the model yourself.
  ///
  /// Throws a [FormatException] saying what's wrong for a file that can't
  /// be read: a `.gltf` (JSON) file, a
  /// broken one, one that refers to other files, or one that needs a glTF
  /// extension the engine doesn't know. [bytes] is copied.
  static Future<Model> load(Uint8List bytes) async {
    ensureEngine();
    // As Texture2D.load: native memory, and only its address to the
    // helper.
    final copy = malloc<Uint8>(bytes.isEmpty ? 1 : bytes.length);
    copy.asTypedList(bytes.length).setAll(0, bytes);
    try {
      final address = await inHelper(_loadAt(copy.address, bytes.length));
      return Model._(Pointer<FyModel>.fromAddress(address));
    } finally {
      malloc.free(copy);
    }
  }

  /// What the helper isolate runs: just the reading. The handles to the
  /// model's parts are made on the caller's isolate, which owns them.
  static int Function() _loadAt(int address, int length) => () {
    ensureEngine();
    checkFile(fy_model_load(Pointer<Uint8>.fromAddress(address), length, scratch.model));
    return scratch.model.value.address;
  };

  Pointer<FyModel> _pointer;

  /// Every mesh, each drawn by one or more of the model's nodes.
  /// Primitives that share their vertices share a mesh.
  late final List<Mesh> meshes;

  /// Every image the materials use, decoded once.
  late final List<Texture2D> textures;

  /// The model's materials, named as in the file. Changing one changes
  /// every copy of the model that uses it, in every world.
  late final List<Material> materials;

  /// The model's animations, in the file's order: what a copy's
  /// [GameObject.animator] plays.
  late final List<AnimationClip> clips;

  /// What was skipped or changed, one line for each kind of thing: empty
  /// when the model is drawn exactly as the file says.
  late final List<String> warnings;

  Pointer<FyModel> get _live {
    if (_pointer == nullptr) throw StateError('this Model was disposed');
    return _pointer;
  }

  /// Whether [dispose] was called; a disposed Model can't be
  /// [instantiate]d.
  bool get isDisposed => _pointer == nullptr;

  /// The model's material [index]: its values, its name, and its
  /// textures from [textures].
  Material _material(int index) {
    check(fy_model_material(_pointer, index, scratch.material));
    final m = scratch.material.ref;
    Texture2D? texture(int number) => number == 0 ? null : textures[number - 1];
    return Material(
      name: _string((buf, cap, len) => fy_model_material_name(_pointer, index, buf, cap, len)),
      color: m.base_color.read(),
      metallic: m.metallic,
      roughness: m.roughness,
      emissive: m.emissive.read(),
      emissiveIntensity: m.emissive_intensity,
      uvScale: Vec2(m.uv_scale.x, m.uv_scale.y),
      uvOffset: Vec2(m.uv_offset.x, m.uv_offset.y),
      alphaCutoff: m.alpha_cutoff,
      doubleSided: m.double_sided,
      blending: Blending.values[m.blend],
      baseColorTexture: texture(m.base_color_texture),
      normalTexture: texture(m.normal_texture),
      metallicRoughnessTexture: texture(m.metallic_roughness_texture),
      occlusionTexture: texture(m.occlusion_texture),
      emissiveTexture: texture(m.emissive_texture),
    );
  }

  /// A copy of the model to add to a world: a root object at [transform],
  /// named after the model's scene, with an object for each node of the
  /// file below it, named and placed as there. A node that draws one part
  /// gets a [MeshRenderer]; one that draws several gets an unnamed child
  /// for each. Find the parts with [GameObject.find] once it's added.
  ///
  /// Every copy shares the model's meshes, textures and [materials]
  /// (changing a material changes every copy), so adding another uploads
  /// nothing new. Adding it throws, adding nothing, if one of the model's
  /// meshes or textures was disposed, or the model was.
  GameObject instantiate({Transform? transform}) {
    _live;
    return GameObject(transform: transform).._described!.model = this;
  }

  /// Frees the model's own data. Its [meshes], [textures] and [materials]
  /// stay usable, and copies already added keep drawing; only
  /// [instantiate]d copies not yet added can't be added any more. Calling
  /// it again does nothing.
  ///
  /// To free everything the model brought, once no copy of it is left, dispose its parts as well: the [materials] first, as they use
  /// the [textures], then the [meshes] and [textures].
  void dispose() {
    if (_pointer == nullptr) return;
    _finalizer.detach(this);
    fy_model_free(_pointer);
    _pointer = nullptr;
  }

  @override
  String toString() => 'Model(${meshes.length} meshes, ${materials.length} materials)';
}

/// The handle a `fy_model_*` call wrote to [out], once [status] is OK.
Pointer<T> _take<T extends NativeType>(int status, Pointer<Pointer<T>> out) {
  check(status);
  return out.value;
}

/// A string a `fy_*` call copies out as UTF-8: tried in the scratch
/// buffer, and again in a bigger one when it didn't fit.
String _string(int Function(Pointer<Uint8> buf, int cap, Pointer<Size> len) call) {
  final capacity = scratch.byteCapacity;
  check(call(scratch.bytes(), capacity, scratch.count));
  final length = scratch.count.value;
  if (length > capacity) check(call(scratch.bytes(length), length, scratch.count));
  return const Utf8Decoder().convert(scratch.bytes().asTypedList(length));
}
