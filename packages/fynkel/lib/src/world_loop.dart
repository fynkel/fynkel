part of 'world.dart';

/// The parts of [World.update]: sending transforms, running motions and
/// routines, tidying, and turning objects on and off.
extension _Loop on World {
  void _sendTransforms() {
    if (_unsent.isEmpty) return;
    final w = pointer;
    final n = _unsent.length;
    final ids = scratch.ids(n);
    final floats = scratch.transformFloats(n);
    // An FyTransform is ten floats: position, rotation, scale. Written
    // through the pointers, not asTypedList views: in Flutter's debug VM a
    // view made here once had a bad data pointer and crashed the process.
    var count = 0;
    for (var i = 0; i < n; i++) {
      final object = _unsent[i];
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
    check(fy_transform_set_many(w, ids, scratch.transforms(), count));
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
      if (m._made >= frame || m._owner._paused) continue;
      try {
        m._step(m.unscaled ? time._unscaledDelta : time._delta);
      } catch (error, stack) {
        _report(m._owner._ownerObject, error, stack);
        m.stop();
      }
      if (!m.isRunning) ended = true;
    }
    if (ended) motions.removeWhere(_notRunning);
  }

  static bool _notRunning(Motion m) => !m.isRunning || m._group != null;

  /// Stops the motions [owner] made, as it goes.
  void _haltMotions(List<Motion>? motions) {
    if (motions == null || motions.isEmpty) return;
    for (final m in [...motions].reversed) {
      m._halt();
    }
  }

