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

part 'animator.dart';
part 'assets.dart';
part 'cameras.dart';
part 'components.dart';
part 'debug.dart';
part 'game_object.dart';
part 'input.dart';
part 'keys.dart';
part 'layers.dart';
part 'mesh_building.dart';
part 'model.dart';
part 'motion.dart';
part 'motion_kinds.dart';
part 'particles.dart';
part 'pointing.dart';
part 'pool.dart';
part 'profile.dart';
part 'resources.dart';
part 'routines.dart';
part 'shader.dart';
part 'signals.dart';
part 'sky.dart';
part 'spline_objects.dart';
part 'splines.dart';
part 'sprites.dart';
part 'systems.dart';
part 'text.dart';
part 'time.dart';
part 'trails.dart';
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
  var _shaderIds = Expando<int>();
  // Fonts kept for [TextRenderer]s, by their id here: kept till the world
  // is disposed, as a game has few.
  var _fontIds = Expando<int>();
  final _shaders = <int, WeakReference<Shader>>{};

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
    _shaderIds = Expando();
    _fontIds = Expando();
    _shaders.clear();
    _collected.clear();
    _objects.clear();
    _byClass.clear();
    _live.clear();
    _joining.clear();
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
    // What a disposed world reports from here goes to the zone.
    _errors.close();
  }

  /// Completes once every shader the world's views have drawn with is
  /// ready: for a loading screen to wait on before it shows the game.
  ///
  /// A view on screen doesn't hold a frame up for a shader the GPU's
  /// driver hasn't compiled before (a third of a second, or more on a
  /// phone, the first time each look is drawn on a device): it draws the
  /// frame without those objects while the shader is compiled beside it.
  /// Build the level behind the loading screen, let the view draw it, and
  /// `await world.shadersReady()`; then nothing pops in. Checks once a
  /// frame, after at least two; completes at [timeout] whatever's left.
  Future<void> shadersReady({Duration timeout = const Duration(seconds: 10)}) async {
    ensureEngine();
    final from = time.frame;
    final watch = Stopwatch()..start();
    final out = malloc<Uint32>();
    try {
      while (!isDisposed && watch.elapsed < timeout) {
        if (time.frame >= from + 2) {
          check(fy_shaders_waiting(out));
          if (out.value == 0) return;
        }
        await Future<void>.delayed(const Duration(milliseconds: 16));
      }
    } finally {
      malloc.free(out);
    }
  }

  /// Reads every shader [assets] loaded again from the world's source and
  /// puts its new code in ([Shader.reload]): `FynkelGame` calls it on a hot
  /// reload, so a shader's file can be changed while the game runs. One
  /// that doesn't compile now keeps its old code, and its [ShaderError]
  /// goes to [errors].
  Future<void> reloadShaders() async {
    final source = _source;
    if (source == null) return;
    for (final asset in [..._assets.values]) {
      if (asset.value case final Shader shader) {
        try {
          final text = utf8.decode(await source(asset.name));
          if (isDisposed || shader.isDisposed) return;
          shader.reload(text);
        } catch (error, stack) {
          if (!isDisposed) _report(null, error, stack);
        }
      }
    }
  }

  /// Reads again, from the world's source, every shader [assets] loaded
  /// ([reloadShaders]) and every asset of a package's kind that
  /// [AssetKind.reloads], such as sounds, and puts the new files in:
  /// `FynkelGame` calls it on a hot reload. One that fails keeps what it
  /// had, and the error goes to [errors].
  Future<void> reloadAssets() async {
    await reloadShaders();
    final source = _source;
    if (source == null) return;
    for (final asset in [..._assets.values]) {
      final (kind, value) = (asset.kind, asset.value);
      if (value == null || kind is _BuiltInKind || !kind.reloads) continue;
      try {
        final bytes = await source(asset.name);
        if (isDisposed) return;
        // Freed while it was read: nothing to put it in.
        if (!identical(asset.value, value)) continue;
        await kind._reload(this, asset.name, value, bytes);
      } catch (error, stack) {
        if (!isDisposed) _report(null, error, stack);
      }
    }
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

  /// Emits [value] on one of the world's signals, sending what an owner-less
  /// listener throws to [errors]: it mustn't escape the frame.
  void _emit<T>(Signal<T> signal, T value) {
    try {
      signal.emit(value);
    } catch (error, stack) {
      _report(null, error, stack);
    }
  }

  void _report(GameObject? object, Object error, StackTrace stack) {
    if (_errors.hasListener && !_errors.isClosed) {
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

  // The objects with something of the game's to call (a subclass's, or a
  // behaviour's), by [GameObject._order]: the order they became live, so
  // parents come before their children. Destroyed ones are dropped and
  // moved ones put last at the start of the next update. A plain object,
  // and a model's part, isn't in it (DC4: three walks over 32,000 still
  // objects cost 1.3 ms) until a behaviour is put on it: then it joins in
  // its place, before the frame's fixed steps.
  final _live = <GameObject>[];
  var _liveDirty = false;
  final _joining = <GameObject>[];
  var _nextOrder = 1;
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

  // The enabled spline containers, for [Debug.drawSplines].
  final _splineContainers = <SplineContainer>[];

  /// How long the engine took to move every particle system on, in the
  /// last frame: what Unity's Profiler shows as ParticleSystem.Update.
  /// Zero in a world without particle systems. The `Particles` system in
  /// [profile] holds it, with the rest of their frame: the moved objects
  /// sent first, and their events.
  Duration get particleStepTime =>
      Duration(microseconds: maybeSystem<_ParticleWorld>()?._watch.elapsedMicroseconds ?? 0);

  /// How long the last frame took, part by part, and each system's part:
  /// the game's updates, physics, the animators, the particles and the
  /// rest, as Unity's Profiler shows them. `print(world.profile)` lists
  /// them.
  late final FrameProfile profile = FrameProfile._(this);

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
  // readWorldTransforms' buffers, cast once each: physics', audio's.
  final _manyOuts = <Pointer<FyTransform>>[];

  // How many suns and lamps the world has, and whether it has said that
  // it has more than a frame takes ([_countLight]).
  var _suns = 0, _lamps = 0;
  var _warnedSuns = false, _warnedLamps = false;

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
    _emit(onPausedChanged, value);
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

  /// The most frames a second the world runs and its views draw, as
  /// Unity's `Application.targetFrameRate`: 60 unless set, and 0 for every
  /// frame the display shows (120 on a ProMotion iPhone or Mac). A game can
  /// change it at any time, from its settings page for example. The display
  /// sets the frames it can be, so on a 120 Hz screen 90 runs at 60; above
  /// the display's rate it runs at the display's. Fewer frames keep a phone
  /// cool and its battery longer. Throws an ArgumentError below 0.
  int get targetFrameRate => _targetFrameRate;
  set targetFrameRate(int value) {
    if (value < 0) throw ArgumentError.value(value, 'targetFrameRate', 'is below 0');
    _targetFrameRate = value;
  }

  var _targetFrameRate = 60;

  /// What the world's views draw, and how finely ([Quality]), from their
  /// next frame: null, as a new world has, for the level the device picks
  /// ([Quality.device]). A game can change it at any time, from its
  /// settings page for example; `world.quality ?? Quality.device` is what's
  /// drawn. Throws an ArgumentError for settings out of range.
  Quality? get quality => _quality;
  set quality(Quality? value) {
    if (value?._refused() case final error?) throw error;
    using((arena) {
      Pointer<FyQuality> q = nullptr;
      if (value != null) value._write((q = arena<FyQuality>()).ref);
      check(fy_world_quality_set(_engineWorld, q));
    });
    _quality = value;
    _checkEffects();
  }

  // Whether a camera has asked for bloom or ambient occlusion, whether a
  // view draws the world, and whether the world has said its quality
  // doesn't draw what was asked ([_checkEffects]).
  var _bloomAsked = false, _aoAsked = false, _viewed = false, _onScreen = false;
  var _warnedBloom = false, _warnedAo = false;

  /// Says once, in a debug build, for each of bloom and ambient occlusion,
  /// when a camera asks for it and the quality drawn doesn't draw it: the
  /// world's own, or else the device's level's. Only once a view draws it:
  /// a test's world, drawn by none, stays quiet.
  void _checkEffects() {
    if (const bool.fromEnvironment('dart.vm.product')) return;
    final bloom = _bloomAsked && !_warnedBloom, ao = _aoAsked && !_warnedAo;
    if (!bloom && !ao || !_viewed) return;
    final quality = _quality ?? Quality.device;
    String says(String effect, String field) =>
        'Fynkel: a camera asks for $effect, but the quality drawn (${quality.name}'
        '${_quality == null ? ", the device's pick" : ''}) doesn\'t draw it. Fine for a cheaper quality chosen on '
        'purpose; to draw it, use a quality with $field: true.';
    if (bloom && !quality.bloom) {
      _warnedBloom = true;
      print(says('bloom', 'bloom'));
    }
    if (ao && !quality.ambientOcclusion) {
      _warnedAo = true;
      print(says('ambient occlusion', 'ambientOcclusion'));
    }
  }

  Quality? _quality;

  /// What's behind everything, and the light it gives ([Sky]): null, as a
  /// new world has, for none, so the camera's [Camera.clearColor] shows.
  /// From the views' next frame; a new gradient or image costs each view
  /// a few milliseconds of GPU work, once.
  Sky? get sky => _sky;
  set sky(Sky? value) {
    using((arena) {
      final s = arena<FySky>();
      if (value != null) {
        s.ref.lighting = value.lighting;
        value._write(s.ref);
      }
      check(fy_world_sky_set(_engineWorld, value == null ? nullptr : s));
    });
    _sky = value;
  }

  Sky? _sky;

  /// The world's [Fog], or null, as a new world has, for none. Throws an
  /// [ArgumentError] unless `0 ≤ start < end` and `height` is above 0,
  /// leaving it as it was.
  Fog? get fog => _fog;
  set fog(Fog? value) {
    if (value != null) {
      final Fog(:start, :end) = value;
      if (!(start >= 0 && start < 3.4e38)) {
        throw ArgumentError('Fog.start is ${_valueText(start)}: it must be 0 metres or more, in $value', 'start');
      }
      if (!(end > start && end < 3.4e38)) {
        throw ArgumentError(
          'Fog.end is ${_valueText(end)}: it must be beyond its start, ${_valueText(start)} metres, in $value',
          'end',
        );
      }
      if (value.height case final height? when !(height > 0 && height < 3.4e38)) {
        throw ArgumentError('Fog.height is ${_valueText(height)}: it must be above 0 metres, in $value', 'height');
      }
    }
    using((arena) {
      Pointer<FyFog> f = nullptr;
      if (value != null) {
        f = arena<FyFog>();
        final color = value.color;
        if (color != null) f.ref.color.write(color);
        f.ref
          ..use_color = color != null
          ..start = value.start
          ..end = value.end
          ..height = value.height ?? 0;
      }
      check(fy_world_fog_set(_engineWorld, f));
    });
    _fog = value;
  }

  Fog? _fog;

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
  ///    were made, then the routines' waits end ([GameObject.startRoutine]),
  ///    then the objects whose `destroy(after:)` or `release(after:)` is
  ///    due go.
  ///    Then the models' clips pose their parts ([Animator]), and the
  ///    systems' `afterUpdate`.
  /// 6. **Late updates.** Then each object updated this frame, and its
  ///    behaviours, get [GameObject.onLateUpdate] in the same order: for
  ///    what follows where the others went. Then the systems'
  ///    `afterLateUpdate`.
  /// 7. **Cameras.** The camera rigs move their objects ([Follow] and
  ///    [Orbit], then [LookAt]), then [cameras] puts the view's camera
  ///    where the live [VirtualCamera] is, and shakes it.
  /// 8. **Meshes and debug lines.** The meshes changed this frame go to
  ///    the worlds that have them ([Mesh.setPositions]), and what was
  ///    drawn with [debug] goes to the engine, to be drawn over the frame.
  ///    Last, the systems' `afterFrame`.
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
      if (_disposeAsked) {
        dispose();
      } else {
        profile._end();
      }
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
    final profile = this.profile.._begin();
    _tidy();
    _runSystems(_SystemHook.beforeFrame);
    if (_disposeAsked) return;
    profile._lap(_Part.other);

    input
      .._now += dt
      .._step();
    profile._lap(_Part.input);

    var kept = 0;
    for (var i = 0; i < _starting.length; i++) {
      switch (_starting[i]) {
        case final GameObject object:
          // Started already: a plain object that's since joined the loop.
          if (object._destroyed || object._started) continue;
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
    if (_joining.isNotEmpty) _join();
    profile._lap(_Part.starts);

    if (_disposeAsked) return;
    _runFixedSteps();
    if (_disposeAsked) return;
    profile._lap(_Part.fixedUpdates);
    _runSystems(_SystemHook.beforeUpdate);
    if (_disposeAsked) return;
    profile._lap(_Part.other);

    // Objects added from here on are past the end when they come, or not
    // started: they wait for the next frame. The calls are guarded inline,
    // not through [_safely], so a frame makes no closures.
    final updated = _live.length;
    _callEach(_Call.update, updated);
    profile._lap(_Part.updates);

    if (_disposeAsked) return;
    _runMotions();
    _runRoutines();
    if (_disposeAsked) return;
    if (_dueObjects.isNotEmpty) _runDue();
    if (_disposeAsked) return;
    if (_shaders.isNotEmpty) check(fy_world_time_set(pointer, time._sinceStart, time._delta));
    profile._lap(_Part.motions);
    // The animators pose, then the particles move, then the packages'.
    _runSystems(_SystemHook.afterUpdate);
    if (_disposeAsked) return;
    profile._lap(_Part.other);

    _callEach(_Call.lateUpdate, updated);
    profile._lap(_Part.lateUpdates);

    if (_disposeAsked) return;
    // The packages', then the cameras move.
    _runSystems(_SystemHook.afterLateUpdate);
    if (_disposeAsked) return;
    profile._lap(_Part.other);

    Mesh._publishChanges();
    profile._lap(_Part.meshes);
    _debug?._flush();
    profile._lap(_Part.debug);
    _runSystems(_SystemHook.afterFrame);
  }

  // --- Motions ---

  // The running motions not in a group, in the order they were made.
  final _motions = <Motion>[];

  // The objects with a [GameObject.destroy] or [GameObject.release] due,
  // and the ones whose was done or dropped, which [_runDue] takes out.
  final _dueObjects = <GameObject>[];

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
  /// refuses a component on it or on anything below it, nothing is added
  /// and the error is thrown: the world is as it was, and the objects can
  /// be added once mended.
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
  /// disabled behaviour on an active object is found. So is an object still
  /// loading, as Unity finds one before its `Awake`: it isn't drawn,
  /// started or updated yet, and what its [GameObject.onLoad] sets may not
  /// be set; `await object.loaded` before reading that.
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
  /// drawn from (both sides for a [Material.doubleSided] one), each part
  /// ([Mesh.submeshCount]) only with a material of its own and from its
  /// own material's sides, and a skinned character where its skeleton
  /// has bent it to. Cut-outs
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

  /// A view now draws the world: what it draws at is known.
  /// [onScreen] for one the player sees, not a [HeadlessView]: the first
  /// tells the systems ([WorldSystem.viewStarted]).
  void viewStarted({required bool onScreen}) {
    _viewed = true;
    _checkEffects();
    if (!onScreen || _onScreen) return;
    _onScreen = true;
    final systems = _systems;
    for (var i = 0; i < systems.length; i++) {
      final system = systems[i];
      if (system._off) continue;
      try {
        system.viewStarted();
      } catch (error, stack) {
        _systemThrew(system, error, stack);
      }
    }
  }
}

/// What a world's views draw, and how finely: set on [World.quality]. A
/// game sets its own, or starts from a level's ([Quality.low],
/// [Quality.medium], [Quality.high]) and changes what it likes:
///
/// ```dart
/// world.quality = Quality.medium.copyWith(bloom: false, shadowCascades: 1);
/// ```
///
/// Cameras ask for their effects ([Camera.bloom],
/// [Camera.ambientOcclusion]); a quality says whether they're drawn. Like
/// Unity's quality levels: a game can have one, or as many as it likes,
/// and switch between them at any time.
final class Quality {
  /// A quality of the game's own: by default, [Quality.medium]'s.
  const Quality({
    this.name = 'custom',
    this.hdr = true,
    this.bloom = true,
    this.ambientOcclusion = false,
    this.lampHighlights = true,
    this.shadows = true,
    this.shadowCascades = 2,
    this.shadowResolution = 2048,
    this.shadowSoftness = ShadowSoftness.soft,
    this.shadowDistance = 60,
    this.spotShadows = 2,
    this.spotShadowResolution = 1024,
    this.renderScale = 1,
    this.antiAliasing = 4,
    this.softParticles = true,
  });

  /// The cheapest, for slow GPUs and ones not known: drawn straight to the
  /// screen, at three quarters of its pixels across, hard shadows from the
  /// sun and one spot light, no bloom, no ambient occlusion, lamps without
  /// highlights.
  static const low = Quality(
    name: 'low',
    hdr: false,
    bloom: false,
    lampHighlights: false,
    shadowCascades: 1,
    shadowResolution: 1024,
    shadowSoftness: ShadowSoftness.hard,
    shadowDistance: 40,
    spotShadows: 1,
    spotShadowResolution: 512,
    renderScale: 0.75,
    softParticles: false,
  );

  /// Between: bloom, and soft shadows in two cascades; no ambient
  /// occlusion. For a phone that keeps up with it; no device picks it
  /// itself.
  static const medium = Quality(name: 'medium');

  /// Apple's GPUs, and desktops': everything, with softer shadows in three
  /// cascades.
  static const high = Quality(
    name: 'high',
    ambientOcclusion: true,
    shadowCascades: 3,
    shadowSoftness: ShadowSoftness.softer,
    shadowDistance: 100,
    spotShadows: 4,
  );

  /// The three levels, cheapest first.
  static const levels = [low, medium, high];

  /// The level this device picks for a world without a quality of its own
  /// ([World.quality] null): [high] on Apple's GPUs and desktops', [low]
  /// on phones' (Mali, Adreno), on a software GPU and on one not known.
  /// Starts the GPU if no view has yet.
  static Quality get device {
    final out = malloc<Uint32>();
    try {
      check(fy_quality_auto(out));
      return levels[out.value - FY_QUALITY_LOW];
    } finally {
      malloc.free(out);
    }
  }

  /// What it's called, for a settings page or a log: a level's own name,
  /// or a game's.
  final String name;

  /// Whether the scene is drawn into an image of its own, with a last pass
  /// after it, rather than straight to the screen: its brightest light is
  /// kept and toned down evenly. The image holds 10 bits a colour, its
  /// darkest tones dithered so slow gradients don't band. Bloom draws into it whether this is on or
  /// not.
  final bool hdr;

  /// Whether cameras' [Camera.bloom] is drawn.
  final bool bloom;

  /// Whether cameras' [Camera.ambientOcclusion] is drawn: a depth pass and
  /// two half-size passes more.
  final bool ambientOcclusion;

  /// Whether point and spot lights give shiny surfaces a highlight, as
  /// suns always do. Without, a lamp lights a surface evenly, which is
  /// cheaper where many overlap.
  final bool lampHighlights;

  /// Whether lights' shadows are drawn ([Light.shadows]): without, none
  /// are, whatever the lights ask, and the shadow settings below aren't
  /// used.
  final bool shadows;

  /// How many maps the sun's shadows are split into, 1 to 3: the nearest
  /// sharp, the further ones still there.
  final int shadowCascades;

  /// Each of the sun's maps' side, in texels: a power of two, 256 to 4096.
  final int shadowResolution;

  /// How shadows' edges are drawn.
  final ShadowSoftness shadowSoftness;

  /// How far from the camera the sun's shadows reach, in metres (or the
  /// camera's far plane, if nearer). They fade out before it.
  final double shadowDistance;

  /// The most spot lights casting shadows in a frame, 0 to 4: the ones
  /// nearest the camera.
  final int spotShadows;

  /// Each spot light's map's side, as [shadowResolution].
  final int spotShadowResolution;

  /// Pixels drawn per pixel of the screen, as Unity's render scale: above
  /// 0 and at most 2; 0.75 at low, 1 at medium and high. Below 1 draws
  /// fewer pixels, which is faster and keeps a phone cooler, and the
  /// picture is a little softer: at 0.75, about half the pixels (a Galaxy
  /// Note10+ draws Sheepdog at 55 fps, against 43 at 1). Each view's own
  /// `renderScale` multiplies it.
  final double renderScale;

  /// How many samples each pixel is drawn with, which smooths the edges of
  /// what's drawn (MSAA), as Unity's anti-aliasing setting: 0 (none), 2 or
  /// 4, and 4 at every level. A device that can't draw 2 draws none. On
  /// phones' GPUs the samples stay on the chip, so 4 costs less than it
  /// sounds (a Note10+ at low gained nothing without it, at 0.75); without,
  /// edges are jagged and shimmer as things move.
  final int antiAliasing;

  /// Whether particles with [ParticleSystem.softness] fade where they meet
  /// what's behind them (Unity's soft particles setting): off at low,
  /// where they're hard, since reading the scene's depth costs a pass
  /// more on a phone.
  final bool softParticles;

  /// A copy with the values given changed.
  Quality copyWith({
    String? name,
    bool? hdr,
    bool? bloom,
    bool? ambientOcclusion,
    bool? lampHighlights,
    bool? shadows,
    int? shadowCascades,
    int? shadowResolution,
    ShadowSoftness? shadowSoftness,
    double? shadowDistance,
    int? spotShadows,
    int? spotShadowResolution,
    double? renderScale,
    int? antiAliasing,
    bool? softParticles,
  }) => Quality(
    name: name ?? this.name,
    hdr: hdr ?? this.hdr,
    bloom: bloom ?? this.bloom,
    ambientOcclusion: ambientOcclusion ?? this.ambientOcclusion,
    lampHighlights: lampHighlights ?? this.lampHighlights,
    shadows: shadows ?? this.shadows,
    shadowCascades: shadowCascades ?? this.shadowCascades,
    shadowResolution: shadowResolution ?? this.shadowResolution,
    shadowSoftness: shadowSoftness ?? this.shadowSoftness,
    shadowDistance: shadowDistance ?? this.shadowDistance,
    spotShadows: spotShadows ?? this.spotShadows,
    spotShadowResolution: spotShadowResolution ?? this.spotShadowResolution,
    renderScale: renderScale ?? this.renderScale,
    antiAliasing: antiAliasing ?? this.antiAliasing,
    softParticles: softParticles ?? this.softParticles,
  );

  /// Why a world can't have it, if it can't: what each field's doc says.
  /// The shadow settings count only with [shadows].
  ArgumentError? _refused() {
    ArgumentError no(String field, Object value, String why) =>
        ArgumentError('Quality.$field of $name is $value: $why', field);
    bool size(int n) => n >= 256 && n <= 4096 && n & (n - 1) == 0;
    if (!(renderScale > 0 && renderScale <= 2)) return no('renderScale', renderScale, 'above 0 and at most 2');
    if (![0, 2, 4].contains(antiAliasing)) return no('antiAliasing', antiAliasing, '0, 2 or 4');
    if (!shadows) return null;
    if (shadowCascades < 1 || shadowCascades > 3) return no('shadowCascades', shadowCascades, 'from 1 to 3');
    if (!size(shadowResolution)) return no('shadowResolution', shadowResolution, 'a power of two, 256 to 4096');
    if (!size(spotShadowResolution)) {
      return no('spotShadowResolution', spotShadowResolution, 'a power of two, 256 to 4096');
    }
    if (spotShadows < 0 || spotShadows > 4) return no('spotShadows', spotShadows, 'from 0 to 4');
    if (!(shadowDistance > 0 && shadowDistance < 3.4e38)) {
      return no('shadowDistance', shadowDistance, 'more than 0 metres');
    }
    return null;
  }

  void _write(FyQuality q) {
    q
      ..hdr = hdr
      ..bloom = bloom
      ..ambient_occlusion = ambientOcclusion
      ..lamp_highlights = lampHighlights
      ..shadows = shadows
      ..shadow_cascades = shadowCascades
      ..shadow_resolution = shadowResolution
      ..shadow_softness = shadowSoftness._value
      ..shadow_distance = shadowDistance
      ..spot_shadows = spotShadows
      ..spot_shadow_resolution = spotShadowResolution
      ..samples = antiAliasing == 0 ? 1 : antiAliasing
      ..soft_particles = softParticles;
  }

  List<Object> get _fields => [
    name,
    hdr,
    bloom,
    ambientOcclusion,
    lampHighlights,
    shadows,
    shadowCascades,
    shadowResolution,
    shadowSoftness,
    shadowDistance,
    spotShadows,
    spotShadowResolution,
    renderScale,
    antiAliasing,
    softParticles,
  ];

  @override
  bool operator ==(Object other) {
    if (other is! Quality) return false;
    final (a, b) = (_fields, other._fields);
    for (var i = 0; i < a.length; i++) {
      if (a[i] != b[i]) return false;
    }
    return true;
  }

  @override
  int get hashCode => Object.hashAll(_fields);

  @override
  String toString() =>
      'Quality($name: hdr $hdr, bloom $bloom, ambientOcclusion $ambientOcclusion, lampHighlights $lampHighlights, '
      'shadows ${shadows ? '$shadowCascades × $shadowResolution ${shadowSoftness.name} to ${shadowDistance}m, '
                'spot $spotShadows × $spotShadowResolution' : 'off'}, renderScale $renderScale, '
      'antiAliasing $antiAliasing, softParticles $softParticles)';
}

/// How a shadow's edge is drawn ([Quality.shadowSoftness]).
enum ShadowSoftness {
  /// Sharp: one sample.
  hard(FY_SHADOWS_HARD),

  /// Soft: 4 samples, across about 2 texels.
  soft(FY_SHADOWS_SOFT),

  /// Softer: 9 samples, across about 3 texels.
  softer(FY_SHADOWS_SOFTER);

  const ShadowSoftness(this._value);

  final int _value;
}
