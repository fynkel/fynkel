import 'dart:convert';
import 'dart:ffi';
import 'dart:isolate';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'bindings.g.dart';
import 'color.dart';
import 'error.dart';
import 'math.dart';
import 'native.dart';

part 'components.dart';
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

  // Meshes and materials this world has, both ways. Stage 1 never removes
  // them: they live as long as the world.
  final _meshIds = <Mesh, int>{};
  final _meshes = <int, Mesh>{};
  final _materialIds = <Material, int>{};
  final _materials = <int, Material>{};

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

  int _meshId(Mesh mesh) {
    final known = _meshIds[mesh];
    if (known != null) return known;
    check(fy_mesh_add(pointer, mesh._pointer, scratch.id));
    final id = scratch.id.value;
    _meshIds[mesh] = id;
    _meshes[id] = mesh;
    return id;
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
    final m = scratch.material.ref;
    m.base_color.write(material.color);
    m.metallic = material.metallic;
    m.roughness = material.roughness;
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
