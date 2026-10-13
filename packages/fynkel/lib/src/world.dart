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
import 'frame_list.dart';
import 'math.dart';
import 'native.dart';

part 'assets.dart';
part 'cameras.dart';
part 'components.dart';
part 'debug.dart';
part 'game_object.dart';
part 'input.dart';
part 'keys.dart';
part 'layers.dart';
part 'model.dart';
part 'motion.dart';
part 'motion_kinds.dart';
part 'pointing.dart';
part 'pool.dart';
part 'resources.dart';
part 'routines.dart';
part 'signals.dart';
part 'systems.dart';
part 'time.dart';
part 'world_loop.dart';
part 'world_objects.dart';
part 'world_resources.dart';

// This file has the world's state and what the game calls; the work behind
// it is in world_loop.dart, world_objects.dart and world_resources.dart, and
// objects are in game_object.dart.

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

  // Meshes, materials and textures this world has, both ways, weakly.
  // They're added the first time something uses them, and stay until the
  // world is disposed, their `dispose` takes them out, or nothing reaches
  // them any more: then their [_Release] puts them in [_collected].
  var _meshIds = Expando<int>();
  final _meshes = <int, WeakReference<Mesh>>{};
  var _materialIds = Expando<int>();
  final _materials = <int, WeakReference<Material>>{};
  var _textureIds = Expando<int>();

  // Garbage-collected resources to take out, at the end of the next update.
  // One the engine still uses stays here, and is tried again when another
  // is collected.
  final _collected = <(_Resource, int)>[];
  var _collectedNew = false;

  Pointer<FyWorld> get _engineWorld {
    if (_pointer == nullptr) throw StateError('this World was disposed');
    return _pointer;
  }

  /// Whether [dispose] was called (and, when called from inside [update],
  /// the update has returned). Then adding to the world and every call
  /// that reaches the engine throw a StateError.
  bool get isDisposed => _pointer == nullptr;

  /// Destroys every object, as a scene unloads in Unity, then frees the
  /// world. Each object gets its `onDisable` and `onDestroy`, root by root
  /// in the order they were added, so a game's clean-up and saving run
  /// when its page is left or it starts over. Nothing can be used after
  /// this; a view of the world keeps drawing what it last had until the
  /// view is disposed too. Called from inside [update], it waits for the
  /// update to return.
  void dispose() {
    if (_pointer == nullptr || _disposing) return;
    if (_updating) {
      _disposeAsked = true;
      return;
    }
    _disposing = true;
    // What an onDestroy adds is destroyed too, a few rounds deep.
    for (var round = 0; round < 8 && _objects.isNotEmpty; round++) {
      final roots = [
        for (final object in _objects.values)
          if (!object._destroyed && object._parent == null) object,
      ];
      for (final root in roots) {
        root.destroy();
      }
    }
    // The assets still held: a package's are freed now, while its system
    // is there; the engine's side of the rest goes with the world, and
    // what's left after it.
    final owned = <Object>[];
    for (final asset in _assets.values) {
      final value = asset.value;
      if (value == null) continue;
      if (asset.kind is _BuiltInKind) {
        owned.add(value);
      } else {
        asset.value = null;
        _freeAsset(asset.kind, value, null);
      }
    }
    // Nothing of a disposed world hears a signal again, or holds one.
    for (final object in _objects.values) {
      object._stopListening();
    }
    for (final wait in [..._waits, ..._fixedWaits]) {
      wait.over = true;
    }
    final systems = _systems;
    _systems = [];
    _systemsByType.clear();
    _assetKinds.clear();
    for (final system in systems) {
      try {
        system.dispose();
      } catch (error, stack) {
        _report(null, error, stack);
      }
    }
    _finalizer.detach(this);
    _debug?._dispose();
    fy_world_free(_pointer);
    _pointer = nullptr;
    _meshIds = Expando();
    _meshes.clear();
    _materialIds = Expando();
    _materials.clear();
    _textureIds = Expando();
    _collected.clear();
    _objects.clear();
    _byClass.clear();
    _live.clear();
    _starting.clear();
    _compact.clear();
    _waits.clear();
    _fixedWaits.clear();
    _microtasks.clear();
    _laterMicrotasks.clear();
    _motions.clear();
    _reparented.clear();
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
  /// onLateUpdate, a pointer handler, onEnable, onDisable, onDestroy, a
  /// routine, a tween's callback or a package's callback, such as a
  /// physics trigger (the frame goes on, as in Unity: one object's bug
  /// doesn't stop the others). A behaviour added to an object that has
  /// already appeared loads by itself: when its [Behaviour.onLoad] fails,
  /// only the behaviour is removed. A [WorldSystem] that throws is
  /// reported too, and turned off after [WorldSystem.turnOffAfter] frames
  /// in a row of throwing. With no listener, each is an uncaught error of the current zone (for a load,
  /// the zone the object was added in), as an unawaited future's would
  /// be, so a test with no listener fails; listen to handle them, or
  /// rethrow to stop.
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

  // --- The loop ---

  // Every object the game added, in the order it became live, so parents
  // come before their children. Destroyed ones are dropped and moved ones
  // put last at the start of the next update.
  final _live = <GameObject>[];
  var _liveDirty = false;
  // The objects given a new parent this frame: they and what's below them
  // go last in [_live] at the start of the next update.
  final _reparented = <GameObject>[];

  // Live objects and behaviours that haven't had onStart yet, in the same
  // order, each object before its behaviours.
  // Those still waiting (hidden, not loaded) stay, moved up in place: a
  // list cleared and refilled each frame would drop its storage and grow
  // it again, which allocates while a pool's hidden objects wait.
  final _starting = <Object>[];

  // Objects with removed components still in their lists: taken out at the
  // start of the next update, so a removal never shifts a list being run.
  final _compact = <GameObject>[];

  // Goes up whenever an object is added, destroyed, shown, hidden or
  // given a new parent (systems.dart).
  var _shape = 0;

  // Objects whose transform the game set since it was last sent, in the
  // order they were first set. [_sendTransforms] sends them all in one
  // call before anything reads the engine's transforms: a view's frame, a
  // raycast, a world transform, a tap.
  final _unsent = FrameList<GameObject>();

  // Goes up whenever an object's transform, parent or layer is set: for
  // systems that follow objects (systems.dart).
  var _transformVersion = 0;

  // The moved log (systems.dart): the objects whose transform, parent or
  // layer was set, kept only while a [MovedReader] reads it. [_movedBase]
  // is the place of its first entry, counted since the world was made;
  // [_movedReadTo] the furthest any reader has read.
  final _movedLog = FrameList<GameObject>();
  var _movedBase = 0;
  var _movedReadTo = 0;
  final _movedReaders = <MovedReader>[];
  // readWorldTransforms' last buffer, as the engine's type.
  Pointer<FyTransform> _manyOut = nullptr;

  // Whether the world has said that it draws only one sun ([_noteSun]).
  var _warnedSuns = false;

  // Whether a destroyed object is in [_unsent], to be dropped at the next
  // update: with no view drawing, nothing else would let go of it.
  var _unsentGone = false;

  /// What the player does: fingers, gestures, keys and named controls.
  /// The view feeds it; [update] runs its step first. Objects read it as
  /// [GameObject.input].
  late final Input input = Input().._targets = _Pointing(this);

  /// Whether the game is paused, as for a pause menu: the game's time
  /// stands still ([Time.delta] is 0, no fixed steps run, motions and
  /// waits hold), and [input] takes nothing: what's held is let go when
  /// the pause starts, and taps and keys meanwhile are dropped. Objects
  /// still get their updates, and [Time.unscaledDelta] still runs, for
  /// what moves while paused. [Time.scale] is left as it was, and the first
  /// frame after the pause counts no time, however long it was.
  ///
  /// Unlike `time.scale = 0`, which stops only the time. Each change is
  /// told by [onPausedChanged].
  bool get paused => _paused;
  set paused(bool value) {
    if (value == _paused) return;
    _paused = value;
    input._paused = value;
    input._pausing = value;
    time._paused = value;
    if (!value) time._skip = true;
    onPausedChanged.emit(value);
  }

  var _paused = false;

  /// Emits [paused]'s new value each time it changes, as it changes: for
  /// what shows the pause, such as a pause panel (in Flutter,
  /// `world.pausedListenable` from `fynkel_flutter`).
  final onPausedChanged = Signal<bool>();

  /// Whether the app is away: in the background, or hidden. [FynkelView]
  /// sets it from the app's lifecycle; a test can set it. When it changes,
  /// the [WorldSystem]s' `applicationPause`, then every shown object's
  /// [GameObject.onApplicationPause] and its enabled behaviours',
  /// [Behaviour.onApplicationPause], as Unity's `OnApplicationPause`: to
  /// save, or stop the sound. Unlike [paused], it stops nothing.
  bool get applicationPaused => _applicationPaused;
  set applicationPaused(bool value) {
    if (value == _applicationPaused || isDisposed) return;
    _applicationPaused = value;
    final systems = _systems;
    for (var i = 0; i < systems.length; i++) {
      final system = systems[i];
      if (system._off) continue;
      try {
        system.applicationPause(value);
      } catch (error, stack) {
        _systemThrew(system, error, stack);
      }
    }
    final count = _live.length;
    for (var i = 0; i < count && i < _live.length; i++) {
      final object = _live[i];
      if (object._destroyed || _isHidden(object)) continue;
      _safely(object, () => object.onApplicationPause(value));
      final components = object._components;
      if (components == null) continue;
      for (var j = 0; j < components.length && !object._destroyed; j++) {
        if (components[j] case final Behaviour b when b._enabledNow && !b._removed) {
          _safely(object, () => b.onApplicationPause(value));
        }
      }
    }
  }

  var _applicationPaused = false;

  /// The virtual cameras, their blends, and shaking the view's camera
  /// (docs/stage-4.md §3.8).
  late final Cameras cameras = Cameras._(this);

  /// Lines drawn over the scene, to see what the code sees
  /// (docs/stage-4.md §3.9). Objects and behaviours reach it as `debug`.
  Debug get debug => _debug ??= Debug._(this);
  Debug? _debug;

  /// The camera the view draws from (Unity's `Camera.main`): of the
  /// shown objects ([GameObject.activeInHierarchy]) with a [Camera], the
  /// one with the lowest id; null when there's none. A hidden camera is
  /// skipped. Its object is `camera.gameObject`.
  Camera? get camera => _cameraObject?.get<Camera>();

  GameObject? get _cameraObject {
    final w = pointer;
    _sendTransforms();
    check(fy_camera_active(w, scratch.id));
    final id = scratch.id.value;
    return id == 0 ? null : _object(id);
  }

  /// Runs one frame of the game, [dt] seconds after the last one: [time]
  /// moves on (see [Time.delta]), the [WorldSystem]s' `beforeFrame` run
  /// (where physics collects the step it ran between frames), then, in
  /// this order:
  ///
  /// 1. **Input.** [input]'s events since the last frame, in order, then
  ///    its gestures ([Input]): its signals, and the objects' pointer
  ///    handlers ([GameObject.onPointerDown] and the rest), cast into the
  ///    world from the camera the view draws from.
  /// 2. **Starts.** Each object added since the last frame gets
  ///    [GameObject.onStart], once, parents before children, each right
  ///    before its behaviours' [Behaviour.onStart]. Objects added during a
  ///    start start too.
  /// 3. **Fixed steps.** As many as the frame's time holds
  ///    ([Time.fixedDelta] each, maybe none). In each, the [WorldSystem]s'
  ///    `beforeFixedUpdate`, then each started object's
  ///    [GameObject.onFixedUpdate] and its behaviours', in the same order
  ///    as the updates, then the systems' `afterFixedUpdate` (where physics
  ///    steps, or leaves the step for between frames), then the routines
  ///    waiting for [fixedStep]. Then, with steps or none, the systems'
  ///    `beforeUpdate`.
  /// 4. **Updates.** Each started object gets [GameObject.onUpdate],
  ///    then its started behaviours get [Behaviour.onUpdate] in the order
  ///    they were added, parents before children. Objects and behaviours
  ///    added during an update start next frame; objects destroyed during
  ///    it get no more calls, even this frame. A new [GameObject.parent]
  ///    counts from the next frame.
  /// 5. **Motions and routines.** Each running [Motion] moves on
  ///    ([GameObject.tween], [GameObject.moveTo]...), in the order they
  ///    were made, then the routines' waits end ([GameObject.start]).
  ///    Then the systems' `afterUpdate` (where an animator poses).
  /// 6. **Late updates.** Then each object updated this frame, and its
  ///    behaviours, get [GameObject.onLateUpdate] in the same order: for
  ///    what follows where the others went. Then the systems'
  ///    `afterLateUpdate`.
  /// 7. **Cameras.** The camera rigs move their objects ([Follow] and
  ///    [Orbit], then [LookAt]), then [cameras] puts the view's camera
  ///    where the live [VirtualCamera] is, and shakes it.
  /// 8. **Debug lines.** What was drawn with [debug] goes to the engine,
  ///    to be drawn over the frame. Last, the systems' `afterFrame`.
  ///
  /// Hidden objects (inactive, or below one that is) and their behaviours
  /// are skipped by the last six and start when they're first shown; so are
  /// disabled behaviours ([Behaviour.enabled]) and those still loading. A
  /// view then draws the world as it is. What their code throws goes to
  /// [errors], and the frame goes on.
  ///
  /// [dispose] called from inside an update (by the game's code) waits
  /// for the update: the phases left are skipped, and the world is freed as
  /// it returns.
  ///
  /// Throws an [ArgumentError] if [dt] is negative or NaN, and a
  /// [StateError] if called from inside an update: a frame runs one.
  void update(double dt) {
    pointer;
    if (_updating) {
      throw StateError('World.update was called from inside an update; each frame runs one, from its view or test');
    }
    _updating = true;
    try {
      _update(dt);
    } finally {
      _updating = false;
      if (_disposeAsked) dispose();
    }
  }

  // Whether an update is running, and whether dispose was called in it.
  var _updating = false;
  var _disposeAsked = false;

  // Whether dispose is destroying the objects, for one called again from
  // an onDestroy.
  var _disposing = false;

  void _update(double dt) {
    time._advance(dt);
    _tidy();
    _runSystems(_SystemHook.beforeFrame);
    if (_disposeAsked) return;

    input
      .._now = time._unscaledSinceStart
      .._step();

    var kept = 0;
    for (var i = 0; i < _starting.length; i++) {
      switch (_starting[i]) {
        case final GameObject object:
          if (object._destroyed) continue;
          if (_isHidden(object)) {
            _starting[kept++] = object;
            continue;
          }
          object._started = true;
          _safely(object, object.onStart);
        case final Behaviour behaviour:
          final object = behaviour._owner!;
          if (behaviour._removed || object._destroyed) continue;
          if (!behaviour._loaded || !behaviour._enabledFlag || !object._started || _isHidden(object)) {
            _starting[kept++] = behaviour;
            continue;
          }
          behaviour._started = true;
          _safely(object, behaviour.onStart);
      }
    }
    // Shorter unless an onStart disposed the world, which empties it.
    if (kept < _starting.length) _starting.length = kept;

    if (_disposeAsked) return;
    _runFixedSteps();
    if (_disposeAsked) return;
    _runSystems(_SystemHook.beforeUpdate);
    if (_disposeAsked) return;

    // Objects added from here on are past the end when they come, or not
    // started: they wait for the next frame. The calls are guarded inline,
    // not through [_safely], so a frame makes no closures.
    final updated = _live.length;
    _callEach(_Call.update, updated);

    if (_disposeAsked) return;
    _runMotions();
    _runRoutines();
    if (_disposeAsked) return;
    _runSystems(_SystemHook.afterUpdate);
    if (_disposeAsked) return;

    _callEach(_Call.lateUpdate, updated);

    if (_disposeAsked) return;
    _runSystems(_SystemHook.afterLateUpdate);
    if (_disposeAsked) return;

    cameras._run();
    _debug?._flush();
    _runSystems(_SystemHook.afterFrame);
  }

  // --- Motions ---

  // The running motions not in a group, in the order they were made.
  final _motions = <Motion>[];

  // --- Systems ---

  // The world's systems, in the order their hooks run (systems.dart): a
  // new list each time one is made. The asset kinds packages added.
  var _systems = <WorldSystem>[];
  final _systemsByType = <Type, WorldSystem>{};
  final _assetKinds = <AssetKind<Object>>[];

  // --- Routines ---

  // What routines wait for, in the order they were made; the microtasks
  // routines scheduled during [_runRoutines], and those they scheduled
  // outside it, for the next one (not a fixed step's).
  final _waits = <_Wait>[];
  final _fixedWaits = <_Wait>[];
  final _microtasks = <void Function()>[];
  final _laterMicrotasks = <void Function()>[];
  var _inRoutineStep = false;

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
  ///
  /// As Unity's `FindObjectsByType`, objects turned off ([GameObject.active]
  /// false, theirs or a parent's), and the components on them, are left
  /// out unless [includeInactive]: a pool's waiting objects aren't found. A
  /// disabled behaviour on an active object is found.
  List<T> all<T extends Object>({bool includeInactive = false}) => [
    for (final objects in _byClass.values)
      if (objects.first is T)
        for (final o in objects)
          if (includeInactive || _isActive(o)) o as T,
  ];

  /// The one live object or component of type [T] that the game added:
  /// `world.only<Score>()`. Throws a [StateError] if there's none or more
  /// than one, so a game's singletons need no `static instance`. See
  /// [all], for [includeInactive] too.
  T only<T extends Object>({bool includeInactive = false}) {
    T? found;
    var count = 0;
    for (final objects in _byClass.values) {
      if (objects.first is! T) continue;
      for (final o in objects) {
        if (!includeInactive && !_isActive(o)) continue;
        found ??= o as T;
        count++;
      }
    }
    if (count == 1) return found!;
    throw StateError(
      count == 0
          ? 'there is no $T in this world; only<$T>() needs exactly one'
          : 'there are $count of $T in this world; only<$T>() needs exactly one (all<$T>() gives them all)',
    );
  }

  /// The first live object or component of type [T] that the game added,
  /// or null. See [all], for [includeInactive] too.
  T? first<T extends Object>({bool includeInactive = false}) {
    for (final objects in _byClass.values) {
      if (objects.first is! T) continue;
      for (final o in objects) {
        if (includeInactive || _isActive(o)) return o as T;
      }
    }
    return null;
  }

  // Whether [o], an object or a component the game added, is on an object
  // that's active, and every one above it too.
  bool _isActive(Object o) {
    for (GameObject? x = o is Component ? o._owner : o as GameObject; x != null; x = x._parent) {
      if (!x._activeSelf) return false;
    }
    return true;
  }

  // The Dart object for each live engine id that has been asked for, so
  // the same instance always comes back: the game's own, or a plain one
  // made the first time a model's part is asked for.
  final _objects = <int, GameObject>{};

  // The objects and components the game added, by class, for [all] and
  // [first].
  final _byClass = <Type, Set<Object>>{};

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
    return RayHit(_object(hit.object), hit.point.read(), hit.normal.read(), hit.distance);
  }
}

