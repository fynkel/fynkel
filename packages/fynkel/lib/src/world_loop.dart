part of 'world.dart';

/// The parts of [World.update]: sending transforms, running motions and
/// routines, tidying, and turning objects on and off.
extension _Loop on World {
  void _sendTransforms() {
    if (_unsent.isEmpty) return;
    final w = pointer;
    final n = _unsent.length;
    final ids = scratch.ids(n);
    final values = scratch.transforms(n);
    // An FyTransform is ten floats: position, rotation, scale. Written
    // through the pointers, not asTypedList views: in Flutter's debug VM a
    // view made here once had a bad data pointer and crashed the process.
    final floats = values.cast<Float>();
    var count = 0;
    for (final object in _unsent) {
      if (!object._unsent) continue;
      object._unsent = false;
      final Transform(:position, :rotation, :scale) = object._transform!;
      ids[count] = object._id;
      floats
        ..[count * 10] = position.x
        ..[count * 10 + 1] = position.y
        ..[count * 10 + 2] = position.z
        ..[count * 10 + 3] = rotation.x
        ..[count * 10 + 4] = rotation.y
        ..[count * 10 + 5] = rotation.z
        ..[count * 10 + 6] = rotation.w
        ..[count * 10 + 7] = scale.x
        ..[count * 10 + 8] = scale.y
        ..[count * 10 + 9] = scale.z;
      count++;
    }
    _unsent.clear();
    check(fy_transform_set_many(w, ids, values, count));
  }

  /// Moves each running motion on by this frame's time: those made before
  /// this frame, whose owners aren't paused.
  void _runMotions() {
    final motions = _motions;
    if (motions.isEmpty) return;
    final frame = time._frame;
    var ended = false;
    final n = motions.length;
    for (var i = 0; i < n; i++) {
      final m = motions[i];
      if (!m.isRunning || m._group != null) {
        ended = true;
        continue;
      }
      if (m._made >= frame || _pausedOwner(m._owner)) continue;
      try {
        m._step(m.unscaled ? time._unscaledDelta : time._delta);
      } catch (error, stack) {
        _report(_objectOf(m._owner), error, stack);
        m.stop();
      }
      if (!m.isRunning) ended = true;
    }
    if (ended) motions.removeWhere(_notRunning);
  }

  static bool _notRunning(Motion m) => !m.isRunning || m._group != null;

  /// Stops the motions [owner] made, as it goes.
  void _haltMotions(List<Motion>? motions) {
    if (motions == null) return;
    for (final m in [...motions].reversed) {
      m._halt();
    }
  }

  Routine<T> _start<T>(Object owner, GameObject object, Future<T> Function() body) {
    if (isDisposed || object._destroyed) throw StateError('$owner was destroyed, so it can\'t start a routine');
    final parent = _currentRoutine;
    final base = _baseZone;
    final routine = Routine<T>._(this, owner, object, parent?._running == true ? parent : null);
    if (routine._parent case final p?) (p._children ??= []).add(routine);
    switch (owner) {
      case final GameObject o:
        (o._routines ??= []).add(routine);
      case final Behaviour b:
        (b._routines ??= []).add(routine);
    }
    final zone = base.fork(specification: _routineZone, zoneValues: {_routineKey: routine, _baseKey: base});
    final Future<T> future;
    try {
      future = zone.run(body);
    } catch (error, stack) {
      routine._fail(error, stack);
      return routine;
    }
    base.run(() => future.then(routine._finish, onError: routine._fail));
    return routine;
  }

  /// Ends what routines wait for, in the order the waits were made, each
  /// running its routine on to its next wait; then what those runs
  /// scheduled. Waits made during it end in a later frame. With [fixed],
  /// the waits for a fixed step ([fixedStep]), in a later step.
  void _runRoutines({bool fixed = false}) {
    final waits = fixed ? _fixedWaits : _waits;
    if (waits.isEmpty) return;
    _inRoutineStep = true;
    try {
      final n = waits.length;
      final now = fixed ? time._fixedSteps : time._frame;
      for (var i = 0; i < n; i++) {
        final wait = waits[i];
        if (wait.over || wait.made >= now || wait.routine._paused) continue;
        final bool ends;
        try {
          ends = wait.ends(time);
        } catch (error, stack) {
          wait.over = true;
          wait.fail(error, stack);
          _runMicrotasks();
          continue;
        }
        if (!ends) continue;
        wait.over = true;
        wait.end();
        _runMicrotasks();
      }
      _runMicrotasks();
    } finally {
      _inRoutineStep = false;
      waits.removeWhere((w) => w.over);
    }
  }

  void _runMicrotasks() {
    for (var i = 0; i < _microtasks.length; i++) {
      _microtasks[i]();
    }
    _microtasks.clear();
  }

  /// Unwinds [routine] from what it waits for.
  void _failWaits(Routine<Object?> routine) {
    for (final waits in [_waits, _fixedWaits]) {
      for (var i = 0; i < waits.length; i++) {
        final wait = waits[i];
        if (wait.over || !identical(wait.routine, routine)) continue;
        wait.over = true;
        wait.fail(RoutineStopped._(), StackTrace.current);
      }
    }
  }

