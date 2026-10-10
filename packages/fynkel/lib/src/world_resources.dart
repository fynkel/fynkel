part of 'world.dart';

/// Meshes, textures and materials in the engine: their ids, writing them,
/// and freeing what was disposed or garbage-collected.
extension _Resources on World {
  /// Disposes [things]: materials first (they use textures), then meshes,
  /// textures and models. What something still uses is left to be freed
  /// when that's garbage-collected; any other error is reported, not
  /// thrown, as this runs while objects are destroyed.
  void _free(Iterable<Object> things, GameObject? object) {
    for (final kind in [Material, Mesh, Texture2D, Model]) {
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
            case Texture2D texture:
              _unloadMaterialsUsing(texture);
              texture.dispose();
          }
        } catch (error, stack) {
          // Something still using it is freed with what uses it.
          if (error is FynkelException && error.status == FynkelStatus.inUse) continue;
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
        throw FynkelException(FynkelStatus.inUse, '$texture is still used by $material');
      }
    }
    check(fy_texture_remove(_pointer, id));
    _textureIds[texture] = null;
  }

  int _materialId(Material material) {
    final known = _materialIds[material];
    if (known != null) return known;
    if (material.isDisposed) throw StateError('$material was disposed');
    _writeMaterial(material);
    check(fy_material_add(pointer, scratch.material, scratch.id));
    final id = scratch.id.value;
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
      };
      if (status == FyStatus.FY_STATUS_IN_USE) return false;
      if (kind == _Resource.material) _materials.remove(id);
      if (kind == _Resource.mesh) _meshes.remove(id);
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
    return true;
  }

  void _writeMaterial(Material material) {
    int texture(Texture2D? t) => t == null ? 0 : _textureId(t);
    // Textures first: adding one can fail, and then nothing was written.
    final textures = material._textures.map(texture).toList();
    final m = scratch.material.ref;
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
    m.double_sided = material.doubleSided;
    m.base_color_texture = textures[0];
    m.normal_texture = textures[1];
    m.metallic_roughness_texture = textures[2];
    m.occlusion_texture = textures[3];
    m.emissive_texture = textures[4];
  }
}
