import 'dart:convert';
import 'dart:ffi';
import 'dart:isolate';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:ffi/ffi.dart';
import 'package:meta/meta.dart';

import 'bindings.g.dart';
import 'color.dart';
import 'error.dart';
import 'math.dart';
import 'native.dart';

part 'components.dart';
part 'input.dart';
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
  // disposed or their `dispose` takes them out.
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
    _objects.clear();
    _byClass.clear();
    _live.clear();
    _starting.clear();
    _moved.clear();
    _off.clear();
    _hidden.clear();
  }

  // --- The loop ---

  // Every object the game added, in the order it became live, so parents
  // come before their children. Destroyed ones are dropped and moved ones
  // put last at the start of the next update.
  final _live = <GameObject>[];
  var _liveDirty = false;
  final _moved = <GameObject>[];

  // Live objects that haven't had onStart yet, in the same order.
  final _starting = <GameObject>[];

  // Objects whose own flag is off, and the ids of everything they hide,
  // worked out again when [_shape] has changed since [_hiddenAt]. Only
  // Dart turns objects off, so it knows them all; with none off (the
  // usual case) nothing is worked out at all.
  final _off = <GameObject>{};
  final _hidden = <int>{};
  var _shape = 0;
  var _hiddenAt = -1;

  Input _input = Input.none;

  /// What the player did since the last frame, as the last [update] got
  /// it. Objects read it as [GameObject.input].
  Input get input => _input;

  /// Runs one frame of the game, in this order:
  ///
  /// 1. **Taps.** Each of [input]'s taps is cast into the world from the
  ///    camera the view draws from. The object hit gets
  ///    [GameObject.onTap]; if it returns false, its parent does, and so on
  ///    up to the root, until one returns true.
  /// 2. **Starts.** Each object added since the last frame gets
  ///    [GameObject.onStart], once, parents before children. Objects added
  ///    during a start start too.
  /// 3. **Updates.** Each started object gets [GameObject.onUpdate],
  ///    parents before children, in the order they were added. Objects
  ///    added during an update start next frame; objects destroyed during
  ///    it get no more calls, even this frame. A new [GameObject.parent]
  ///    counts from the next frame.
  ///
  /// Hidden objects (inactive, or below one that is) are skipped by all
  /// three and start when they're first shown. A view then draws the world
  /// as it is.
  void update(double dt, [Input? input]) {
    final w = pointer;
    _input = input ?? Input.none;
    _tidy();

    for (final at in _input.taps) {
      _tap(w, at);
    }

    final waiting = <GameObject>[];
    for (var i = 0; i < _starting.length; i++) {
      final object = _starting[i];
      if (object._destroyed) continue;
      if (_isHidden(object)) {
        waiting.add(object);
        continue;
      }
      object._started = true;
      object.onStart();
    }
    _starting
      ..clear()
      ..addAll(waiting);

    // Objects added from here on are past the end when they come, or not
    // started: they wait for the next frame.
    for (var i = 0; i < _live.length; i++) {
      final object = _live[i];
      if (!object._started || object._destroyed || _isHidden(object)) continue;
      object.onUpdate(dt);
    }
  }

  /// Drops destroyed objects from the update order, and puts each moved
  /// one last with everything below it.
  void _tidy() {
    if (_liveDirty) {
      _live.removeWhere((o) => o._destroyed);
      _liveDirty = false;
    }
    for (final moved in _moved) {
      if (moved._destroyed) continue;
      final below = _subtreeIds(moved._id);
      final tail = [
        for (final o in _live)
          if (below.contains(o._id)) o,
      ];
      _live
        ..removeWhere((o) => below.contains(o._id))
        ..addAll(tail);
    }
    _moved.clear();
  }

  void _tap(Pointer<FyWorld> w, ViewPoint at) {
    check(fy_camera_active(w, scratch.id));
    final camera = scratch.id.value;
    if (camera == 0) return;
    final hit = raycast(_object(camera).rayThrough(at));
    if (hit == null) return;
    final tap = Tap._(at, hit);
    for (GameObject? object = hit.object; object != null && !object._destroyed;) {
      final up = object.parent;
      if (object.onTap(tap)) return;
      object = up;
    }
  }

  bool _isHidden(GameObject object) {
    if (_off.isEmpty) return false;
    if (_hiddenAt != _shape) {
      _hidden.clear();
      for (final off in _off) {
        _hidden.addAll(_subtreeIds(off._id));
      }
      _hiddenAt = _shape;
    }
    return _hidden.contains(object._id);
  }

  /// [id] and the ids of everything below it.
  Set<int> _subtreeIds(int id) {
    final ids = <int>{};
    void collect(int id) {
      ids.add(id);
      for (final child in _childIds(id)) {
        collect(child);
      }
    }

    collect(id);
    return ids;
  }

  /// Makes [root] live in this world, with everything added to it so
  /// far, and returns it. A world can have any number of roots: a level,
  /// a player who outlives levels, and so on.
  ///
  /// Throws a [StateError] if [root] was already added (to move a live
  /// object, set its [GameObject.parent]) or destroyed. If the engine
  /// refuses one of its components, the object isn't added and the error
  /// is thrown; objects below it that were already live stay so.
  T add<T extends GameObject>(T root) {
    root._checkAddable();
    _attach(root, null);
    return root;
  }

  /// Every live object of class [T] (or a subclass) that the game added,
  /// grouped by class, each class in the order its objects were added.
  /// The parts a model makes aren't included: find them by name.
  List<T> all<T extends GameObject>() => [
    for (final objects in _byClass.values)
      if (objects.first is T) ...objects.cast<T>(),
  ];

  /// The first live object of class [T] (or a subclass) that the game
  /// added, or null.
  T? first<T extends GameObject>() {
    for (final objects in _byClass.values) {
      if (objects.first is T) return objects.first as T;
    }
    return null;
  }

  // The Dart object for each live engine id that has been asked for, so
  // the same instance always comes back: the game's own, or a plain one
  // made the first time a model's part is asked for.
  final _objects = <int, GameObject>{};

  // The objects the game added, by class, for [all] and [first].
  final _byClass = <Type, Set<GameObject>>{};

  GameObject _object(int id) => _objects[id] ??= GameObject._live(this, id);

  /// Spawns [object] in the engine under [parent], then what was added to
  /// it before it was live.
  void _attach(GameObject object, GameObject? parent) {
    final described = object._described!;
    final id = described.model == null ? _spawn() : _spawnModel(described.model!, described.transform);
    object
      .._world = this
      .._id = id;
    try {
      if (parent != null) check(fy_parent_set(pointer, id, parent.id));
      if (described.model == null) object._writeTransform(described.transform);
      if (described.name != null) object._writeName(described.name!);
      if (!described.active) check(fy_active_set(pointer, id, false));
      for (final component in described.components) {
        object._write(component);
      }
    } catch (_) {
      fy_despawn(_pointer, id);
      object
        .._world = null
        .._id = 0;
      rethrow;
    }
    object._described = null;
    _objects[id] = object;
    (_byClass[object.runtimeType] ??= {}).add(object);
    _live.add(object);
    _starting.add(object);
    if (!described.active) _off.add(object);
    _shape++;
    for (final child in described.children) {
      child._queuedIn = null;
      _attach(child, object);
    }
  }

  int _spawn() {
    check(fy_spawn(pointer, scratch.id));
    return scratch.id.value;
  }

  /// A copy of [model]'s objects, with its root at [transform]: each copy
  /// shares the model's meshes, textures and materials.
  int _spawnModel(Model model, Transform transform) {
    final meshes = model.meshes.map(_meshId).toList();
    final materials = model.materials.map(_materialId).toList();
    final t = scratch.transform.ref;
    t.position.write(transform.position);
    t.rotation.write(transform.rotation);
    t.scale.write(transform.scale);
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
      return out.value;
    });
  }

  /// Takes [id] and everything below it out of the engine and forgets
  /// their Dart objects. Each gets [GameObject.onDestroy] first, children
  /// before parents, while it's still there to read.
  void _destroy(int id) {
    final ids = _subtreeIds(id).toList();
    final objects = [for (final id in ids) ?_objects[id]];
    for (final object in objects) {
      object._destroyed = true;
    }
    try {
      for (final object in objects.reversed) {
        object.onDestroy();
      }
    } finally {
      check(fy_despawn(pointer, id));
      for (final object in objects) {
        _objects.remove(object._id);
        _off.remove(object);
        final same = _byClass[object.runtimeType];
        if (same != null && same.remove(object) && same.isEmpty) _byClass.remove(object.runtimeType);
      }
      _liveDirty = true;
      _shape++;
    }
  }

  List<int> _childIds(int id) {
    final w = pointer;
    final capacity = scratch.idCapacity;
    check(fy_children_get(w, id, scratch.ids(), capacity, scratch.count));
    final count = scratch.count.value;
    if (count > capacity) check(fy_children_get(w, id, scratch.ids(count), count, scratch.count));
    final ids = scratch.ids();
    return [for (var i = 0; i < count; i++) ids[i]];
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
    return RayHit._(_object(hit.object), hit.point.read(), hit.normal.read(), hit.distance);
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
    if (material.isDisposed) throw StateError('$material was disposed');
    _writeMaterial(material);
    check(fy_material_add(pointer, scratch.material, scratch.id));
    final id = scratch.id.value;
    _materialIds[material] = id;
    _materials[id] = material;
    material._worlds.add(WeakReference(this));
    return id;
  }

  /// Takes [material] out of this world, for [Material.dispose]. The
  /// engine refuses while an object uses it.
  void _releaseMaterial(Material material) {
    final id = _materialIds[material];
    if (id == null || isDisposed) return;
    check(fy_material_remove(_pointer, id));
    _materialIds.remove(material);
    _materials.remove(id);
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

/// An object in a [World]: a [Transform], whatever other components it
/// has, and the objects added to it.
///
/// Make one, then add it: `world.add(GameObject(...))` or
/// `parent.add(child)`. Until then it only describes itself: its
/// [transform], [name] and components can be read and set, and objects
/// can be added to it, but anything that needs the world ([parent],
/// [children], [find], [worldTransform]...) throws a [StateError].
///
/// Extend it to give an object its own fields and code: a class is what
/// other engines call a prefab, and each instance is one object. The same
/// instance comes back from [find], [children], [parent] and
/// [World.raycast], so a hit on a character's arm leads to the
/// `Character` with its fields.
///
/// (Called `GameObject`, not `Object`, because Dart's `Object` is every
/// value's type.)
class GameObject {
  /// A new object, not yet in any world, with [transform] (the identity if
  /// not given) and [components]: at most one of each kind, and no
  /// [Transform] among them.
  GameObject({String? name, Transform? transform, Iterable<Component> components = const []})
    : _described = _Description(name, transform ?? Transform(), _checked(components));

  /// The object for an engine id the game didn't make, such as a model's
  /// part.
  GameObject._live(World world, int id) : _world = world, _id = id;

  static List<Component> _checked(Iterable<Component> components) {
    final list = components.toList();
    for (var i = 0; i < list.length; i++) {
      if (list[i] is Transform) {
        throw ArgumentError('give the Transform as transform:, not among the components');
      }
      for (var j = 0; j < i; j++) {
        if (list[i].runtimeType == list[j].runtimeType) {
          throw ArgumentError('got two ${list[i].runtimeType}s; an object has at most one');
        }
      }
    }
    return list;
  }

  World? _world;
  int _id = 0;

  // What the object is to be, until it's added.
  _Description? _described;

  // The object this one was added to before either was live.
  GameObject? _queuedIn;

  bool _destroyed = false;
  bool _started = false;

  /// The world the object was added to. Throws a [StateError] before it's
  /// added.
  World get world => _world ?? (throw _notAdded('world'));

  /// The engine's id: never 0, and never reused. Throws a [StateError]
  /// before the object is added.
  int get id => _world == null ? throw _notAdded('id') : _id;

  /// Whether the object is live: added to a world, not destroyed, and the
  /// world not disposed.
  bool get isAlive => _world != null && !_destroyed && !_world!.isDisposed && fy_exists(_world!._pointer, _id);

  /// Whether [destroy] was called on this object or one above it.
  bool get isDestroyed => _destroyed;

  StateError _notAdded(String what) => StateError(
    '$this has no $what yet: it isn\'t added to a world. Add it first (world.add, or add to an object '
    'that is), and touch the world from its onLoad on.',
  );

  // --- The loop ---

  /// What the player did since the last frame: the world's [World.input].
  Input get input => world.input;

  /// Called once, in the first [World.update] after the object is added
  /// and shown, before its first [onUpdate]. Its parent's comes first.
  @mustCallSuper
  @protected
  void onStart() {}

  /// Called every [World.update] with the seconds since the last one,
  /// parents before children, while the object is shown. Override it to
  /// move, animate or think; a mixin that overrides it calls
  /// `super.onUpdate(dt)` so the next one runs too.
  @mustCallSuper
  @protected
  void onUpdate(double dt) {}

  /// Called when the object, or one above it, is destroyed: children
  /// before parents, while the object is still there to read (where it
  /// was, what it carried). Adding to it then throws.
  @mustCallSuper
  @protected
  void onDestroy() {}

  /// Called when a tap hits this object or something below it that didn't
  /// take it ([Tap.object] is the one hit). Return true to take the tap;
  /// false (the default) passes it to the parent.
  @protected
  bool onTap(Tap tap) => false;

  // --- Adding and destroying ---

  /// Adds [child] below this object and returns it. If this object is
  /// live, [child] is now live too; if not, it becomes live when this one
  /// does.
  ///
  /// Throws a [StateError] if [child] was already added (to move a live
  /// object, set its [parent]) or destroyed, or would end up below itself.
  T add<T extends GameObject>(T child) {
    child._checkAddable();
    if (_destroyed) throw StateError('$this was destroyed, so nothing can be added to it');
    for (GameObject? above = this; above != null; above = above._queuedIn) {
      if (identical(above, child)) throw StateError('$child can\'t be added below itself');
    }
    if (_world != null) {
      _world!._attach(child, this);
    } else {
      child._queuedIn = this;
      _described!.children.add(child);
    }
    return child;
  }

  void _checkAddable() {
    if (_destroyed) throw StateError('$this was destroyed; make a new one to add');
    if (_world != null || _queuedIn != null) {
      throw StateError('$this was already added; to move it, set its parent');
    }
  }

  /// Removes the object and everything below it. Using any of them
  /// afterwards throws a [FynkelException] with [FynkelStatus.noObject].
  /// Destroying an object again does nothing.
  void destroy() {
    if (_destroyed) return;
    final world = _world;
    if (world == null) {
      _queuedIn?._described?.children.remove(this);
      _queuedIn = null;
      _destroyed = true;
      return;
    }
    if (world.isDisposed || !fy_exists(world._pointer, _id)) {
      _destroyed = true;
      return;
    }
    world._destroy(_id);
  }

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
    return parent == 0 ? null : world._object(parent);
  }

  set parent(GameObject? value) {
    if (value != null && !identical(value._world, world)) {
      throw FynkelException(
        FynkelStatus.invalidValue,
        '$value is in another world, so it can\'t be the parent of $this',
      );
    }
    check(fy_parent_set(world.pointer, id, value?.id ?? 0));
    world._moved.add(this);
    world._shape++;
  }

  /// The objects whose [parent] is this one, in the order they were added.
  /// A new list each time.
  List<GameObject> get children => List.unmodifiable(world._childIds(id).map(world._object));

  /// Whether this object is [other] or below it.
  bool isPartOf(GameObject other) {
    if (other._world == null || !identical(other._world, world)) return false;
    check(fy_is_part_of(world.pointer, id, other.id, scratch.flag));
    return scratch.flag.value;
  }

  /// The nearest object above this one that is a [T], or null.
  T? ancestor<T extends GameObject>() {
    for (var above = parent; above != null; above = above.parent) {
      if (above is T) return above;
    }
    return null;
  }

  /// Whether this object is on. One that is off hides itself and
  /// everything below it: they aren't drawn, don't light the scene or show
  /// it, and aren't hit by rays. Its children keep their own [active], so
  /// turning it on again shows the ones that were on. See
  /// [activeInHierarchy].
  bool get active {
    if (_described case final d?) return d.active;
    check(fy_active_get(world.pointer, id, scratch.flag));
    return scratch.flag.value;
  }

  set active(bool value) {
    if (_described case final d?) {
      d.active = value;
      return;
    }
    check(fy_active_set(world.pointer, id, value));
    if (value) {
      world._off.remove(this);
    } else {
      world._off.add(this);
    }
    world._shape++;
  }

  /// Whether this object is shown: it and every object above it [active].
  bool get activeInHierarchy {
    check(fy_active_in_hierarchy_get(world.pointer, id, scratch.flag));
    return scratch.flag.value;
  }

  /// The object's name, or null. Names needn't be unique; setting '' is
  /// the same as null.
  String? get name {
    if (_described case final d?) return d.name;
    final w = world.pointer;
    final capacity = scratch.byteCapacity;
    check(fy_name_get(w, id, scratch.bytes(), capacity, scratch.count));
    final length = scratch.count.value;
    if (length == 0) return null;
    if (length > capacity) {
      check(fy_name_get(w, id, scratch.bytes(length), length, scratch.count));
    }
    return const Utf8Decoder().convert(scratch.bytes().asTypedList(length));
  }

  set name(String? value) {
    if (_described case final d?) {
      d.name = value == '' ? null : value;
      return;
    }
    _writeName(value ?? '');
  }

  void _writeName(String value) {
    final length = scratch.utf8(value);
    check(fy_name_set(world.pointer, id, scratch.bytes(), length));
  }

  /// The first object below this one named [name], searching depth first
  /// in [children] order, or null. This object itself isn't searched.
  GameObject? find(String name) {
    final length = scratch.utf8(name);
    check(fy_find(world.pointer, id, scratch.bytes(), length, scratch.id));
    final found = scratch.id.value;
    return found == 0 ? null : world._object(found);
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

  /// Removes the object's component of kind [T]. Throws if it has none.
  void remove<T extends Component>() {
    if (_described case final d?) {
      if (T == Transform) throw ArgumentError('an object that isn\'t added yet always has a Transform');
      if (d.get<T>() == null) throw FynkelException(FynkelStatus.noComponent, '$this has no $T');
      d.components.removeWhere((c) => c is T);
      return;
    }
    final w = world.pointer;
    check(switch (T) {
      const (Transform) => fy_transform_remove(w, id),
      const (Camera) => fy_camera_remove(w, id),
      const (Light) => fy_light_remove(w, id),
      const (MeshRenderer) => fy_mesh_renderer_remove(w, id),
      _ => throw ArgumentError('remove needs a component type, such as remove<Camera>(); got $T'),
    });
  }

  /// The object's component of kind [T] while it's only described. Throws
  /// as the engine would when it has none.
  T _describedOr<T extends Component>(_Description d) =>
      d.get<T>() ?? (throw FynkelException(FynkelStatus.noComponent, '$this has no $T'));

  void _write(Component component) => switch (component) {
    Transform c => _writeTransform(c),
    Camera c => _writeCamera(c),
    Light c => _writeLight(c),
    MeshRenderer c => _writeMeshRenderer(c),
  };

  Transform get transform {
    if (_described case final d?) return d.transform;
    check(fy_transform_get(world.pointer, id, scratch.transform));
    final t = scratch.transform.ref;
    return Transform._(t.position.read(), t.rotation.read(), t.scale.read());
  }

  set transform(Transform value) {
    if (_described case final d?) {
      d.transform = value;
      return;
    }
    _writeTransform(value);
  }

  void _writeTransform(Transform value) {
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
    if (_described case final d?) return _describedOr<Camera>(d);
    check(fy_camera_get(world.pointer, id, scratch.camera));
    final c = scratch.camera.ref;
    return Camera(fovY: c.fov_y, near: c.near_clip, far: c.far_clip, clearColor: c.clear_color.read());
  }

  set camera(Camera value) => _described == null ? _writeCamera(value) : _described!.set(value);

  void _writeCamera(Camera value) {
    final c = scratch.camera.ref;
    c.fov_y = value.fovY;
    c.near_clip = value.near;
    c.far_clip = value.far;
    c.clear_color.write(value.clearColor);
    check(fy_camera_set(world.pointer, id, scratch.camera));
  }

  Light get light {
    if (_described case final d?) return _describedOr<Light>(d);
    check(fy_light_get(world.pointer, id, scratch.light));
    final l = scratch.light.ref;
    return switch (l.kind) {
      FY_LIGHT_DIRECTIONAL => Light.sun(color: l.color.read(), intensity: l.intensity),
      FY_LIGHT_AMBIENT => Light.ambient(color: l.color.read(), intensity: l.intensity),
      final kind => throw StateError('the engine returned light kind $kind, which this package:fynkel predates'),
    };
  }

  set light(Light value) => _described == null ? _writeLight(value) : _described!.set(value);

  void _writeLight(Light value) {
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
    if (_described case final d?) return _describedOr<MeshRenderer>(d);
    check(fy_mesh_renderer_get(world.pointer, id, scratch.meshRenderer));
    final r = scratch.meshRenderer.ref;
    return MeshRenderer(world._meshes[r.mesh], world._materials[r.material]);
  }

  set meshRenderer(MeshRenderer value) => _described == null ? _writeMeshRenderer(value) : _described!.set(value);

  void _writeMeshRenderer(MeshRenderer value) {
    final w = world;
    final mesh = value.mesh == null ? 0 : w._meshId(value.mesh!);
    final material = value.material == null ? 0 : w._materialId(value.material!);
    scratch.meshRenderer.ref
      ..mesh = mesh
      ..material = material;
    check(fy_mesh_renderer_set(w.pointer, id, scratch.meshRenderer));
  }

  @override
  String toString() => _world == null ? '$runtimeType (not added)' : '$runtimeType#$_id';
}

/// What an object is to be, until it's added: what its constructor and
/// setters said.
final class _Description {
  _Description(this.name, this.transform, this.components);

  String? name;
  bool active = true;
  Transform transform;
  final List<Component> components;
  final children = <GameObject>[];

  /// For a model's copy ([Model.instantiate]): the engine makes its objects.
  Model? model;

  T? get<T extends Component>() => transform is T ? transform as T : components.whereType<T>().firstOrNull;

  void set<T extends Component>(T component) {
    components.removeWhere((c) => c is T);
    components.add(component);
  }
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