  /// Stops the routines [owner] started, the last started first.
  void _stopRoutines(List<Routine<Object?>>? routines) {
    if (routines == null) return;
    for (final routine in [...routines].reversed) {
      routine.stop();
    }
  }

  /// Runs one of the systems' hooks on each system, in the order they were
  /// made; what one throws goes to [errors].
  void _runSystems(_SystemHook hook) {
    final systems = _systems;
    for (var i = 0; i < systems.length; i++) {
      final system = systems[i];
      try {
        switch (hook) {
          case _SystemHook.beforeFrame:
            system.beforeFrame();
          case _SystemHook.beforeUpdate:
            system.beforeUpdate();
          case _SystemHook.afterLateUpdate:
            system.afterLateUpdate();
          case _SystemHook.afterFrame:
            system.afterFrame();
        }
      } catch (error, stack) {
        _report(null, error, stack);
      }
    }
  }

  /// Runs the fixed steps this frame's time holds: in each, the systems'
  /// [WorldSystem.beforeFixedUpdate], every started, shown object's
  /// [GameObject.onFixedUpdate] and its behaviours', the systems'
  /// [WorldSystem.afterFixedUpdate], then the routines waiting for
  /// [fixedStep]. Objects added during a step start next frame.
  void _runFixedSteps() {
    final time = this.time;
    if (!time._fixedDue) return;
    final systems = _systems;
    try {
      while (time._fixedDue) {
        time._fixedTime += time._fixedDelta;
        time._fixedSteps++;
        time._inFixed = true;
        for (var i = 0; i < systems.length; i++) {
          final system = systems[i];
          try {
            system.beforeFixedUpdate();
          } catch (error, stack) {
            _report(null, error, stack);
          }
        }
        final n = _live.length;
        for (var i = 0; i < n && i < _live.length; i++) {
          final object = _live[i];
          if (!object._started || object._destroyed || _isHidden(object)) continue;
          try {
            object.onFixedUpdate();
          } catch (error, stack) {
            _report(object, error, stack);
          }
          final components = object._components;
          if (components == null) continue;
          for (var j = 0; j < components.length && !object._destroyed; j++) {
            if (components[j] case final Behaviour b when b._started && b._enabledFlag && !b._removed) {
              try {
                b.onFixedUpdate();
              } catch (error, stack) {
                _report(object, error, stack);
              }
            }
          }
        }
        for (var i = 0; i < systems.length; i++) {
          final system = systems[i];
          try {
            system.afterFixedUpdate();
          } catch (error, stack) {
            _report(null, error, stack);
          }
        }
        _runRoutines(fixed: true);
        if (isDisposed) return;
      }
    } finally {
      time._inFixed = false;
    }
  }

  /// Drops destroyed objects from the update order, and puts each moved
  /// one last with everything below it.
  void _tidy() {
    if (_collectedNew) _removeCollected();
    // Index loops, and only when there's something: a steady frame
    // allocates nothing, not even an iterator.
    if (_compact.isNotEmpty) {
      for (var i = 0; i < _compact.length; i++) {
        _compact[i]._components?.removeWhere((c) => c._takenOff);
      }
      _compact.clear();
    }
    if (_unsentGone) {
      _unsentGone = false;
      _unsent.removeWhere((o) => o._gone);
    }
    if (_liveDirty) {
      _live.removeWhere((o) => o._destroyed);
      _liveDirty = false;
    }
    if (_moved.isEmpty) return;
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

  /// Calls onEnable or onDisable on [root], everything below it, and their
  /// behaviours, wherever being shown changed: parents first, each object
  /// before its behaviours.
  void _refresh(GameObject root) {
    if (root._destroyed || root._world == null) return;
    for (final id in _subtreeIds(root._id)) {
      if (_objects[id] case final object?) _refreshObject(object);
    }
  }

  void _refreshObject(GameObject object) {
    final shown = object._appeared && !object._destroyed && !_isHidden(object);
    if (shown != object._enabled) {
      object._enabled = shown;
      _safely(object, shown ? object.onEnable : object.onDisable);
    }
    final components = object._components;
    for (var i = 0; components != null && i < components.length; i++) {
      if (components[i] case final Behaviour b) _refreshBehaviour(object, b);
    }
  }

  /// A behaviour is enabled while its object is, it's [Behaviour.enabled],
  /// loaded, and still on.
  void _refreshBehaviour(GameObject object, Behaviour b) {
    final on = object._enabled && b._enabledFlag && b._loaded && !b._removed;
    if (on == b._enabledNow) return;
    b._enabledNow = on;
    _safely(object, on ? b.onEnable : b.onDisable);
  }

  /// [id] and the ids of everything below it, parents first.
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
}

/// The [WorldSystem] hooks run once a frame.
enum _SystemHook { beforeFrame, beforeUpdate, afterLateUpdate, afterFrame }
