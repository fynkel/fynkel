part of 'world.dart';

/// Meshes, textures and materials in the engine: their ids, writing them,
/// and freeing what was disposed or garbage-collected.
extension _Resources on World {
  /// Disposes [things]: materials first (they use textures), then meshes,
  /// textures and models. What something still uses is left to be freed
  /// when that's garbage-collected; any other error is reported, not
  /// thrown, as this runs while objects are destroyed.
  void _free(Iterable<Object> things, GameObject? object) {
    for (final kind in [Material, Mesh, Texture2D, Shader, Model]) {
      for (final thing in things) {
        if (thing.runtimeType != kind) continue;
        try {
          switch (thing) {
            case Model model:
              _free([...model.materials, ...model.meshes, ...model.textures], object);
              model.dispose();
            case Material material:
              material.dispose();
            case Mesh mesh:
              mesh.dispose();
            case Shader shader:
              shader.dispose();
            case Texture2D texture:
              _unloadMaterialsUsing(texture);
              texture.dispose();
          }
        } catch (error, stack) {
          // Something still using it is freed with what uses it.
          if (error is InUseError) continue;
          if (!isDisposed) _report(object, error, stack);
        }
      }
    }
  }

  int _meshId(Mesh mesh) {
    final known = _meshIds[mesh];
    if (known != null) return known;
    check(fy_mesh_add(pointer, mesh._live, scratch.id));
    final id = scratch.id.value;
    _meshIds[mesh] = id;
    _meshes[id] = WeakReference(mesh);
    mesh._release.add(mesh, this, id);
    return id;
  }

  /// Takes [mesh] out of this world, for [Mesh.dispose]. The engine
  /// refuses while an object uses it.
  void _releaseMesh(Mesh mesh) {
    final id = _meshIds[mesh];
    if (id == null || isDisposed) return;
    check(fy_mesh_remove(_pointer, id));
    _meshIds[mesh] = null;
    _meshes.remove(id);
  }

  int _textureId(Texture2D texture) {
    final known = _textureIds[texture];
    if (known != null) return known;
    check(fy_texture_add(pointer, texture._live, scratch.id));
    final id = scratch.id.value;
    _textureIds[texture] = id;
    texture._release.add(texture, this, id);
    return id;
  }

  /// Takes [texture] out of this world, for [Texture2D.dispose]. Refused
  /// while one of this world's materials uses it; checked here, so the
  /// error names the Dart material rather than the engine's id.
  void _releaseTexture(Texture2D texture) {
    final id = _textureIds[texture];
    if (id == null || isDisposed) return;
    for (final material in _materials.values.map((m) => m.target).nonNulls) {
      if (material._uses(texture)) {
        throw InUseError('$texture is still used by $material');
      }
    }
    check(fy_texture_remove(_pointer, id));
    _textureIds[texture] = null;
  }

  int _shaderId(Shader shader) {
    final known = _shaderIds[shader];
    if (known != null) return known;
    check(fy_shader_add(pointer, shader._live, scratch.id));
    final id = scratch.id.value;
    _shaderIds[shader] = id;
    _shaders[id] = WeakReference(shader);
    shader._release.add(shader, this, id);
    return id;
  }

  /// Takes [shader] out of this world, for [Shader.dispose]. Refused
  /// while one of this world's materials uses it.
  void _releaseShader(Shader shader) {
    final id = _shaderIds[shader];
    if (id == null || isDisposed) return;
    for (final material in _materials.values.map((m) => m.target).nonNulls) {
      if (identical(material._shader, shader)) {
        throw InUseError('$shader is still used by $material');
      }
    }
    check(fy_shader_remove(_pointer, id));
    _shaderIds[shader] = null;
    _shaders.remove(id);
  }

  /// Gives this world [shader]'s new code, and its materials' values laid
  /// out for it.
  void _reloadShader(Shader shader) {
    final id = _shaderIds[shader];
    if (id == null || isDisposed) return;
    check(fy_shader_set(_pointer, id, shader._live));
    for (final material in _materials.values.map((m) => m.target).nonNulls) {
      if (identical(material._shader, shader)) _updateMaterial(material);
    }
  }

  int _materialId(Material material) {
    final known = _materialIds[material];
    if (known != null) return known;
    if (material.isDisposed) throw StateError('$material was disposed');
    _writeMaterial(material);
    check(fy_material_add(pointer, scratch.material, scratch.id));
    final id = scratch.id.value;
    _writeValues(material, id);
    _materialIds[material] = id;
    _materials[id] = WeakReference(material);
    material._release.add(material, this, id);
    return id;
  }

