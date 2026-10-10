import 'dart:async';
import 'dart:collection';
import 'dart:convert';
import 'dart:ffi';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:ffi/ffi.dart';
import 'package:meta/meta.dart';

import 'bindings.g.dart';
import 'color.dart';
import 'error.dart';
import 'math.dart';
import 'native.dart';

part 'assets.dart';
part 'cameras.dart';
part 'components.dart';
part 'debug.dart';
part 'input.dart';
part 'keys.dart';
part 'layers.dart';
part 'model.dart';
part 'motion.dart';
part 'pointing.dart';
part 'resources.dart';
part 'routines.dart';
part 'signals.dart';
part 'time.dart';

/// Everything in a scene: objects, their components, and the meshes and
/// materials they use. A world lives in the engine; free it with
/// [dispose] (or it's freed when it's garbage collected).
final class World implements Finalizable {
  /// A new, empty world. [source] gives the bytes of an asset by name,
  /// for [assets]; package:fynkel_flutter passes the app's bundle.
  World({this._source}) {
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
    // What the objects owned and the assets still held: the engine's side
    // goes with the world, the rest goes after it.
    final owned = [
      for (final object in _objects.values) ...?object._owned,
      for (final asset in _assets.values) ?asset.value,
    ];
    // Nothing of a disposed world hears a signal again, or holds one.
    for (final object in _objects.values) {
      object._stopListening();
    }
    for (final wait in _waits) {
      wait.over = true;
    }
    _finalizer.detach(this);
    _debug?._release();
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
    _compact.clear();
    _waits.clear();
    _microtasks.clear();
    _motions.clear();
    _moved.clear();
    _off.clear();
    _hidden.clear();
    _unsent.clear();
    _assets.clear();
    _free(owned, null);
  }

  // --- Errors ---

  final _errors = StreamController<ObjectError>.broadcast();

  /// Errors no caller could catch: an [GameObject.onLoad] that failed
  /// (the object is destroyed, and its parent's load goes on without it),
  /// what couldn't be freed when an object was destroyed, and what an
  /// object's or behaviour's own code threw from onStart, onUpdate,
  /// onLateUpdate, a pointer handler, onEnable, onDisable or onDestroy (the frame goes
  /// on, as in Unity: one object's bug doesn't stop the others). With no
  /// listener, each is an uncaught error of the current zone (for a load,
  /// the zone the object was added in), as an unawaited future's would
  /// be; listen to handle them, or rethrow to stop.
  Stream<ObjectError> get errors => _errors.stream;

  void _report(GameObject? object, Object error, StackTrace stack) {
    if (_errors.hasListener) {
      _errors.add(ObjectError._(object, error, stack));
    } else {
      Zone.current.handleUncaughtError(error, stack);
    }
  }

  /// Runs one of [object]'s calls (or its behaviour's) that isn't run
  /// every frame, reporting what it throws.
  void _safely(GameObject object, void Function() call) {
    try {
      call();
    } catch (error, stack) {
      _report(object, error, stack);
    }
  }

  // --- Time ---

  /// The world's clock: [Time.delta], [Time.scale], [Time.sinceStart] and
  /// the rest. Objects and behaviours read it as `time`.
  final Time time = Time._();

  // --- Assets ---

  final AssetSource? _source;
  final _assets = <String, _Asset>{};

  /// Assets held by the world itself, for as long as it lives: what
  /// `world.assets.preload` loads stays. Objects use their own
  /// [GameObject.assets], so what they load goes with them.
  late final Assets assets = Assets._(this, null);

