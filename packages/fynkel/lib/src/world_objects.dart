part of 'world.dart';

/// Adding, loading, spawning and destroying objects, and knowing them by
/// class and by engine id.
extension _Objects on World {
  void _register(Object thing) => (_byClass[thing.runtimeType] ??= {}).add(thing);

  void _unregister(Object thing) {
    final same = _byClass[thing.runtimeType];
    if (same != null && same.remove(thing) && same.isEmpty) _byClass.remove(thing.runtimeType);
  }

  /// Puts an object the game didn't add (a model's part) in the update
  /// order, once something on it needs updates.
  void _enterLoop(GameObject object) {
    if (object._inLoop) return;
    object._inLoop = true;
    _live.add(object);
    _starting.add(object);
  }

  /// The object with engine id [id]: every live one is known, a model's
  /// parts included.
  GameObject _object(int id) => _objects[id] ?? (throw StateError('no object has id $id in this world'));

  // --- The hierarchy, kept in Dart (GameObject._parent, _children) ---

  /// Hangs [child] from [parent] (null: makes it a root), last among its
  /// children, in the engine and here: the only way an object's parent
  /// changes after it's added.
  void _setParent(GameObject child, GameObject? parent) {
    if (parent != null && !identical(parent._world, this)) {
      throw FynkelException(
        FynkelStatus.invalidValue,
        '$parent is in another world, so it can\'t be the parent of $child',
      );
    }
    // The engine refuses a cycle, or a destroyed parent, before anything
    // changes here.
    check(fy_parent_set(pointer, child._id, parent?._id ?? 0));
    _unlink(child);
    _link(child, parent);
    child._transformStamp = ++_transformVersion;
    _moved.add(child);
    _shape++;
    _refresh(child);
  }

  void _link(GameObject child, GameObject? parent) {
    child._parent = parent;
    if (parent != null) (parent._children ??= []).add(child);
  }

  void _unlink(GameObject child) {
    child._parent?._children?.remove(child);
    child._parent = null;
  }

  /// Turns [object]'s own flag on or off. While it loads it stays hidden,
  /// and takes the flag when it appears.
  void _setActive(GameObject object, bool value) {
    object._activeSelf = value;
    if (object._hiddenForLoad) return;
    check(fy_active_set(pointer, object._id, value));
    _shape++;
    _refresh(object);
  }

  /// Whether [object] is shown: it and everything above it on, and none
  /// still loading.
  bool _shown(GameObject object) {
    for (GameObject? o = object; o != null; o = o._parent) {
      if (!o._activeSelf || o._hiddenForLoad) return false;
    }
    return true;
  }

  /// [root] and everything below it, parents first, in children order.
  List<GameObject> _subtree(GameObject root) {
    final out = [root];
    for (var i = 0; i < out.length; i++) {
      final children = out[i]._children;
      if (children != null) out.addAll(children);
    }
    return out;
  }

  /// Makes the objects for the parts below [object], a model's copy just
  /// spawned, from the engine's tree: read once, here.
  void _readParts(GameObject object) {
    final w = pointer;
    final queue = [object];
    for (var i = 0; i < queue.length; i++) {
      final node = queue[i];
      final capacity = scratch.idCapacity;
      check(fy_children_get(w, node._id, scratch.ids(), capacity, scratch.count));
      final count = scratch.count.value;
      if (count > capacity) check(fy_children_get(w, node._id, scratch.ids(count), count, scratch.count));
      final ids = scratch.ids();
      final childIds = [for (var j = 0; j < count; j++) ids[j]];
      for (final id in childIds) {
        final part = _objects[id] = GameObject._live(this, id);
        _link(part, node);
        queue.add(part);
      }
    }
  }

  /// Spawns [object] under [parent] with everything added to it so far,
  /// then runs their loads, parents first. Those still loading when that
  /// returns are hidden until they're done.
  void _add(GameObject object, GameObject? parent) {
    final fresh = <GameObject>[];
    try {
      _attach(object, parent, fresh);
    } finally {
      for (final o in fresh) {
        _runLoad(o);
      }
      for (final o in fresh) {
        if (o._pending > 0 && !o._destroyed) _hideWhileLoading(o);
      }
    }
  }