  /// Takes [material] out of this world, for [Material.dispose]. The
  /// engine refuses while an object uses it.
  void _releaseMaterial(Material material) {
    final id = _materialIds[material];
    if (id == null || isDisposed) return;
    check(fy_material_remove(_pointer, id));
    _materialIds[material] = null;
    _materials.remove(id);
  }

  /// Takes out of this world the materials using [texture] that no object
  /// draws with, so the texture can be freed. Each comes back the next
  /// time something uses it.
  void _unloadMaterialsUsing(Texture2D texture) {
    if (isDisposed) return;
    for (final MapEntry(key: id, value: ref) in _materials.entries.toList()) {
      final Material? material = ref.target;
      if (material == null || !material._uses(texture)) continue;
      if (fy_material_remove(_pointer, id) != FyStatus.FY_STATUS_OK) continue;
      _materialIds[material] = null;
      _materials.remove(id);
      material._release.drop(this);
    }
  }

  /// For a [_Release]: [id], a [kind] of resource nothing reaches any more,
  /// is to be taken out.
  void _collect(_Resource kind, int id) {
    if (isDisposed) return;
    _collected.add((kind, id));
    _collectedNew = true;
  }

  /// Takes out what was garbage-collected: materials first, as they use
  /// textures. What an object still uses (a part of a model copy whose
  /// model was collected) stays until the world is disposed.
  void _removeCollected() {
    _collectedNew = false;
    _collected.sort((a, b) => a.$1.index - b.$1.index);
    _collected.removeWhere((gone) {
      final (kind, id) = gone;
      final status = switch (kind) {
        _Resource.material => fy_material_remove(_pointer, id),
        _Resource.mesh => fy_mesh_remove(_pointer, id),
        _Resource.texture => fy_texture_remove(_pointer, id),
        _Resource.shader => fy_shader_remove(_pointer, id),
      };
      if (status == FyStatus.FY_STATUS_IN_USE) return false;
      if (kind == _Resource.material) _materials.remove(id);
      if (kind == _Resource.mesh) _meshes.remove(id);
      if (kind == _Resource.shader) _shaders.remove(id);
      return true;
    });
  }

  /// Sends [material]'s new values to the engine. False once this world no
  /// longer has the material (it was disposed), so the material forgets it.
  bool _updateMaterial(Material material) {
    final id = _materialIds[material];
    if (id == null || isDisposed) return false;
    _writeMaterial(material);
    check(fy_material_set(_pointer, id, scratch.material));
    _writeValues(material, id);
    return true;
  }

  /// Sends [material]'s shader values to the engine, its material [id].
  void _writeValues(Material material, int id) {
    if (material._shader case final shader? when !identical(material._packedFor, shader._layout)) {
      material._pack();
    }
    final bytes = material._bytes;
    if (bytes == null) return;
    final native = scratch.bytes(bytes.length);
    native.asTypedList(bytes.length).setAll(0, bytes);
    check(fy_material_values_set(_pointer, id, 0, native, bytes.length));
  }

  void _writeMaterial(Material material) {
    // Textures first: adding one can fail, and then nothing was written.
    final textures = material._textures;
    final base = _textureOrNone(textures[0]);
    final normal = _textureOrNone(textures[1]);
    final metallicRoughness = _textureOrNone(textures[2]);
    final occlusion = _textureOrNone(textures[3]);
    final emissive = _textureOrNone(textures[4]);
    final shader = material._shader == null ? 0 : _shaderId(material._shader!);
    final own0 = _textureOrNone(material._shaderTexture(0));
    final own1 = _textureOrNone(material._shaderTexture(1));
    final own2 = _textureOrNone(material._shaderTexture(2));
    final own3 = _textureOrNone(material._shaderTexture(3));
    final m = scratch.material.ref;
    m.shader = shader;
    m.shader_textures[0] = own0;
    m.shader_textures[1] = own1;
    m.shader_textures[2] = own2;
    m.shader_textures[3] = own3;
    m.base_color.write(material.color);
    m.metallic = material.metallic;
    m.roughness = material.roughness;
    m.emissive.write(material.emissive);
    m.emissive_intensity = material.emissiveIntensity;
    m.uv_scale
      ..x = material.uvScale.x
      ..y = material.uvScale.y;
    m.uv_offset
      ..x = material.uvOffset.x
      ..y = material.uvOffset.y;
    m.alpha_cutoff = material.alphaCutoff;
    m.reach = material.reach;
    m.double_sided = material.doubleSided;
    m.blend = material.blending.index;
    m.base_color_texture = base;
    m.normal_texture = normal;
    m.metallic_roughness_texture = metallicRoughness;
    m.occlusion_texture = occlusion;
    m.emissive_texture = emissive;
  }

  int _textureOrNone(Texture2D? texture) => texture == null ? 0 : _textureId(texture);
}
