import 'dart:convert';
import 'dart:ffi';
import 'dart:isolate';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'bindings.g.dart';
import 'color.dart';
import 'error.dart';
import 'math.dart';
import 'native.dart';

part 'components.dart';
part 'model.dart';
part 'resources.dart';

/// Everything in a scene: objects, their components, and the meshes and
/// materials they use. A world lives in the engine; free it with
/// [dispose] (or it's freed when it's garbage collected).
final class World implements Finalizable {
  World() {
    ensureEngine();
    _pointer = fy_world_new();
    _finalizer.attach(this, _pointer.cast(), detach: this);
  }

  static final _finalizer = NativeFinalizer(
    Native.addressOf<NativeFunction<Void Function(Pointer<FyWorld>)>>(fy_world_free).cast(),
  );

  Pointer<FyWorld> _pointer = nullptr;

  // Meshes, materials and textures this world has, both ways. They're
  // added the first time something uses them, and stay until the world is
  // disposed or [Mesh.dispose] or [Texture2D.dispose] takes them out.
  final _meshIds = <Mesh, int>{};
  final _meshes = <int, Mesh>{};
  final _materialIds = <Material, int>{};
  final _materials = <int, Material>{};
  final _textureIds = <Texture2D, int>{};

  /// The engine's world, for package:fynkel's views.
  Pointer<FyWorld> get pointer {
    if (_pointer == nullptr) throw StateError('this World was disposed');
    return _pointer;
  }

  bool get isDisposed => _pointer == nullptr;

  /// Frees the world. Its objects can't be used after this; a view of it
  /// keeps drawing what it last had until the view is disposed too.
  void dispose() {
    if (_pointer == nullptr) return;
    _finalizer.detach(this);
    fy_world_free(_pointer);
    _pointer = nullptr;
    _meshIds.clear();
    _meshes.clear();
    _materialIds.clear();
    _materials.clear();
    _textureIds.clear();
  }

  /// A new object with these components (at most one of each kind). If a
  /// component is refused, the object is despawned again and the error
  /// thrown.
  GameObject spawn([Component? a, Component? b, Component? c, Component? d]) {
    final components = [a, b, c, d].nonNulls.toList();
    for (var i = 0; i < components.length; i++) {
      for (var j = 0; j < i; j++) {
        if (components[i].runtimeType == components[j].runtimeType) {
          throw ArgumentError('spawn got two ${components[i].runtimeType}s; an object has at most one');
        }
      }
    }
    check(fy_spawn(pointer, scratch.id));
    final object = GameObject._(this, scratch.id.value);
    try {
      components.forEach(object.add);
    } catch (_) {
      fy_despawn(_pointer, object.id);
      rethrow;
    }
    return object;
  }

  /// Spawns [model]: a root object at [transform], named after the
  /// model's scene, with an object for each node of the file below it,
  /// named and placed as there. A node that draws one part gets a
  /// [MeshRenderer]; one that draws several gets an unnamed child for
  /// each. Find nodes with [GameObject.find].
  ///
  /// Every copy shares the model's meshes, textures and [Model.materials]
  /// (changing a material changes every copy), so spawning a model again
  /// uploads nothing new. Throws, spawning nothing, if one of the model's
  /// meshes or textures was disposed.
  GameObject spawnModel(Model model, [Transform? transform]) {
    final meshes = model.meshes.map(_meshId).toList();
    final materials = model.materials.map(_materialId).toList();
    final at = transform ?? Transform();
    final t = scratch.transform.ref;
    t.position.write(at.position);
    t.rotation.write(at.rotation);
    t.scale.write(at.scale);
    return using((arena) {
      Pointer<Uint64> copy(List<int> ids) {
        final p = arena<Uint64>(ids.isEmpty ? 1 : ids.length);
        p.asTypedList(ids.length).setAll(0, ids);
        return p;
      }

      final out = arena<Uint64>();
      check(
        fy_model_spawn(
          pointer,
          model._live,
          copy(meshes),
          meshes.length,
          copy(materials),
          materials.length,
          scratch.transform,
          out,
        ),
      );
      return GameObject._(this, out.value);
    });
  }

