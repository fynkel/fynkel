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
part 'game_object.dart';
part 'input.dart';
part 'keys.dart';
part 'layers.dart';
part 'model.dart';
part 'motion.dart';
part 'pointing.dart';
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
    // The assets still held: the engine's side goes with the world, the
    // rest goes after it.
    final owned = [for (final asset in _assets.values) ?asset.value];
    // Nothing of a disposed world hears a signal again, or holds one.
    for (final object in _objects.values) {
      object._stopListening();
    }
    for (final wait in [..._waits, ..._fixedWaits]) {
      wait.over = true;
    }
    final systems = [..._systems];
    _systems.clear();
    _systemsByType.clear();
    for (final system in systems) {
      try {
        system.dispose();
      } catch (error, stack) {
        _report(null, error, stack);
      }
    }
    _finalizer.detach(this);
    _debug?._release();
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

  // Goes up whenever an object's transform, parent or layer is set: for
  // systems that follow objects (systems.dart).
  var _transformVersion = 0;
  // readWorldTransforms' last buffer, as the engine's type.
  Pointer<FyTransform> _manyOut = nullptr;

  // Whether a destroyed object is in [_unsent], to be dropped at the next
  // update: with no view drawing, nothing else would let go of it.
  var _unsentGone = false;

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
  /// 3. **Fixed steps.** As many as the frame's time holds
  ///    ([Time.fixedDelta] each, maybe none). In each, the [WorldSystem]s'
  ///    `beforeFixedUpdate`, then each started object's
  ///    [GameObject.onFixedUpdate] and its behaviours', in the same order
  ///    as the updates, then the systems' `afterFixedUpdate` (where physics
  ///    steps), then the routines waiting for [fixedStep].
  /// 4. **Updates.** Each started object gets [GameObject.onUpdate],
  ///    then its started behaviours get [Behaviour.onUpdate] in the order
  ///    they were added, parents before children. Objects and behaviours
  ///    added during an update start next frame; objects destroyed during
  ///    it get no more calls, even this frame. A new [GameObject.parent]
  ///    counts from the next frame.
  /// 5. **Motions and routines.** Each running [Motion] moves on
  ///    ([GameObject.tween], [GameObject.moveTo]...), in the order they
  ///    were made, then the routines' waits end ([GameObject.start]).
  /// 6. **Late updates.** Then each object updated this frame, and its
  ///    behaviours, get [GameObject.onLateUpdate] in the same order: for
  ///    what follows where the others went. Then the systems'
  ///    `afterLateUpdate`.
  /// 7. **Cameras.** The camera rigs move their objects ([Follow] and
  ///    [Orbit], then [LookAt]), then [cameras] puts the view's camera
  ///    where the live [VirtualCamera] is, and shakes it.
  /// 8. **Debug lines.** What was drawn with [debug] goes to the engine,
  ///    to be drawn over the frame.
  ///
  /// Hidden objects (inactive, or below one that is) and their behaviours
  /// are skipped by the last six and start when they're first shown; so are
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

    _runFixedSteps();
    if (isDisposed) return;

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

    for (var i = 0; i < _systems.length; i++) {
      final system = _systems[i];
      try {
        system.afterLateUpdate();
      } catch (error, stack) {
        _report(null, error, stack);
      }
    }

    cameras._run();
    _debug?._flush();
  }

  // --- Motions ---

  // The running motions not in a group, in the order they were made.
  final _motions = <Motion>[];

  // --- Systems ---

  // The world's systems, in the order they were made (systems.dart).
  final _systems = <WorldSystem>[];
  final _systemsByType = <Type, WorldSystem>{};

  // --- Routines ---

  // What routines wait for, in the order they were made, and the
  // microtasks routines scheduled during [_runRoutines].
  final _waits = <_Wait>[];
  final _fixedWaits = <_Wait>[];
  final _microtasks = <void Function()>[];
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