  /// Spawns [object] in the engine under [parent], then what was added to
  /// it before it was live, and lists each in [fresh].
  void _attach(GameObject object, GameObject? parent, List<GameObject> fresh) {
    final described = object._described!;
    final transform = described.transform;
    GameObject._check(transform);
    final id = described.model == null ? _spawn() : _spawnModel(described.model!, described.transform);
    object
      .._world = this
      .._id = id;
    try {
      if (parent != null) check(fy_parent_set(pointer, id, parent.id));
      if (described.model != null) _readParts(object);
      if (described.name != null) object._writeName(described.name!);
      if (!described.active) check(fy_active_set(pointer, id, false));
      if (object._layer != 0) check(fy_layer_set(pointer, id, object._layer));
      for (final component in object._components ?? const <Component>[]) {
        if (!component._removed) object._write(component);
      }
    } catch (_) {
      for (final part in _subtree(object).skip(1)) {
        _objects.remove(part._id);
      }
      object._children = null;
      fy_despawn(_pointer, id);
      object
        .._world = null
        .._id = 0;
      rethrow;
    }
    object
      .._described = null
      .._activeSelf = described.active
      .._probed = described.model == null
      .._model = described.model
      .._inLoop = true
      .._pending = 1;
    if (described.asset case final asset?) (object._uses ??= []).add(asset);
    if (parent != null && parent._pending > 0) {
      object._waiter = parent;
      parent._pending++;
    }
    _link(object, parent);
    fresh.add(object);
    if (described.model == null) object._setTransform(transform);
    _objects[id] = object;
    _register(object);
    _live.add(object);
    _starting.add(object);
    for (final component in object._components ?? const <Component>[]) {
      if (component._removed) continue;
      _register(component);
      if (component is Behaviour) _starting.add(component);
    }
    _shape++;
    for (final child in described.children) {
      child._queuedIn = null;
      _attach(child, object, fresh);
    }
  }

  /// Runs [object]'s [GameObject.onLoad], then its behaviours'
  /// [Behaviour.onLoad]s: each done at once when it returns no future,
  /// else when its future completes. The object waits for all of them.
  void _runLoad(GameObject object) {
    if (object._destroyed) return;
    final behaviours = [
      for (final c in object._components ?? const <Component>[])
        if (c is Behaviour && !c._removed) c,
    ];
    object._pending += behaviours.length;
    _load(object, null, object.onLoad);
    for (final b in behaviours) {
      _load(object, b, b.onLoad);
    }
  }

  void _load(GameObject object, Behaviour? behaviour, FutureOr<void> Function() onLoad) {
    if (object._destroyed) return;
    final FutureOr<void> result;
    try {
      result = onLoad();
    } catch (error, stack) {
      _loadFailed(object, error, stack);
      return;
    }
    if (result is Future<void>) {
      result.then(
        (_) => _loadDone(object, behaviour),
        onError: (Object error, StackTrace stack) => _loadFailed(object, error, stack),
      );
    } else {
      _loadDone(object, behaviour);
    }
  }

  void _loadDone(GameObject object, [Behaviour? behaviour]) {
    behaviour?._loaded = true;
    if (object._destroyed) return;
    object._pending--;
    _appearIfDone(object);
  }

  /// Puts [component] on [object], which is live: the engine's are written
  /// by the caller; a behaviour loads, with the object if it's still
  /// loading, else by itself (a failed load removes only the behaviour).
  void _componentAdded(GameObject object, Component component) {
    _register(component);
    final systems = _systems;
    for (var i = 0; i < systems.length; i++) {
      try {
        systems[i].componentAdded(object, component);
      } catch (error, stack) {
        _report(null, error, stack);
      }
    }
    if (component is! Behaviour) return;
    _enterLoop(object);
    _starting.add(component);
    if (!object._appeared) {
      object._pending++;
      _load(object, component, component.onLoad);
      return;
    }
    void failed(Object error, StackTrace stack) {
      if (component._removed || object._destroyed || isDisposed) return;
      object.removeComponent(component);
      _report(object, error, stack);
    }

    final FutureOr<void> result;
    try {
      result = component.onLoad();
    } catch (error, stack) {
      failed(error, stack);
      return;
    }
    if (result is Future<void>) {
      result.then((_) {
        component._loaded = true;
        if (!isDisposed) _refreshBehaviour(object, component);
      }, onError: failed);
    } else {
      component._loaded = true;
      _refreshBehaviour(object, component);
    }
  }