  /// The closest object along [ray], no further than [maxDistance], or
  /// null when it hits nothing. It reads the world as it is now, not as the
  /// last frame showed it, so it finds what the game just moved.
  ///
  /// It hits what a view draws: objects with a [Transform] and a
  /// [MeshRenderer] with both a mesh and a material, from the side they're
  /// drawn from (both sides for a [Material.doubleSided] one). Cut-outs
  /// ([Material.alphaCutoff]) are hit as if solid. No physics is involved:
  /// each object's box is tested, then the triangles of those the ray may
  /// hit.
  RayHit? raycast(Ray ray, {double maxDistance = double.infinity}) {
    final r = scratch.ray.ref;
    r.origin.write(ray.origin);
    r.direction.write(ray.direction);
    check(fy_raycast(pointer, scratch.ray, maxDistance, scratch.rayHit));
    final hit = scratch.rayHit.ref;
    if (hit.object == 0) return null;
    return RayHit._(GameObject._(this, hit.object), hit.point.read(), hit.normal.read(), hit.distance);
  }

  int _meshId(Mesh mesh) {
    final known = _meshIds[mesh];
    if (known != null) return known;
    check(fy_mesh_add(pointer, mesh._live, scratch.id));
    final id = scratch.id.value;
    _meshIds[mesh] = id;
    _meshes[id] = mesh;
    mesh._worlds.add(WeakReference(this));
    return id;
  }

  /// Takes [mesh] out of this world, for [Mesh.dispose]. The engine
  /// refuses while an object uses it.
  void _releaseMesh(Mesh mesh) {
    final id = _meshIds[mesh];
    if (id == null || isDisposed) return;
    check(fy_mesh_remove(_pointer, id));
    _meshIds.remove(mesh);
    _meshes.remove(id);
  }

  int _textureId(Texture2D texture) {
    final known = _textureIds[texture];
    if (known != null) return known;
    check(fy_texture_add(pointer, texture._live, scratch.id));
    final id = scratch.id.value;
    _textureIds[texture] = id;
    texture._worlds.add(WeakReference(this));
    return id;
  }

  /// Takes [texture] out of this world, for [Texture2D.dispose]. Refused
  /// while one of this world's materials uses it; checked here, so the
  /// error names the Dart material rather than the engine's id.
  void _releaseTexture(Texture2D texture) {
    final id = _textureIds[texture];
    if (id == null || isDisposed) return;
    for (final material in _materials.values) {
      if (material._uses(texture)) {
        throw FynkelException(FynkelStatus.inUse, '$texture is still used by $material');
      }
    }
    check(fy_texture_remove(_pointer, id));
    _textureIds.remove(texture);
  }