  /// Disposes [things]: materials first (they use textures), then meshes,
  /// textures and models. What can't be disposed (still used elsewhere)
  /// is reported, not thrown, as this runs while objects are destroyed.
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
              texture.dispose();
          }
        } catch (error, stack) {
          if (!isDisposed) _report(object, error, stack);
        }
      }
    }
  }

  // --- The loop ---

  // Every object the game added, in the order it became live, so parents
  // come before their children. Destroyed ones are dropped and moved ones
  // put last at the start of the next update.
  final _live = <GameObject>[];
  var _liveDirty = false;
  final _moved = <GameObject>[];

  // Live objects and behaviours that haven't had onStart yet, in the same
  // order, each object before its behaviours.
  var _starting = <Object>[];

  // Objects with removed components still in their lists: taken out at the
  // start of the next update, so a removal never shifts a list being run.
  final _compact = <GameObject>[];

  // Objects whose own flag is off, and the ids of everything they hide,
  // worked out again when [_shape] has changed since [_hiddenAt]. Only
  // Dart turns objects off, so it knows them all; with none off (the
  // usual case) nothing is worked out at all.
  final _off = <GameObject>{};
  final _hidden = <int>{};
  var _shape = 0;
  var _hiddenAt = -1;

  // Objects whose transform the game set since it was last sent, in the
  // order they were first set. [_sendTransforms] sends them all in one
  // call before anything reads the engine's transforms: a view's frame, a
  // raycast, a world transform, a tap.
  final _unsent = <GameObject>[];

  // Whether a destroyed object is in [_unsent], to be dropped at the next
  // update: with no view drawing, nothing else would let go of it.
  var _unsentGone = false;

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

  /// What the player does: fingers, gestures, keys and named controls.
  /// The view feeds it; [update] runs its step first. Objects read it as
  /// [GameObject.input].
  late final Input input = Input().._targets = _Pointing(this);

  /// The virtual cameras, their blends, and shaking the view's camera
  /// (docs/stage-4.md §3.8).
  late final Cameras cameras = Cameras._(this);

  /// Lines drawn over the scene, to see what the code sees
  /// (docs/stage-4.md §3.9). Objects and behaviours reach it as `debug`.
  Debug get debug => _debug ??= Debug._(this);
  Debug? _debug;

  /// The object the view draws from: the one with a [Camera] and the
  /// lowest id; null when there's none.
  GameObject? get camera {
    final w = pointer;
    _sendTransforms();
    check(fy_camera_active(w, scratch.id));
    final id = scratch.id.value;
    return id == 0 ? null : _object(id);
  }

  // The objects and behaviours still waiting to start, reused each frame.
  var _waiting = <Object>[];

  /// Runs one frame of the game, [dt] seconds after the last one: [time]
  /// moves on (see [Time.delta]), then, in this order:
  ///
  /// 1. **Input.** [input]'s events since the last frame, in order, then
  ///    its gestures ([Input]): its signals, and the objects' pointer
  ///    handlers ([GameObject.onPointerDown] and the rest), cast into the
  ///    world from the camera the view draws from.
  /// 2. **Starts.** Each object added since the last frame gets
  ///    [GameObject.onStart], once, parents before children, each right
  ///    before its behaviours' [Behaviour.onStart]. Objects added during a
  ///    start start too.
  /// 3. **Updates.** Each started object gets [GameObject.onUpdate],
  ///    then its started behaviours get [Behaviour.onUpdate] in the order
  ///    they were added, parents before children. Objects and behaviours
  ///    added during an update start next frame; objects destroyed during
  ///    it get no more calls, even this frame. A new [GameObject.parent]
  ///    counts from the next frame.
  /// 4. **Motions and routines.** Each running [Motion] moves on
  ///    ([GameObject.tween], [GameObject.moveTo]...), in the order they
  ///    were made, then the routines' waits end ([GameObject.start]).
  /// 5. **Late updates.** Then each object updated this frame, and its
  ///    behaviours, get [GameObject.onLateUpdate] in the same order: for
  ///    what follows where the others went.
  /// 6. **Cameras.** The camera rigs move their objects ([Follow] and
  ///    [Orbit], then [LookAt]), then [cameras] puts the view's camera
  ///    where the live [VirtualCamera] is, and shakes it.
  /// 7. **Debug lines.** What was drawn with [debug] goes to the engine,
  ///    to be drawn over the frame.
  ///
  /// Hidden objects (inactive, or below one that is) and their behaviours
  /// are skipped by the last five and start when they're first shown; so are
  /// disabled behaviours ([Behaviour.enabled]) and those still loading. A
  /// view then draws the world as it is. What their code throws goes to
  /// [errors], and the frame goes on.
  ///
  /// Throws an [ArgumentError] if [dt] is negative or NaN.
  void update(double dt) {
    pointer;
    time._advance(dt);
    _tidy();

    input
      .._now = time._unscaledSinceStart
      .._step();

    final waiting = _waiting;
    for (var i = 0; i < _starting.length; i++) {
      switch (_starting[i]) {
        case final GameObject object:
          if (object._destroyed) continue;
          if (_isHidden(object)) {
            waiting.add(object);
            continue;
          }
          object._started = true;
          _safely(object, object.onStart);
        case final Behaviour behaviour:
          final object = behaviour._owner!;
          if (behaviour._removed || object._destroyed) continue;
          if (!behaviour._loaded || !behaviour._enabledFlag || !object._started || _isHidden(object)) {
            waiting.add(behaviour);
            continue;
          }
          behaviour._started = true;
          _safely(object, behaviour.onStart);
      }
    }
    // The two lists swap, so a frame makes none.
    _waiting = _starting..clear();
    _starting = waiting;

    // Objects added from here on are past the end when they come, or not
    // started: they wait for the next frame. The calls are guarded inline,
    // not through [_safely], so a frame makes no closures.
    final updated = _live.length;
    for (var i = 0; i < updated; i++) {
      final object = _live[i];
      if (!object._started || object._destroyed || _isHidden(object)) continue;
      try {
        object.onUpdate();
      } catch (error, stack) {
        _report(object, error, stack);
      }
      final components = object._components;
      if (components == null) continue;
      for (var j = 0; j < components.length && !object._destroyed; j++) {
        if (components[j] case final Behaviour b when b._started && b._enabledFlag && !b._removed) {
          try {
            b.onUpdate();
          } catch (error, stack) {
            _report(object, error, stack);
          }
        }
      }
    }

    _runMotions();
    _runRoutines();

    for (var i = 0; i < updated && i < _live.length; i++) {
      final object = _live[i];
      if (!object._started || object._destroyed || _isHidden(object)) continue;
      try {
        object.onLateUpdate();
      } catch (error, stack) {
        _report(object, error, stack);
      }
      final components = object._components;
      if (components == null) continue;
      for (var j = 0; j < components.length && !object._destroyed; j++) {
        if (components[j] case final Behaviour b when b._started && b._enabledFlag && !b._removed) {
          try {
            b.onLateUpdate();
          } catch (error, stack) {
            _report(object, error, stack);
          }
        }
      }
    }

    cameras._run();
    _debug?._flush();
  }

  // --- Motions ---

  // The running motions not in a group, in the order they were made.
  final _motions = <Motion>[];

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

  // --- Routines ---

  // What routines wait for, in the order they were made, and the
  // microtasks routines scheduled during [_runRoutines].
  final _waits = <_Wait>[];
  final _microtasks = <void Function()>[];
  var _inRoutineStep = false;

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
  /// scheduled. Waits made during it end in a later frame.
  void _runRoutines() {
    if (_waits.isEmpty) return;
    _inRoutineStep = true;
    try {
      final n = _waits.length;
      final frame = time._frame;
      for (var i = 0; i < n; i++) {
        final wait = _waits[i];
        if (wait.over || wait.made >= frame || wait.routine._paused) continue;
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
      _waits.removeWhere((w) => w.over);
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
    for (var i = 0; i < _waits.length; i++) {
      final wait = _waits[i];
      if (wait.over || !identical(wait.routine, routine)) continue;
      wait.over = true;
      wait.fail(RoutineStopped._(), StackTrace.current);
    }
  }

  /// Stops the routines [owner] started, the last started first.
  void _stopRoutines(List<Routine<Object?>>? routines) {
    if (routines == null) return;
    for (final routine in [...routines].reversed) {
      routine.stop();
    }
  }

  /// Drops destroyed objects from the update order, and puts each moved
  /// one last with everything below it.
  void _tidy() {
    // Index loops, and only when there's something: a steady frame
    // allocates nothing, not even an iterator.
    if (_compact.isNotEmpty) {
      for (var i = 0; i < _compact.length; i++) {
        _compact[i]._components?.removeWhere((c) => c._removed);
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
    _add(root, null);
    return root;
  }

  /// Every live object and component of type [T] (a class, a subclass,
  /// or an interface they implement) that the game added, grouped by
  /// class, each class in the order they were added: `world.all<Enemy>()`,
  /// `world.all<Health>()`. The parts a model makes, and the components the
  /// engine gave them, aren't included: find them by name.
  List<T> all<T extends Object>() => [
    for (final objects in _byClass.values)
      if (objects.first is T) ...objects.cast<T>(),
  ];

  /// The one live object or component of type [T] that the game added:
  /// `world.only<Score>()`. Throws a [StateError] if there's none or more
  /// than one, so a game's singletons need no `static instance`. See
  /// [all].
  T only<T extends Object>() {
    T? found;
    var count = 0;
    for (final objects in _byClass.values) {
      if (objects.first is! T) continue;
      found ??= objects.first as T;
      count += objects.length;
    }
    if (count == 1) return found!;
    throw StateError(
      count == 0
          ? 'there is no $T in this world; only<$T>() needs exactly one'
          : 'there are $count of $T in this world; only<$T>() needs exactly one (all<$T>() gives them all)',
    );
  }

  /// The first live object or component of type [T] that the game added,
  /// or null. See [all].
  T? first<T extends Object>() {
    for (final objects in _byClass.values) {
      if (objects.first is T) return objects.first as T;
    }
    return null;
  }

  // The Dart object for each live engine id that has been asked for, so
  // the same instance always comes back: the game's own, or a plain one
  // made the first time a model's part is asked for.
  final _objects = <int, GameObject>{};

  // The objects and components the game added, by class, for [all] and
  // [first].
  final _byClass = <Type, Set<Object>>{};

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

  GameObject _object(int id) => _objects[id] ??= GameObject._live(this, id);

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
      if (described.name != null) object._writeName(described.name!);
      if (!described.active) check(fy_active_set(pointer, id, false));
      if (object._layer != 0) check(fy_layer_set(pointer, id, object._layer));
      for (final component in object._components ?? const <Component>[]) {
        if (!component._removed) object._write(component);
      }
    } catch (_) {
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
      .._inLoop = true
      .._pending = 1;
    if (described.asset case final asset?) (object._uses ??= []).add(asset);
    if (parent != null && parent._pending > 0) {
      object._waiter = parent;
      parent._pending++;
    }
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
    if (!described.active) _off.add(object);
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
        _off.remove(object);
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
    _off.add(object);
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
  void _destroy(int id) {
    final ids = _subtreeIds(id).toList();
    final objects = [for (final id in ids) ?_objects[id]];
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
      check(fy_despawn(pointer, id));
      for (final object in objects) {
        if (object._unsent) _unsentGone = true;
        object
          .._gone = true
          .._transform = null
          .._unsent = false;
        _objects.remove(object._id);
        _off.remove(object);
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
    _free([for (final object in objects) ...?object._owned], objects.firstOrNull);
    for (final object in objects) {
      object._owned = null;
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
  /// ([Material.alphaCutoff]) are hit as if solid, and only objects on
  /// [layers] are hit ([GameObject.layer]): the ray goes through the rest.
  /// No physics is involved: each object's box is tested, then the
  /// triangles of those the ray may hit.
  RayHit? raycast(Ray ray, {double maxDistance = double.infinity, LayerMask layers = LayerMask.all}) {
    _sendTransforms();
    final r = scratch.ray.ref;
    r.origin.write(ray.origin);
    r.direction.write(ray.direction);
    check(fy_raycast(pointer, scratch.ray, maxDistance, layers.value, scratch.rayHit));
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

/// An object in a [World]: a [Transform], its components (the engine's
/// [Camera], [Light] and [MeshRenderer], and any number of the game's
/// [Behaviour]s), and the objects added to it.
///
/// Make one, then add it: `world.add(GameObject(...))` or
/// `parent.add(child)`. Until then it only describes itself: its
/// [transform], [name] and components can be read and changed, and objects
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
class GameObject with _Moving {
  /// A new object, not yet in any world, with [transform] (the identity if
  /// not given), [components] (as [addComponent] puts them on) and
  /// [children] (as [add] adds them), so a whole prefab can be one
  /// expression:
  ///
  /// ```dart
  /// GameObject(name: 'enemy', components: [Health(30), Chase()], children: [Body()])
  /// ```
  GameObject({
    String? name,
    Transform? transform,
    Iterable<Component> components = const [],
    Iterable<GameObject> children = const [],
    int layer = 0,
  }) : _described = _Description(name, transform ?? Transform()) {
    _layer = _checkLayer(layer);
    components.forEach(addComponent);
    children.forEach(add);
  }

  /// The object for an engine id the game didn't make, such as a model's
  /// part.
  GameObject._live(World world, int id) : _world = world, _id = id, _appeared = true, _probed = false;

  World? _world;
  int _id = 0;

  // Its layer, kept here: only the game sets one, so it's never read back.
  int _layer = 0;

  // What the object is to be, until it's added.
  _Description? _described;

  // The object this one was added to before either was live.
  GameObject? _queuedIn;

  // Destroyed: from [destroy] on. Gone: out of the engine, once its
  // onDestroy calls are done.
  bool _destroyed = false;
  bool _gone = false;
  bool _started = false;

  // Whether it had onEnable and not onDisable since: it's shown.
  bool _enabled = false;

  // The routines it started that are still running, and the signals it
  // listens to with [on].
  List<Routine<Object?>>? _routines;
  _Listening? _listening;

  // Its components, in the order they were put on; removed ones stay until
  // the world's next update takes them out. For an object the engine made
  // (a model's part), its engine components are read the first time
  // they're asked for ([_probed]). [_inLoop]: in the world's update order.
  List<Component>? _components;
  bool _probed = true;
  bool _inLoop = false;

  // The transform as the game last set or read it, so reading it makes no
  // call, and whether it waits in [World._unsent]. Null until first read
  // or set, and once the object is gone.
  Transform? _transform;
  bool _unsent = false;

  // Loading: how many loads this object waits for (its own, and those of
  // the objects added during it), the object waiting for this one, and
  // whether it's hidden until they're done. [_activeSelf] is the game's
  // own flag, which the engine's follows once the object has appeared.
  int _pending = 0;
  GameObject? _waiter;
  bool _appeared = false;
  bool _hiddenForLoad = false;
  bool _activeSelf = true;
  Completer<void>? _loadedCompleter;
  (Object, StackTrace)? _loadFailure;

  // What the object frees when it's destroyed: what it [own]s, and the
  // assets it holds a use of.
  List<Object>? _owned;
  List<_Asset>? _uses;
  Assets? _assets;

  @override
  GameObject get _self => this;

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

  /// The engine's world, for what needs this object there; a
  /// [StateError] saying [what] can't be done once it's destroyed.
  Pointer<FyWorld> _engine(String what) {
    if (_gone) throw StateError('$this was destroyed, so $what');
    return world.pointer;
  }

  StateError _notAdded(String what) => StateError(
    '$this has no $what yet: it isn\'t added to a world. Add it first (world.add, or add to an object '
    'that is), and touch the world from its onLoad on.',
  );

  // --- Loading ---

  /// Called once, when the object is added: where it gets its assets
  /// and adds its children. Until it, and everything added during it,
  /// are loaded, the object is hidden: not drawn, not hit, not started or
  /// updated. Then it appears whole, and [loaded] completes.
  ///
  /// Return a future (`Future<void> onLoad() async`) to load something;
  /// an object that loads nothing appears at once, in the same `add`.
  /// A load that throws destroys the object and goes to [World.errors];
  /// the parent's load goes on without it. If the object is destroyed
  /// while it loads, its load finds it gone at its next step (adding to
  /// it and loading for it throw a [StateError]) and is ignored, and
  /// what it loaded is freed.
  @mustCallSuper
  @protected
  FutureOr<void> onLoad() {}

  /// Completes when the object has appeared: it and everything added
  /// during its [onLoad] are loaded. Fails with the load's error if its
  /// load failed, or with a [StateError] if it was destroyed first.
  ///
  /// To keep one level on screen until the next is ready:
  /// `final forest = world.add(Forest()); await forest.loaded; garden.destroy();`.
  Future<void> get loaded {
    if (_appeared) return Future.value();
    if (_loadFailure case (final error, final stack)) return Future.error(error, stack);
    return (_loadedCompleter ??= Completer<void>()).future;
  }

  void _failLoaded(Object error, StackTrace stack) {
    if (_appeared || _loadFailure != null) return;
    _loadFailure = (error, stack);
    _loadedCompleter?.completeError(error, stack);
    _loadedCompleter = null;
  }

  /// The world's assets, loaded for this object: each one it asks for is
  /// held until it's destroyed, and freed then if no one else holds it.
  Assets get assets => _assets ??= Assets._(_world ?? (throw _notAdded('assets')), this);

  /// Gives [resource] (a [Mesh], [Texture2D], [Material] or [Model] the
  /// game made itself) to this object, to be disposed when it's
  /// destroyed, and returns it: `own(Material(baseColorTexture: grass))`.
  /// Materials are disposed before meshes and textures, as they use them.
  T own<T extends Object>(T resource) {
    if (_destroyed) throw StateError('$this was destroyed, so it can\'t own anything');
    switch (resource) {
      case Mesh(_shape: true):
        throw ArgumentError.value(resource, 'resource', 'is built in and is never disposed');
      case Mesh() || Texture2D() || Material() || Model():
        (_owned ??= []).add(resource);
        return resource;
      default:
        throw ArgumentError.value(resource, 'resource', 'is not a Mesh, Texture2D, Material or Model');
    }
  }

  // --- The loop ---

  /// What the player did since the last frame: the world's [World.input].
  Input get input => world.input;

  /// The world's [World.debug]: lines drawn over the scene.
  Debug get debug => world.debug;

  /// The world's clock: `time.delta` is the seconds this frame counts for.
  /// See [Time].
  Time get time => world.time;

  /// Starts a routine: runs [body] now, up to its first wait, and returns
  /// the [Routine], which can be awaited for what [body] returns, or
  /// stopped. Its waits ([nextFrame], [wait], [until], [whileTrue],
  /// [race], [all]) end in the routines step of a later frame, after
  /// every [onUpdate] (docs/stage-4.md §3.2):
  ///
  /// ```dart
  /// start(() async {
  ///   while (true) {
  ///     await wait(2);
  ///     add(Coin());
  ///   }
  /// });
  /// ```
  ///
  /// It stops when this object is destroyed, and is paused while it's
  /// hidden. Throws a [StateError] before the object is added.
  Routine<T> start<T>(Future<T> Function() body) => world._start(this, this, body);

  /// Calls [handler] with each value [signal] emits, until this object is
  /// destroyed (with its routines: after its onDisable, before its
  /// onDestroy) or the subscription is cancelled. The usual way to listen:
  /// a destroyed object never hears anything, and nothing holds it. What
  /// [handler] throws goes to [World.errors]. It hears signals while it's
  /// hidden too, as in Unity.
  SignalSubscription<T> on<T>(Signal<T> signal, void Function(T value) handler) {
    if (_destroyed) throw StateError('$this was destroyed, so it can\'t listen');
    return _on(this, signal, handler);
  }

  /// Stops listening with [on], for this object and its behaviours.
  void _stopListening() {
    _listening?.cancelAll();
    for (final c in _components ?? const <Component>[]) {
      if (c is Behaviour) c._listening?.cancelAll();
    }
  }

  /// Called each time the object is shown: when it appears (right after
  /// its load, with everything added during it), and when it, or one
  /// above it, is turned on ([active]) or it's moved out from below a
  /// hidden one. Parents come first, each before its behaviours.
  @mustCallSuper
  @protected
  void onEnable() {}

  /// Called once, in the first [World.update] after the object is added
  /// and shown, before its first [onUpdate]. Its parent's comes first.
  @mustCallSuper
  @protected
  void onStart() {}

  /// Called every [World.update], parents before children, while the
  /// object is shown. Override it to move, animate or think, by
  /// [Time.delta] (`time.delta`); a mixin that overrides it calls
  /// `super.onUpdate()` so the next one runs too.
  @mustCallSuper
  @protected
  void onUpdate() {}

  /// Called every [World.update] after every object's [onUpdate], in the
  /// same order: for what follows where the others went, such as a camera.
  @mustCallSuper
  @protected
  void onLateUpdate() {}

  /// Called each time a shown object is hidden: it or one above it is
  /// turned off, it's moved below a hidden one, or it's destroyed (right
  /// before [onDestroy]). Parents come first, each before its behaviours.
  @mustCallSuper
  @protected
  void onDisable() {}

  /// Called when the object, or one above it, is destroyed: children
  /// before parents, while the object is still there to read (where it
  /// was, what it carried). Adding to it then throws.
  @mustCallSuper
  @protected
  void onDestroy() {}

  // --- Pointers (docs/stage-4.md §3.6) ---
  //
  // As Unity's IPointerDownHandler and the rest. What's under a finger is
  // cast from the camera the view draws from; [Finger.over] is the hit.

  /// A finger went down over this object or something below it that didn't
  /// take it. Return true to take it: this object then gets its
  /// [onPointerUp]. False (the default) passes it to its behaviours, then
  /// to the parent.
  @protected
  bool onPointerDown(Finger finger) => false;

  /// The finger whose [onPointerDown] this object took went up, wherever
  /// it is.
  @protected
  void onPointerUp(Finger finger) {}

  /// A finger came over this object or something below it: the mouse
  /// hovering, or a touch. Each object it enters gets it, from the top
  /// down, as in Unity.
  @protected
  void onPointerEnter(Finger finger) {}

  /// A finger is no longer over this object or anything below it (it
  /// moved off, went up, or left the view).
  @protected
  void onPointerExit(Finger finger) {}

  /// A tap ([Input.onTap]) on this object or something below it that
  /// didn't take it ([Finger.over] is the hit; [Finger.tapCount] is 2 for
  /// a double tap). Return true to take it; false (the default) passes it
  /// to its behaviours, then to the parent.
  @protected
  bool onClick(Finger finger) => false;

  /// A long press ([Input.onLongPress]) on this object or below; bubbles
  /// as [onClick] does.
  @protected
  bool onLongPress(Finger finger) => false;

  /// A drag started from this object or something below it
  /// ([Finger.startedOver]). Return true to take it: this object then gets
  /// the drag's [onDrag] and [onDragEnd], wherever the finger goes. False
  /// (the default) passes it on, as [onClick] does.
  @protected
  bool onDragStart(Finger finger) => false;

  /// The drag this object took moved: [Finger.delta] is the frame's move.
  @protected
  void onDrag(Finger finger) {}

  /// The drag this object took ended.
  @protected
  void onDragEnd(Finger finger) {}

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
      _world!._add(child, this);
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

  /// Removes the object and everything below it. Each gets its onDisable
  /// and onDestroy calls first, while it can still be read; using any of
  /// them after that throws a [StateError] saying it was destroyed
  /// ([isDestroyed] tells). Destroying an object again does nothing.
  void destroy() {
    if (_destroyed) return;
    final world = _world;
    if (world == null) {
      _queuedIn?._described?.children.remove(this);
      _queuedIn = null;
      _destroyUnadded();
      return;
    }
    if (world.isDisposed || !fy_exists(world._pointer, _id)) {
      _destroyed = true;
      _gone = true;
      return;
    }
    world._destroy(_id);
  }

  /// Destroys an object that was never added, and what was added to it:
  /// what they own and the assets they hold go too.
  void _destroyUnadded() {
    _destroyed = true;
    _gone = true;
    _stopListening();
    for (final c in _components ?? const <Component>[]) {
      c._removed = true;
    }
    _failLoaded(StateError('$this was destroyed before it was added'), StackTrace.current);
    final described = _described;
    for (final child in described?.children ?? const <GameObject>[]) {
      child._destroyUnadded();
    }
    if (_owned case final owned?) {
      for (final thing in owned) {
        // No world to report to: a resource used elsewhere throws here.
        switch (thing) {
          case Material m:
            m.dispose();
          case Mesh m:
            m.dispose();
          case Texture2D t:
            t.dispose();
          case Model m:
            m.dispose();
        }
      }
    }
    if (described?.asset case final asset?) {
      asset.world._release(asset, this);
      described!.asset = null;
    }
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
    check(fy_parent_get(_engine('its parent can\'t be read'), id, scratch.id));
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
    check(fy_parent_set(_engine('its parent can\'t be set'), id, value?.id ?? 0));
    world._moved.add(this);
    world._shape++;
    world._refresh(this);
  }

  /// The objects whose [parent] is this one, in the order they were added.
  /// A new list each time.
  List<GameObject> get children {
    _engine('its children can\'t be read');
    return List.unmodifiable(world._childIds(id).map(world._object));
  }

  /// Whether this object is [other] or below it.
  bool isPartOf(GameObject other) {
    if (other._world == null || !identical(other._world, world)) return false;
    check(fy_is_part_of(_engine('isPartOf can\'t tell'), id, other.id, scratch.flag));
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
    final w = _engine('active can\'t be read');
    if (_hiddenForLoad) return _activeSelf;
    check(fy_active_get(w, id, scratch.flag));
    return scratch.flag.value;
  }

  set active(bool value) {
    if (_described case final d?) {
      d.active = value;
      return;
    }
    final w = _engine('active can\'t be set');
    _activeSelf = value;
    // While it loads it stays hidden; it takes this flag when it appears.
    if (_hiddenForLoad) return;
    check(fy_active_set(w, id, value));
    if (value) {
      world._off.remove(this);
    } else {
      world._off.add(this);
    }
    world._shape++;
    world._refresh(this);
  }

  /// The object's layer, 0 to 31 (Unity's layers): a camera draws only the
  /// layers in its [Camera.cullingMask], and a ray ([World.raycast]) or a
  /// finger ([Input.layers]) hits only those in its mask. 0 unless set; a
  /// child doesn't take its parent's.
  int get layer => _layer;

  set layer(int value) {
    _checkLayer(value);
    if (_described == null) check(fy_layer_set(_engine('its layer can\'t be set'), id, value));
    _layer = value;
  }

  /// Whether this object is shown: it and every object above it [active],
  /// and loaded (see [GameObject.onLoad]).
  bool get activeInHierarchy {
    check(fy_active_in_hierarchy_get(_engine('activeInHierarchy can\'t be read'), id, scratch.flag));
    return scratch.flag.value;
  }

  /// The object's name, or null. Names needn't be unique; setting '' is
  /// the same as null.
  String? get name {
    if (_described case final d?) return d.name;
    final w = _engine('its name can\'t be read');
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
    check(fy_name_set(_engine('its name can\'t be set'), id, scratch.bytes(), length));
  }

  /// The first object below this one named [name], searching depth first
  /// in [children] order, or null. This object itself isn't searched.
  GameObject? find(String name) {
    final w = _engine('nothing can be found below it');
    final length = scratch.utf8(name);
    check(fy_find(w, id, scratch.bytes(), length, scratch.id));
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
    final w = _engine('its worldTransform can\'t be read');
    world._sendTransforms();
    check(fy_world_transform_get(w, id, scratch.transform));
    final t = scratch.transform.ref;
    return Transform._(t.position.read(), t.rotation.read(), t.scale.read());
  }

  /// The ray from this camera through [point] in a view showing it: from
  /// the point on the near plane, away from the camera. Cast it with
  /// [World.raycast] to find what's drawn there (Unity's
  /// `ScreenPointToRay`).
  ///
  /// Uses the object's [Camera] and its [worldTransform] (its scale is
  /// ignored, as the view ignores it). Throws a [FynkelException] with
  /// [FynkelStatus.noComponent] when it has no camera.
  Ray screenToRay(ViewPoint point) {
    if (!(point.width > 0 && point.height > 0)) {
      throw ArgumentError.value(point, 'point', 'needs a view with an area');
    }
    final c = _lens();
    final t = worldTransform;
    // Across the view from -1 to 1, up positive, so the view's y flips.
    final across = 2 * point.x / point.width - 1;
    final up = 1 - 2 * point.y / point.height;
    final aspect = point.width / point.height;
    if (c.orthographic) {
      // Straight ahead, from the point on the near plane.
      final from = Vec3(across * c.size * aspect, up * c.size, -c.near);
      return Ray(t.position + t.rotation.rotate(from), t.rotation.rotate(Vec3.forward));
    }
    final tanHalf = math.tan(c.fovY * math.pi / 360);
    // Through the point on the plane 1 in front of the camera.
    final through = t.rotation.rotate(Vec3(across * tanHalf * aspect, up * tanHalf, -1));
    return Ray(t.position + through * c.near, through);
  }

  /// Where the ray through [point] meets [plane]: the spot on the ground
  /// under a finger, say. Null when the ray runs along the plane or away
  /// from it.
  Vec3? screenToPlane(ViewPoint point, Plane plane) {
    final ray = screenToRay(point);
    final distance = plane.raycast(ray);
    return distance == null ? null : ray.at(distance);
  }

  /// Where [point], in the world, is in a view showing this camera
  /// (Unity's `WorldToScreenPoint`), in the view's logical pixels from its
  /// top left: in [width] × [height], by default the size [World.input]
  /// was last given. Null when it's behind a perspective camera; it can be
  /// outside the view.
  ViewPoint? worldToScreen(Vec3 point, {double? width, double? height}) {
    final w = width ?? world.input.width, h = height ?? world.input.height;
    if (!(w > 0 && h > 0)) throw ArgumentError('worldToScreen needs a view with an area; got ${w}x$h');
    final c = _lens();
    final t = worldTransform;
    final p = t.rotation.inverse.rotate(point - t.position);
    final aspect = w / h;
    double across, up;
    if (c.orthographic) {
      across = p.x / (c.size * aspect);
      up = p.y / c.size;
    } else {
      final ahead = -p.z;
      if (!(ahead > 0)) return null;
      final tanHalf = math.tan(c.fovY * math.pi / 360);
      across = p.x / (ahead * tanHalf * aspect);
      up = p.y / (ahead * tanHalf);
    }
    return ViewPoint((across + 1) / 2 * w, (1 - up) / 2 * h, width: w, height: h);
  }

  Camera _lens() => get<Camera>() ?? (throw FynkelException(FynkelStatus.noComponent, '$this has no Camera'));

  // --- Components ---

  /// Puts [component] on this object and returns it: a [Behaviour] (any
  /// number), or one of the engine's (at most one [Camera], [Light] and
  /// [MeshRenderer] each). On a live object an engine component is written
  /// at once and a behaviour loads ([Behaviour.onLoad]) and starts the next
  /// frame.
  ///
  /// Throws a [StateError] if [component] is on an object already (each
  /// belongs to one, for its life) or this object was destroyed, an
  /// [ArgumentError] for a second engine component of a kind, and the
  /// engine's error if it refuses the component's values.
  T addComponent<T extends Component>(T component) {
    if (component._owner case final owner?) {
      throw StateError('$component is on $owner already; a component belongs to one object');
    }
    if (_destroyed) throw StateError('$this was destroyed, so nothing can be put on it');
    if (component is! Behaviour) {
      _probe();
      for (final c in _components ?? const <Component>[]) {
        if (!c._removed && c.runtimeType == component.runtimeType) {
          throw ArgumentError('$this has a ${c.runtimeType} already; an object has at most one');
        }
      }
    }
    component._owner = this;
    (_components ??= []).add(component);
    if (_world case final world?) {
      try {
        _write(component);
      } catch (_) {
        _components!.removeLast();
        component._owner = null;
        rethrow;
      }
      world._componentAdded(this, component);
    }
    return component;
  }

  /// Takes [component] off this object. A behaviour gets
  /// [Behaviour.onDisable] if it was enabled, then [Behaviour.onDestroy],
  /// and no calls after; an engine component
  /// is taken out of the engine. It can't be put on again. Throws an
  /// [ArgumentError] if it isn't on this object.
  void removeComponent(Component component) {
    if (!identical(component._owner, this) || component._removed) {
      throw ArgumentError.value(component, 'component', 'is not on $this');
    }
    final world = _world;
    if (world == null) {
      component._removed = true;
      _components!.remove(component);
      if (component is Behaviour) component._listening?.cancelAll();
      return;
    }
    switch (component) {
      case Behaviour b:
        if (b._enabledNow) {
          b._enabledNow = false;
          world._safely(this, b.onDisable);
        }
        world._stopRoutines(b._routines);
        world._haltMotions(b._motions);
        b._listening?.cancelAll();
        world._safely(this, b.onDestroy);
      case Camera():
        check(fy_camera_remove(world.pointer, id));
      case Light():
        check(fy_light_remove(world.pointer, id));
      case MeshRenderer():
        check(fy_mesh_renderer_remove(world.pointer, id));
    }
    component._removed = true;
    world._unregister(component);
    world._compact.add(this);
  }

  /// The first component on this object of type [T] (a class, a subclass,
  /// or an interface one implements), or null: `get<Health>()`,
  /// `get<Camera>()`. Unity's `GetComponent`.
  T? get<T extends Object>() {
    _probe();
    for (final c in _components ?? const <Component>[]) {
      if (c is T && !c._removed) return c as T;
    }
    return null;
  }

  /// Every component on this object of type [T], in the order they were
  /// put on. A new list each time.
  List<T> getAll<T extends Object>() {
    _probe();
    return [
      for (final c in _components ?? const <Component>[])
        if (c is T && !c._removed) c as T,
    ];
  }

  /// Whether this object has a component of type [T].
  bool has<T extends Object>() => get<T>() != null;

  /// The first component of type [T] on this object, or else on the
  /// nearest object above it that has one, or null.
  T? getInParent<T extends Object>() {
    for (GameObject? o = this; o != null; o = o.parent) {
      if (o.get<T>() case final found?) return found;
    }
    return null;
  }

  /// The first component of type [T] on this object, or else below it,
  /// depth first in [children] order, or null.
  T? getInChildren<T extends Object>() {
    if (get<T>() case final found?) return found;
    for (final child in children) {
      if (child.getInChildren<T>() case final found?) return found;
    }
    return null;
  }

  /// Reads the engine components of an object the engine made, the first
  /// time they're asked for, so the same instances come back after.
  void _probe() {
    if (_probed) return;
    _probed = true;
    final world = _world;
    if (world == null || _destroyed || world.isDisposed) return;
    final w = world._pointer;
    bool missing<T>() => !(_components ?? const <Component>[]).any((c) => c is T && !c._removed);
    final found = <Component>[
      if (missing<Camera>() && fy_camera_get(w, _id, scratch.camera) == FyStatus.FY_STATUS_OK)
        Camera(
          fovY: scratch.camera.ref.fov_y,
          near: scratch.camera.ref.near_clip,
          far: scratch.camera.ref.far_clip,
          clearColor: scratch.camera.ref.clear_color.read(),
          orthographic: scratch.camera.ref.orthographic,
          size: scratch.camera.ref.size,
          cullingMask: LayerMask._(scratch.camera.ref.culling_mask),
        ),
      if (missing<Light>() && fy_light_get(w, _id, scratch.light) == FyStatus.FY_STATUS_OK) _readLight(),
      if (missing<MeshRenderer>() && fy_mesh_renderer_get(w, _id, scratch.meshRenderer) == FyStatus.FY_STATUS_OK)
        MeshRenderer(world._meshes[scratch.meshRenderer.ref.mesh], world._materials[scratch.meshRenderer.ref.material]),
    ];
    for (final c in found) {
      c._owner = this;
    }
    if (found.isNotEmpty) _components = [...found, ...?_components];
  }

  Light _readLight() {
    final l = scratch.light.ref;
    return switch (l.kind) {
      FY_LIGHT_DIRECTIONAL => Light.sun(color: l.color.read(), intensity: l.intensity),
      FY_LIGHT_AMBIENT => Light.ambient(color: l.color.read(), intensity: l.intensity),
      final kind => throw StateError('the engine returned light kind $kind, which this package:fynkel predates'),
    };
  }

  void _write(Component component) => switch (component) {
    Camera c => _writeCamera(c),
    Light c => _writeLight(c),
    MeshRenderer c => _writeMeshRenderer(c),
    Behaviour() => null,
  };

  /// Where the object is, how it's turned and how big it is, relative to
  /// its [parent]. Reading it makes no call into the engine. Setting it
  /// checks the value at once, as the engine would, and sends it with the
  /// frame's other moves in one call when something next reads the
  /// engine's transforms (a view's frame, [World.raycast],
  /// [worldTransform]). It reads back exactly as it was set; the engine
  /// keeps it in 32-bit floats, so what it draws may differ in the last
  /// digits.
  Transform get transform {
    if (_described case final d?) return d.transform;
    final w = _engine('its transform can\'t be read');
    if (_transform case final t?) return t;
    check(fy_transform_get(w, id, scratch.transform));
    final t = scratch.transform.ref;
    return _transform = Transform._(t.position.read(), t.rotation.read(), t.scale.read());
  }

  set transform(Transform value) {
    if (_described case final d?) {
      d.transform = value;
      return;
    }
    final w = _engine('its transform can\'t be set');
    if (_transform == null && !fy_exists(w, _id)) {
      throw FynkelException(FynkelStatus.noObject, 'object#$_id does not exist (despawned, or never spawned)');
    }
    _check(value);
    _setTransform(value);
  }

  void _setTransform(Transform value) {
    _transform = value;
    if (_unsent) return;
    _unsent = true;
    world._unsent.add(this);
  }

  /// Throws a [FynkelException] with the engine's message when the engine
  /// would refuse [value]: checked here, so the error comes from the line
  /// that set it, not from the frame that sends it.
  static void _check(Transform value) {
    // In the engine's order: each field's finiteness as 32-bit floats,
    // and the rotation's length after its finiteness.
    final Transform(position: p, rotation: r, scale: s) = value;
    if (!(_fits(p.x) && _fits(p.y) && _fits(p.z))) _refuse('position: NaN or infinity');
    if (!(_fits(r.x) && _fits(r.y) && _fits(r.z) && _fits(r.w))) _refuse('rotation: NaN or infinity');
    if ((math.sqrt(r.x * r.x + r.y * r.y + r.z * r.z + r.w * r.w) - 1).abs() > 1e-3) {
      _refuse('rotation: rotation is not a unit quaternion');
    }
    if (!(_fits(s.x) && _fits(s.y) && _fits(s.z))) _refuse('scale: NaN or infinity');
  }

  static final _f32 = Float32List(1);

  /// Whether [x] is finite as a 32-bit float: the fast test first.
  static bool _fits(double x) => x.abs() < 3.4e38 || (_f32..[0] = x)[0].isFinite;

  static Never _refuse(String what) => throw FynkelException(FynkelStatus.invalidValue, 'Transform.$what');

  /// Turns the object [angle] radians around [axis], in its own frame.
  void rotate(Vec3 axis, double angle) => transform = transform.rotated(axis, angle);

  /// Moves the object by [offset], in its parent's space (the world's for
  /// a root).
  void translate(Vec3 offset) => transform = transform.translated(offset);

  void _writeCamera(Camera value) {
    final c = scratch.camera.ref;
    c.fov_y = value.fovY;
    c.near_clip = value.near;
    c.far_clip = value.far;
    c.clear_color.write(value.clearColor);
    c.orthographic = value.orthographic;
    c.size = value.size;
    c.culling_mask = value.cullingMask.value;
    check(fy_camera_set(_engine('its Camera can\'t be changed'), id, scratch.camera));
  }

  void _writeLight(Light value) {
    final l = scratch.light.ref;
    l.kind = switch (value.kind) {
      LightKind.sun => FY_LIGHT_DIRECTIONAL,
      LightKind.ambient => FY_LIGHT_AMBIENT,
    };
    l.color.write(value.color);
    l.intensity = value.intensity;
    check(fy_light_set(_engine('its Light can\'t be changed'), id, scratch.light));
  }

  void _writeMeshRenderer(MeshRenderer value) {
    _engine('its MeshRenderer can\'t be changed');
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
  _Description(this.name, this.transform);

  String? name;
  bool active = true;
  Transform transform;
  final children = <GameObject>[];

  /// For a model's copy ([Model.instantiate]): the engine makes its objects.
  Model? model;

  /// For a model's copy from [Assets.model]: the use it holds of the
  /// model, until it's destroyed.
  _Asset? asset;
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

/// For package:fynkel's views: sends the transforms the game set since the
/// last send, before a frame copies the world. Not exported.
extension SendTransforms on World {
  void sendTransforms() {
    if (!isDisposed) _sendTransforms();
  }
}

/// An error no caller could catch, from an object's load or from freeing
/// what it held: see [World.errors].
final class ObjectError {
  ObjectError._(this.object, this.error, this.stackTrace);

  /// The object whose load failed (now destroyed), or that was being
  /// destroyed; null for an asset the world freed.
  final GameObject? object;
  final Object error;
  final StackTrace stackTrace;

  @override
  String toString() => '${object ?? 'World'}: $error';
}