  /// Destroys the object whose load failed and reports why. A load that
  /// fails after its object was destroyed (it found the object gone) is
  /// what destroying it mid-load means, and is ignored.
  void _loadFailed(GameObject object, Object error, StackTrace stack) {
    if (object._destroyed || isDisposed) return;
    object._failLoaded(error, stack);
    object.destroy();
    _report(object, error, stack);
  }

  /// Shows [object] once its own load and those of everything added
  /// during it are done, then tells the object waiting for it. The
  /// topmost to appear enables itself and what's below it at once.
  void _appearIfDone(GameObject object) {
    if (object._pending > 0 || object._destroyed) return;
    object._appeared = true;
    if (object._hiddenForLoad) {
      object._hiddenForLoad = false;
      if (object._activeSelf) {
        check(fy_active_set(pointer, object._id, true));
        _shape++;
      }
    }
    object._loadedCompleter?.complete();
    object._loadedCompleter = null;
    final waiter = object._waiter;
    object._waiter = null;
    if (waiter != null) {
      waiter._pending--;
      _appearIfDone(waiter);
    } else {
      _refresh(object);
    }
  }

  void _hideWhileLoading(GameObject object) {
    object._hiddenForLoad = true;
    check(fy_active_set(pointer, object._id, false));
    _shape++;
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
  /// their Dart objects. Each, children before parents, while it's still
  /// there to read, gets [GameObject.onDisable] if it was enabled, then
  /// its enabled behaviours theirs, then [GameObject.onDestroy], then its
  /// behaviours theirs.
  void _destroy(GameObject root) {
    final objects = _subtree(root);
    for (final object in objects) {
      object._destroyed = true;
    }
    try {
      for (final object in objects.reversed) {
        final components = object._components ?? const <Component>[];
        if (object._enabled) {
          object._enabled = false;
          _safely(object, object.onDisable);
        }
        for (final c in components) {
          if (c case final Behaviour b when b._enabledNow) {
            b._enabledNow = false;
            _safely(object, b.onDisable);
          }
        }
        for (final c in components.reversed) {
          if (c case final Behaviour b when !b._removed) {
            _stopRoutines(b._routines);
            _haltMotions(b._motions);
            b._listening?.cancelAll();
          }
        }
        _stopRoutines(object._routines);
        _haltMotions(object._motions);
        object._listening?.cancelAll();
        _safely(object, object.onDestroy);
        for (final c in components) {
          if (c case final Behaviour b when !b._removed) _safely(object, b.onDestroy);
        }
      }
    } finally {
      check(fy_despawn(pointer, root._id));
      _unlink(root);
      for (final object in objects) {
        if (object._unsent) _unsentGone = true;
        object
          .._gone = true
          .._transform = null
          .._unsent = false;
        _objects.remove(object._id);
        _unregister(object);
        for (final c in object._components ?? const <Component>[]) {
          if (c._removed) continue;
          c._removed = true;
          _unregister(c);
        }
      }
      _liveDirty = true;
      _shape++;
      _forget(objects);
    }
  }

  /// What destroying [objects] means beyond the engine: loads waiting for
  /// them go on without them, and what they owned and the assets they
  /// held are freed, if no one else holds them.
  void _forget(List<GameObject> objects) {
    final waiters = <GameObject>[];
    for (final object in objects) {
      if (!object._appeared) {
        object._failLoaded(StateError('$object was destroyed before it appeared'), StackTrace.current);
      }
      object._pending = 0;
      if (object._waiter case final waiter? when !waiter._destroyed) waiters.add(waiter);
      object._waiter = null;
    }
    for (final object in objects) {
      for (final asset in object._uses ?? const <_Asset>[]) {
        _release(asset, object);
      }
      object._uses = null;
    }
    for (final waiter in waiters) {
      waiter._pending--;
      _appearIfDone(waiter);
    }
  }
}