  Routine<T> _start<T>(_Moving owner, GameObject object, Future<T> Function() body) {
    if (isDisposed || object._destroyed) throw StateError('$owner was destroyed, so it can\'t start a routine');
    final parent = _currentRoutine;
    final base = _baseZone;
    final routine = Routine<T>._(this, owner, object, parent?._running == true ? parent : null);
    if (routine._parent case final p?) (p._children ??= []).add(routine);
    (owner._routines ??= []).add(routine);
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

  /// Runs what routines scheduled since the last step, then ends what
  /// they wait for, in the order the waits were made, each running its
  /// routine on to its next wait, then what that run scheduled. Waits made
  /// during it end in a later frame. With [fixed], the waits for a fixed
  /// step ([fixedStep]), in a later step.
  void _runRoutines({bool fixed = false}) {
    final waits = fixed ? _fixedWaits : _waits;
    if (waits.isEmpty && (fixed || _laterMicrotasks.isEmpty)) return;
    _inRoutineStep = true;
    try {
      if (!fixed && _laterMicrotasks.isNotEmpty) {
        _microtasks.addAll(_laterMicrotasks);
        _laterMicrotasks.clear();
        _runMicrotasks();
      }
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

  /// Unwinds [routine] from what it waits for. The world's lists let go
  /// of the waits at their next routines step.
  void _failWaits(Routine<Object?> routine) {
    final waits = routine._waits;
    if (waits == null) return;
    routine._waits = null;
    for (final wait in waits) {
      if (wait.over) continue;
      wait.over = true;
      wait.fail(RoutineStopped._(), StackTrace.current);
    }
  }

  /// Stops the routines [owner] started, the last started first.
  void _stopRoutines(List<Routine<Object?>>? routines) {
    if (routines == null || routines.isEmpty) return;
    for (final routine in [...routines].reversed) {
      routine.stop();
    }
  }

  /// Runs one of the systems' hooks on each system on, in their order;
  /// what one throws goes to [errors].
  void _runSystems(_SystemHook hook) {
    final systems = _systems;
    for (var i = 0; i < systems.length; i++) {
      final system = systems[i];
      if (system._off) continue;
      // What a hook did may have asked for the world to go: none after it.
      if (_disposeAsked) return;
      final from = profile._ticks;
      try {
        switch (hook) {
          case _SystemHook.beforeFrame:
            system.beforeFrame();
          case _SystemHook.beforeUpdate:
            system.beforeUpdate();
          case _SystemHook.afterUpdate:
            system.afterUpdate();
          case _SystemHook.afterLateUpdate:
            system.afterLateUpdate();
          case _SystemHook.afterFrame:
            system.afterFrame();
        }
      } catch (error, stack) {
        _systemThrew(system, error, stack);
      }
      profile._system(system, from);
    }
  }

  /// Reports what [system]'s hook threw, and turns the system off when its
  /// hooks have thrown in [WorldSystem.turnOffAfter] frames in a row.
  void _systemThrew(WorldSystem system, Object error, StackTrace stack) {
    if (system._off) return;
    final frame = time._frame;
    if (system._threwIn != frame) {
      system._throwingFrames = system._threwIn == frame - 1 ? system._throwingFrames + 1 : 1;
      system._threwIn = frame;
    }
    if (system._throwingFrames < WorldSystem.turnOffAfter) {
      _report(null, error, stack);
      return;
    }
    system._off = true;
    _report(
      null,
      StateError(
        '${system.name} threw in ${WorldSystem.turnOffAfter} frames in a row, so it was turned off: '
        'its hooks no longer run. The last error: $error',
      ),
      stack,
    );
  }

  /// Runs the fixed steps this frame's time holds: in each, the systems'
  /// [WorldSystem.beforeFixedUpdate], every started, shown object's
  /// [GameObject.onFixedUpdate] and its behaviours', the systems'
  /// [WorldSystem.afterFixedUpdate], then the routines waiting for
  /// [fixedStep]. Objects added during a step start next frame.
  /// Calls [call] on the first [count] live objects that are started and
  /// shown, and on their started, enabled behaviours, each guarded. A
  /// switch, not a closure, so a frame makes nothing.
  void _callEach(_Call call, int count) {
    for (var i = 0; i < count && i < _live.length; i++) {
      final object = _live[i];
      if (!object._started || object._destroyed || _isHidden(object)) continue;
      try {
        switch (call) {
          case _Call.update:
            object.onUpdate();
          case _Call.lateUpdate:
            object.onLateUpdate();
          case _Call.fixedUpdate:
            object.onFixedUpdate();
        }
      } catch (error, stack) {
        _report(object, error, stack);
      }
      final components = object._components;
      if (components == null) continue;
      for (var j = 0; j < components.length && !object._destroyed; j++) {
        if (components[j] case final Behaviour b when b._started && b._enabledFlag && !b._removed) {
          try {
            switch (call) {
              case _Call.update:
                b.onUpdate();
              case _Call.lateUpdate:
                b.onLateUpdate();
              case _Call.fixedUpdate:
                b.onFixedUpdate();
            }
          } catch (error, stack) {
            _report(object, error, stack);
          }
        }
      }
    }
  }

  void _runFixedSteps() {
    final time = this.time;
    if (!time._fixedDue) return;
    final systems = _systems;
    var steps = 0;
    try {
      while (time._fixedDue) {
        if (steps++ == time._maxFixedSteps) {
          time._dropFixedSteps();
          break;
        }
        time._fixedTime += time._fixedDelta;
        time._fixedSteps++;
        time._inFixed = true;
        input._inFixed = true;
        for (var i = 0; i < systems.length; i++) {
          final system = systems[i];
          if (system._off) continue;
          final from = profile._ticks;
          try {
            system.beforeFixedUpdate();
          } catch (error, stack) {
            _systemThrew(system, error, stack);
          }
          profile._system(system, from);
        }
        _callEach(_Call.fixedUpdate, _live.length);
        for (var i = 0; i < systems.length; i++) {
          final system = systems[i];
          if (system._off) continue;
          final from = profile._ticks;
          try {
            system.afterFixedUpdate();
          } catch (error, stack) {
            _systemThrew(system, error, stack);
          }
          profile._system(system, from);
        }
        _runRoutines(fixed: true);
        input._fixedStepDone();
        if (_disposeAsked) return;
      }
    } finally {
      time._inFixed = false;
      input._inFixed = false;
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
    if (_reparented.isNotEmpty) _reorderMoved();
  }

  /// Puts what was given a new parent last in the live list. Apart from
  /// [_tidy]: its closure captures, and the JIT allocates a closure's
  /// variables when the function starts, so in [_tidy] it would cost
  /// every frame.
  void _reorderMoved() {
    // Each moved object and what's below it go last, in the order they
    // moved (one moved later, inside another's subtree, goes after it),
    // parents before children: new orders for all of them, those in the
    // loop or not, and one pass over the live list.
    final moved = <GameObject>{};
    for (var i = 0; i < _reparented.length; i++) {
      final root = _reparented[i];
      if (root._destroyed) continue;
      for (final o in _subtree(root)) {
        o._order = _nextOrder++;
        if (o._inLoop) moved.add(o);
      }
    }
    _reparented.clear();
    if (moved.isEmpty) return;
    final tail = <GameObject>[];
    _live.removeWhere((o) {
      if (!moved.contains(o)) return false;
      tail.add(o);
      return true;
    });
    tail.sort((a, b) => a._order.compareTo(b._order));
    _live.addAll(tail);
  }

  /// Whether [object] is hidden, or hasn't appeared: as of the last
  /// [_refresh] of it, which every change of parent, flag or load makes.
  /// No walk, so the frame's loops ask it of every object.
  bool _isHidden(GameObject object) => !object._enabled;

  /// Calls onEnable or onDisable on [root], everything below it, and their
  /// behaviours, wherever being shown changed: parents first, each object
  /// before its behaviours, then what's below it.
  void _refresh(GameObject root) {
    if (root._destroyed || root._world == null) return;
    _refreshTree(root);
  }

  // Depth first, as Unity's, and with no list: hiding and showing pooled
  // objects allocates nothing. A child's onEnable or onDisable may move it
  // (or a sibling) out: the walk goes on from the same place then, so the
  // sibling that took its place isn't skipped.
  void _refreshTree(GameObject object) {
    _refreshObject(object);
    final children = object._children;
    for (var i = 0; children != null && i < children.length;) {
      final child = children[i];
      _refreshTree(child);
      if (i < children.length && identical(children[i], child)) i++;
    }
  }

  void _refreshObject(GameObject object) {
    final shown = object._appeared && !object._destroyed && _shown(object);
    if (shown != object._enabled) {
      object._enabled = shown;
      // Called here rather than torn off for [_safely], which allocates.
      try {
        shown ? object.onEnable() : object.onDisable();
      } catch (error, stack) {
        _report(object, error, stack);
      }
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
    try {
      on ? b.onEnable() : b.onDisable();
    } catch (error, stack) {
      _report(object, error, stack);
    }
  }
}

/// The [WorldSystem] hooks run once a frame.
enum _SystemHook { beforeFrame, beforeUpdate, afterUpdate, afterLateUpdate, afterFrame }

/// Which of the per-frame calls [_Loop._callEach] makes.
enum _Call { update, lateUpdate, fixedUpdate }