/// Where a ray hit first ([World.raycast]).
final class RayHit {
  /// A hit a package makes, for [WorldSystem.pointerHit].
  RayHit(this.gameObject, this.point, this.normal, this.distance);

  /// The object hit.
  final GameObject gameObject;

  /// In the world.
  final Vec3 point;

  /// The face's normal in the world, unit length, on the side the ray came
  /// from.
  final Vec3 normal;

  /// From the ray's origin to [point].
  final double distance;

  @override
  String toString() => 'RayHit($gameObject at $point, normal $normal, distance $distance)';
}

/// For package:fynkel's views: sends the transforms the game set since the
/// last send, before a frame copies the world. Not exported.
extension SendTransforms on World {
  /// Sends them; does nothing once the world is disposed.
  void sendTransforms() {
    if (!isDisposed) _sendTransforms();
  }
}

/// An error no caller could catch: anything [World.errors] carries (see
/// its doc), from an object's load, its own code each frame, or freeing
/// what it held.
final class ObjectError {
  ObjectError._(this.gameObject, this.error, this.stackTrace);

  /// The object whose code threw, whose load failed (now destroyed), or
  /// that was being destroyed; null for the world's own (a system, a
  /// freed asset).
  final GameObject? gameObject;

  /// What was thrown.
  final Object error;

  /// Where it was thrown.
  final StackTrace stackTrace;

  @override
  String toString() => '${gameObject ?? 'World'}: $error';
}

/// The engine's world, for package:fynkel's own files; not exported.
extension WorldPointer on World {
  /// The engine's world. Throws a StateError once the world is disposed.
  Pointer<FyWorld> get pointer => _engineWorld;
}