  int _materialId(Material material) {
    final known = _materialIds[material];
    if (known != null) return known;
    _writeMaterial(material);
    check(fy_material_add(pointer, scratch.material, scratch.id));
    final id = scratch.id.value;
    _materialIds[material] = id;
    _materials[id] = material;
    material._worlds.add(WeakReference(this));
    return id;
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

/// An object in a [World]: an id and whatever components it has. Reading a
/// component gives a copy; setting it replaces the object's.
///
/// (Called `GameObject`, not `Object`, because Dart's `Object` is every
/// value's type.)
final class GameObject {
  GameObject._(this.world, this.id);

  final World world;

  /// The engine's id: never 0, and never reused after a despawn.
  final int id;

  /// Whether the object still exists (it hasn't been despawned and the
  /// world hasn't been disposed).
  bool get isAlive => !world.isDisposed && fy_exists(world._pointer, id);

  /// Removes the object and its components, and its [children] with
  /// theirs, all the way down. Using any of them afterwards throws a
  /// [FynkelException] with [FynkelStatus.noObject].
  void despawn() => check(fy_despawn(world.pointer, id));

  // --- Hierarchy ---

  /// The object this one hangs from, or null for a root. Its [transform] is
  /// relative to its parent, so it moves, turns and scales with it.
  ///
  /// Setting it keeps [transform] as it is (now counted from the new
  /// parent) and puts the object last among the parent's [children].
  /// Setting null makes it a root. A parent that is this object or below it
  /// throws [FynkelStatus.invalidValue], as does one from another world.
  GameObject? get parent {
    check(fy_parent_get(world.pointer, id, scratch.id));
    final parent = scratch.id.value;
    return parent == 0 ? null : GameObject._(world, parent);
  }

  set parent(GameObject? value) {
    if (value != null && !identical(value.world, world)) {
      throw FynkelException(
        FynkelStatus.invalidValue,
        '$value is in another world, so it can\'t be the parent of $this',
      );
    }
    check(fy_parent_set(world.pointer, id, value?.id ?? 0));
  }

  /// The objects whose [parent] is this one, in the order they were added.
  /// A new list each time.
  List<GameObject> get children {
    final w = world.pointer;
    var capacity = scratch.idCapacity;
    check(fy_children_get(w, id, scratch.ids(), capacity, scratch.count));
    final count = scratch.count.value;
    if (count > capacity) {
      check(fy_children_get(w, id, scratch.ids(count), count, scratch.count));
    }
    final ids = scratch.ids();
    return List.unmodifiable([for (var i = 0; i < count; i++) GameObject._(world, ids[i])]);
  }

  /// Whether this object is [other] or below it.
  bool isPartOf(GameObject other) {
    if (!identical(other.world, world)) return false;
    check(fy_is_part_of(world.pointer, id, other.id, scratch.flag));
    return scratch.flag.value;
  }

  /// The object's name, or null. Names needn't be unique; setting '' is
  /// the same as null.
  String? get name {
    final w = world.pointer;
    var capacity = scratch.byteCapacity;
    check(fy_name_get(w, id, scratch.bytes(), capacity, scratch.count));
    final length = scratch.count.value;
    if (length == 0) return null;
    if (length > capacity) {
      check(fy_name_get(w, id, scratch.bytes(length), length, scratch.count));
    }
    return const Utf8Decoder().convert(scratch.bytes().asTypedList(length));
  }

  set name(String? value) {
    final length = scratch.utf8(value ?? '');
    check(fy_name_set(world.pointer, id, scratch.bytes(), length));
  }

  /// The first object below this one named [name], searching depth first
  /// in [children] order, or null. This object itself isn't searched.
  GameObject? find(String name) {
    final length = scratch.utf8(name);
    check(fy_find(world.pointer, id, scratch.bytes(), length, scratch.id));
    final found = scratch.id.value;
    return found == 0 ? null : GameObject._(world, found);
  }

  /// Where the object is in the world: its [transform] after its parents'.
  /// Read only; to move it, set [transform].
  ///
  /// The position and rotation are exact. The scale is approximate when a
  /// parent with an uneven scale has a rotated child: the result is then
  /// skewed, which no position, rotation and scale can describe (Unity
  /// calls this the "lossy" scale).
  Transform get worldTransform {
    check(fy_world_transform_get(world.pointer, id, scratch.transform));
    final t = scratch.transform.ref;
    return Transform._(t.position.read(), t.rotation.read(), t.scale.read());
  }

  /// The ray from this camera through [point] in a view showing it: from
  /// the point on the near plane, away from the camera. Cast it with
  /// [World.raycast] to find what's drawn there.
  ///
  /// Uses the object's [Camera] and its [worldTransform] (its scale is
  /// ignored, as the view ignores it). Throws a [FynkelException] with
  /// [FynkelStatus.noComponent] when it has no camera.
  Ray rayThrough(ViewPoint point) {
    if (!(point.width > 0 && point.height > 0)) {
      throw ArgumentError.value(point, 'point', 'needs a view with an area');
    }
    final c = camera;
    final t = worldTransform;
    final tanHalf = math.tan(c.fovY * math.pi / 360);
    // Through the point on the plane 1 in front of the camera, in the
    // camera's frame: -Z ahead, +Y up, so the view's y is flipped.
    final x = (2 * point.x / point.width - 1) * tanHalf * point.width / point.height;
    final y = (1 - 2 * point.y / point.height) * tanHalf;
    final through = t.rotation.rotate(Vec3(x, y, -1));
    return Ray(t.position + through * c.near, through);
  }

  /// Adds [component], replacing one of the same kind.
  void add(Component component) => switch (component) {
    Transform c => transform = c,
    Camera c => camera = c,
    Light c => light = c,
    MeshRenderer c => meshRenderer = c,
  };

  /// Removes the object's component of kind [T]. Throws if it has none.
  void remove<T extends Component>() {
    final w = world.pointer;
    check(switch (T) {
      const (Transform) => fy_transform_remove(w, id),
      const (Camera) => fy_camera_remove(w, id),
      const (Light) => fy_light_remove(w, id),
      const (MeshRenderer) => fy_mesh_renderer_remove(w, id),
      _ => throw ArgumentError('remove needs a component type, such as remove<Camera>(); got $T'),
    });
  }

  Transform get transform {
    check(fy_transform_get(world.pointer, id, scratch.transform));
    final t = scratch.transform.ref;
    return Transform._(t.position.read(), t.rotation.read(), t.scale.read());
  }

  set transform(Transform value) {
    final t = scratch.transform.ref;
    t.position.write(value.position);
    t.rotation.write(value.rotation);
    t.scale.write(value.scale);
    check(fy_transform_set(world.pointer, id, scratch.transform));
  }

  /// Turns the object [angle] radians around [axis], in its own frame.
  void rotate(Vec3 axis, double angle) => transform = transform.rotated(axis, angle);

  /// Moves the object by [offset], in its parent's space (the world's for
  /// a root).
  void translate(Vec3 offset) => transform = transform.translated(offset);

  Camera get camera {
    check(fy_camera_get(world.pointer, id, scratch.camera));
    final c = scratch.camera.ref;
    return Camera(fovY: c.fov_y, near: c.near_clip, far: c.far_clip, clearColor: c.clear_color.read());
  }

  set camera(Camera value) {
    final c = scratch.camera.ref;
    c.fov_y = value.fovY;
    c.near_clip = value.near;
    c.far_clip = value.far;
    c.clear_color.write(value.clearColor);
    check(fy_camera_set(world.pointer, id, scratch.camera));
  }

  Light get light {
    check(fy_light_get(world.pointer, id, scratch.light));
    final l = scratch.light.ref;
    return switch (l.kind) {
      FY_LIGHT_DIRECTIONAL => Light.sun(color: l.color.read(), intensity: l.intensity),
      FY_LIGHT_AMBIENT => Light.ambient(color: l.color.read(), intensity: l.intensity),
      final kind => throw StateError('the engine returned light kind $kind, which this package:fynkel predates'),
    };
  }

  set light(Light value) {
    final l = scratch.light.ref;
    l.kind = switch (value.kind) {
      LightKind.sun => FY_LIGHT_DIRECTIONAL,
      LightKind.ambient => FY_LIGHT_AMBIENT,
    };
    l.color.write(value.color);
    l.intensity = value.intensity;
    check(fy_light_set(world.pointer, id, scratch.light));
  }

  MeshRenderer get meshRenderer {
    check(fy_mesh_renderer_get(world.pointer, id, scratch.meshRenderer));
    final r = scratch.meshRenderer.ref;
    return MeshRenderer(world._meshes[r.mesh], world._materials[r.material]);
  }

  set meshRenderer(MeshRenderer value) {
    final w = world;
    final mesh = value.mesh == null ? 0 : w._meshId(value.mesh!);
    final material = value.material == null ? 0 : w._materialId(value.material!);
    scratch.meshRenderer.ref
      ..mesh = mesh
      ..material = material;
    check(fy_mesh_renderer_set(w.pointer, id, scratch.meshRenderer));
  }

  @override
  bool operator ==(Object other) => other is GameObject && identical(other.world, world) && other.id == id;

  @override
  int get hashCode => Object.hash(identityHashCode(world), id);

  @override
  String toString() => 'GameObject#$id';
}

/// Where a ray hit first ([World.raycast]).
final class RayHit {
  RayHit._(this.object, this.point, this.normal, this.distance);

  final GameObject object;

  /// In the world.
  final Vec3 point;

  /// The face's normal in the world, unit length, on the side the ray came
  /// from.
  final Vec3 normal;

  /// From the ray's origin to [point].
  final double distance;

  @override
  String toString() => 'RayHit($object at $point, normal $normal, distance $distance)';
}
