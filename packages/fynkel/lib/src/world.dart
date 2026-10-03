// The Dart gameplay API (design/dart-sdk.md §21.2, ADR-0008): World,
// GameObject and Transform handles over native logical data, and (behaviour.dart)
// the Behaviour lifecycle the world drives at the §5.1 points of a host
// frame. Scenes come from the cooked cache through `loadProject`/`loadScene`.

import 'dart:async' show Completer, Zone;
import 'dart:convert' show jsonDecode, jsonEncode;
import 'dart:math' as math;
import 'dart:typed_data';

import 'entity_ref.dart';

import '../generated/schema.g.dart' as gen;
import '../math.dart';
import 'animation.dart';
import 'asset_index.dart';
import 'asset_ref.dart';
import 'audio.dart';
import 'clip_data.dart';
import 'context.dart';
import 'diagnostics.dart';
import 'editor_channel.dart';
import 'events.dart';
import 'gizmos.dart';
import 'input.dart';
import 'layers.dart';
import 'mesh_data.dart';
import 'modules.dart';
import 'native/bridge.dart';
import 'physics.dart';
import 'settings.dart';
import 'systems.dart';
import 'transform_writer.dart';
import 'wire/buffers.dart';
import 'wire/support.dart' show WireRecord, WireStrings, writeVec3;

export 'native/bridge.dart' show BridgeStats, FramePlan, StepResult;

part 'behaviour.dart';
part 'gameplay.dart';
part 'registered_components.dart';
part 'gameplay_systems.dart';
part 'data_asset.dart';
part 'object.dart';
part 'object_spec.dart';
part 'projection.dart';
part 'scenes.dart';
part 'tween.dart';

/// Immutable local transform value (position, rotation, scale).
typedef Pose = gen.Transform;

const Pose identityTransform = Pose(position: Vec3.zero, rotation: Quat.identity, scale: Vec3.one);

/// Runtime entity/component container over one native world.
class World {
  List<GameplaySystem> _systems = const [];
  List<String> _systemOrder = const [];
  bool _runningSystems = false;
  final Map<String, GameplaySystemTime> _systemTimes = {};
  final List<GameplaySystemFailure> _systemFailures = [];

  final Map<String, ComponentType<dynamic>> _componentTypes = {};
  World._(this._lib, this._native, {required this.ownsLibrary, required this.fixedDt, BehaviourRegistry? registry})
    : registry = registry ?? BehaviourRegistry() {
    input = WorldInput(() => _native.inputView)..onUndeclared = _undeclaredAction;
    context = GameContext(time: _WorldTime(this), input: input);
    _behaviours = _BehaviourSystem(this);
    scenes = SceneManager._(this);
    projection = ProjectionView._(this);
    // The editor's Play reads this process's stdout (ADR-0016): say the
    // world exists before anything is loaded, so the Console has a first
    // row within a frame of the launch.
    EditorChannel.state('started', tick: 0, fields: {'pid': EditorChannel.processId, 'fixedDt': fixedDt});
  }

  /// Creates a world on a fresh [NativeLibrary] (or the given one).
  factory World.create({
    NativeLibrary? library,
    double fixedDt = 0,
    int maxStepsPerFrame = 0,
    double maxFrameDt = 0,
    BehaviourRegistry? registry,
  }) {
    final lib = library ?? NativeLibrary.open();
    final native = lib.createWorld(fixedDt: fixedDt, maxStepsPerFrame: maxStepsPerFrame, maxFrameDt: maxFrameDt);
    return World._(
      lib,
      native,
      ownsLibrary: library == null,
      fixedDt: fixedDt > 0 ? fixedDt : 1 / 60,
      registry: registry,
    )..maxFrameDt = maxFrameDt > 0 && maxFrameDt.isFinite ? maxFrameDt : 0.25;
  }

  /// The most game time one host frame counts for (seconds; the native
  /// clock's clamp, `MAX_FRAME_DT` 0.25 by default — Unity's
  /// `Time.maximumDeltaTime`): after a two-second hitch the ticks,
  /// `Time.delta`, `Time.elapsed` and coroutine waits all move 0.25 s.
  late final double maxFrameDt;

  /// Seconds per fixed tick (the engine default when not given).
  final double fixedDt;

  /// Behaviour types this world can instantiate from authored data
  /// ([GameObject.addBehaviourFromData]).
  final BehaviourRegistry registry;

  /// The services behaviours see; current during every callback. Replace it
  /// (a [TestGameContext], say) before the first frame.
  late GameContext context;

  /// The project's actions as the native binding table reports them
  /// (`inputActions`, bound on load), refreshed by [frame] before each
  /// tick's callbacks and before `update`. [context]'s input service.
  late final WorldInput input;

  final RecordBatch _inputEvents = RecordBatch(BufferKind.inputEvents, gen.InputEventRecord.wireSize);
  bool _inputDirty = false;

  /// Scene flow (M3-12): the scenes the world holds, `transition(to)`
  /// performed at the start of the next [frame], its observable state.
  late final SceneManager scenes;

  /// Named zone events (M3-12): `zones.on('collect', (e) { … })` runs
  /// right after the tick whose step produced the `ZoneEntered`.
  late final ZoneEvents zones = ZoneEvents(this);

  /// Projected world anchors (M3-13, docs/hud.md): where entities land on
  /// the screen this frame, for labels drawn over the viewport. Register
  /// anchors, set the viewport, `update()` once per frame after [frame]
  /// (`FynkelGameView` does).
  late final ProjectionView projection;

  /// The cooked cache the last successful load came from; the default
  /// [loadScene] cache.
  String? get cacheDir => _cacheDir;
  String? _cacheDir;

  /// The project's assets by path, from the cache of the last load
  /// (ADR-0026 §3): `assets.find('prefabs/burst')` is an [AssetRef], as
  /// Unity's `Resources.Load`. Names known while writing the code are the
  /// generated `Assets.…` constants instead.
  late final AssetIndex assets = AssetIndex(() => _cacheDir);

  /// Installs [store] as [context]'s settings and attaches it to this
  /// world (M3-12): its rebinds go over the project's actions now and
  /// after every update. `world.useSettings(SettingsStore.open(
  /// SettingsStore.defaultPath('My Game'), world: world))` is the usual
  /// startup line, after `loadProject`.
  void useSettings(SettingsStore store) {
    _rejectDuringEdit('useSettings');
    context.settings = store;
    store.attach(this);
  }

  /// The physics settings of the last `loadProject` (gravity); the
  /// defaults before a load.
  PhysicsSettings physics = const PhysicsSettings();

  /// The project's layers and tags from the last `loadProject` (ADR-0020);
  /// `default` and no tags before a load. [layerMask] reads the names.
  LayersAndTags layersAndTags = const LayersAndTags();

  /// The optional modules the loaded project runs (ADR-0029): every one
  /// before a load. A module the project turns off (`modules` in
  /// `fynkel.project.json`) does not run, and the calls into it throw a
  /// [StateError] — `world.modules.has(Module.audio)` answers first.
  Modules modules = Modules.all;

  /// The audio engine (M3-11): `audio.open()` for the device, `audio.play`
  /// for a fire-and-forget sound, `audio.setBus` / `applySettings` for the
  /// gains, `audio.stats`. A source on an object plays through
  /// [GameObject.audio].
  late final AudioSystem audio = AudioSystem._(this);

  final RecordBatch _physicsQueries = RecordBatch(BufferKind.physicsQueries, gen.PhysicsQueryRecord.wireSize);
  int _nextQueryId = 1;

  late final _BehaviourSystem _behaviours;

  /// Every attached behaviour, in callback order.
  List<Behaviour> get behaviours =>
      _gameplay?.allBehaviours() ?? List.unmodifiable(_behaviours._all.where((b) => !b._retired));

  /// Host frames begun and the last frame's dt, scaled and not (for
  /// [TimeService]).
  int _frameCount = 0;
  double _frameDt = 0, _unscaledDt = 0;
  double _gameplayElapsed = 0, _realElapsed = 0;
  int _pendingFrameSteps = 0;

  /// How fast game time runs (`Time.timeScale`, foundation 1.3): [beginFrame]
  /// supplies real dt and the requested scale to the native clock; 0 runs no tick while `update`
  /// still runs with a delta of 0.
  double get timeScale => _gameplay?.timeScale ?? _timeScale;
  set timeScale(double value) {
    final edit = _gameplay;
    if (!(value >= 0) || !value.isFinite) {
      edit?.reject('timeScale must be finite and >= 0');
      throw ArgumentError.value(value, 'timeScale', 'must be finite and >= 0');
    }
    if (edit != null) {
      edit.timeScale = value;
    } else {
      _timeScale = value;
    }
  }

  double _timeScale = 1;

  /// True while [frame] runs its ticks (the host's and the behaviours'
  /// `fixedUpdate`, the step, `afterPhysics` and the contact callbacks):
  /// `Time.delta` is then the fixed step, as Unity's `Time.deltaTime` is
  /// `fixedDeltaTime` inside FixedUpdate.
  bool _inFixedPhase = false;

  /// True during update/lateUpdate callback dispatch. Writes still use the
  /// shared gameplay transaction; this flag does not select another state store.
  bool _inUpdatePhase = false;

  final NativeLibrary _lib;
  final NativeWorld _native;
  final bool ownsLibrary;

  /// Low-level access for hosts and tests.
  NativeWorld get native {
    _rejectDuringEdit('native escape hatch');
    return _native;
  }

  final Map<EntityRef, GameObject> _byEntity = {};

  /// The project's `persistentObjects` the last [loadProject] made, by
  /// name (ADR-0039).
  final Map<String, GameObject> _projectPersistent = {};

  /// Last snapshot: entity → record (read in place, decoded on access).
  WireBuffer _snapshot = WireBuffer.empty;
  final Map<EntityRef, int> _snapshotIndex = {};

  final List<EntityRef> _toSubscribe = [];

  /// The objects whose transform rides the snapshot (foundation 4.4,
  /// ADR-0004: Dart receives only what it observes). An object joins on
  /// its first transform read or write, or with its first behaviour, and
  /// leaves after [_observeTicks] ticks without a read unless it has a
  /// behaviour; a handle alone costs nothing. The physics reports stay
  /// events and character hits for these objects (Unity sends them to
  /// scripts).
  final Set<GameObject> _observed = {};
  static const _observeTicks = 60;

  int _steps = 0;

  /// [go]'s transform was read or written: it rides the snapshot from the
  /// next tick (a pending object from the tick after its spawn).
  void _observe(GameObject go) {
    go._readStep = _steps;
    if (go._observed) return;
    final e = go._entity;
    if (e == null || !go._alive) {
      if (e == null && (_gameplay?.pending.containsKey(go) ?? false)) {
        _gameplay?.afterCommit(() {
          if (go._alive) _observe(go);
        });
      }
      return;
    }
    go._observed = true;
    _observed.add(go);
    _toSubscribe.add(e);
  }

  /// Drops the objects not read for [_observeTicks] ticks.
  void _sweepObserved() {
    final drop = <EntityRef>[];
    _observed.removeWhere((go) {
      if (go._alive && (go._behaviours.isNotEmpty || _steps - go._readStep < _observeTicks)) {
        return false;
      }
      go._observed = false;
      final e = go._entity;
      if (go._alive && e != null) drop.add(e);
      return true;
    });
    if (drop.isNotEmpty) {
      _checkFatal(_native.subscribeTransforms(drop, enable: false), 'unsubscribe');
    }
  }

  /// Called with the diagnostics of each step/submit in release mode
  /// (debug mode throws [FynkelError] instead). Defaults to `print` —
  /// silent under the editor channel, which already carries the record
  /// ([EditorChannel]), so the Console shows each diagnostic once.
  void Function(List<FynkelDiagnostic>) onDiagnostics = _printDiagnostics;

  /// Called when a behaviour callback throws (docs/unity-parity.md § Tier
  /// 2): as in Unity, only that call stops — the other behaviours still
  /// run this pass, and the one that threw is called again next time.
  /// Defaults to [reportBehaviourError].
  void Function(BehaviourError error) onBehaviourError = reportBehaviourError;

  void _undeclaredAction(String action) {
    final message = "Input action '$action' is not declared in fynkel.project.json → inputActions; it reads as nothing";
    if (EditorChannel.enabled) {
      EditorChannel.log(message, source: 'input', severity: 'warning', tick: tick);
    } else {
      print('[input] $message');
    }
  }

  /// The default [onBehaviourError]: an `error` log record with the Dart
  /// stack on the editor channel when an editor listens (the Console's
  /// Game tab, a click to the line); else the error goes to the current
  /// zone's uncaught-error handler — a test fails, a Flutter app reports
  /// it — or, in the root zone of a plain Dart process, to `print`, so the
  /// game keeps running.
  static void reportBehaviourError(BehaviourError e) {
    if (EditorChannel.enabled) {
      EditorChannel.error(e.message, stack: e.stack.toString(), tick: e.tick);
    } else if (!Zone.current.inSameErrorZone(Zone.root)) {
      Zone.current.handleUncaughtError(e.error, e.stack);
    } else {
      print('${e.message}\n${e.stack}');
    }
  }

  /// Throw [FynkelError] on error diagnostics (rejections, protocol errors).
  /// Informational ones ([FynkelDiagnostic.isInformational], e.g. a missed
  /// deadline) always go to [onDiagnostics]. Defaults to [isDebugMode].
  bool throwOnDiagnostics = isDebugMode;

  /// This frame's [Gizmos] lines, when they are on ([showGizmos]); null
  /// otherwise — and always in a release build. The world clears it at the
  /// start of every frame; the view draws it after [frame].
  GizmoBuffer? get gizmos => Gizmos.compiled ? context.serviceOrNull<GizmoBuffer>() : null;

  /// Turns [Gizmos] on or off for this world's context (off by default;
  /// ignored in a release build).
  bool get showGizmos => gizmos != null;
  set showGizmos(bool on) {
    if (!Gizmos.compiled) return;
    if (on && gizmos == null) {
      context.install<GizmoBuffer>(GizmoBuffer());
    } else if (!on) {
      context.uninstall<GizmoBuffer>();
    }
  }

  int get tick => _native.tick;
  double get alpha => _native.alpha;
  bool get isPoisoned => _native.isPoisoned;

  /// Why the engine stopped this world ([isPoisoned]): the message of the
  /// panic it caught; null while it runs (foundation 4.1).
  String? get panicMessage => _native.isPoisoned ? _native.panicMessage : null;

  /// Panics inside the engine on purpose (Unity's `Utils.ForceCrash`), to
  /// test what a game does when the engine stops: throws the
  /// [FynkelPanicError], and the world is poisoned for good.
  void debugPanic() => _native.debugPanic();
  Iterable<GameObject> get objects => _withGameplay(() => _gameplay!.objects());

  _GameplayAdapter? _activeGameplay;
  int _gameplayStateRevision = 0;
  int _lifecycleDepth = 0;

  /// Atomically edits built-in gameplay configuration through ordinary object
  /// APIs. Reads inside the synchronous callback see its writes. Surviving
  /// spawns resolve before this returns; an exception discards the edit.
  ///
  /// May run in synchronous frame/lifecycle callbacks. Entry reconciles live
  /// identities and names without committing work or consuming events.
  /// Body, audio and animation requests publish on commit and execute at the
  /// next eligible tick. Scene replacement, runtime resource mutation and
  /// immediate effects remain guarded during edits.
  /// Arbitrary Dart variables/services are outside the rollback boundary.
  R edit<R>(R Function() callback) => _editGameplay(callback);

  // ---------------------------------------------------------------- spawn

  /// Creates an object through the gameplay transaction. Outside a callback or
  /// [edit], publication completes before this returns. Inside a transaction,
  /// the handle remains pending until that transaction commits and is usable
  /// for component, hierarchy and effect requests within that scope.
  GameObject spawn({Pose transform = identityTransform, GameObject? parent, String name = ''}) {
    return _spawn(transform, parent, name);
  }

  GameObject _spawn(Pose transform, GameObject? parent, String name, [GameObject? handle]) =>
      _withGameplay(() => _gameplay!.spawn(transform, parent, name, handle));

  /// Builds [spec] and everything under it (ADR-0026 §2) and returns its
  /// root: under [parent], at [transform] (default: the spec's own).
  /// The tree and prefab resources publish together, as [spawn] does. The whole
  /// spec is checked first — a behaviour already attached or listed twice,
  /// an unknown tag or layer, a sub-asset that is not a prefab — so a
  /// mistake throws before anything exists.
  /// Inside [edit], ordinary trees and primitives are transactional, including
  /// forward component and script references. Each spec may be built once per edit.
  /// The first successful build promotes its placeholder to the canonical handle
  /// without rewriting script fields; cooked prefab nodes use prepared resources and
  /// lifecycle callbacks wait for the next normal boundary. Spec object bindings
  /// publish only on commit, so abort preserves the previous build's binding.
  GameObject build(ObjectSpec spec, {GameObject? parent, Pose? transform}) {
    return _withGameplay(() => _gameplay!.build(spec, parent, transform ?? spec.transform));
  }

  /// Builds a forest atomically, resolving references between its roots before
  /// component values and behaviours are attached. No hidden parent is created.
  List<GameObject> buildAll(Iterable<ObjectSpec> specs, {GameObject? parent}) =>
      _withGameplay(() => _gameplay!.buildAll(List<ObjectSpec>.of(specs), parent));

  /// A built-in shape (Unity's `GameObject.CreatePrimitive`, ADR-0026
  /// §4): [ObjectSpec.primitive] built under [parent] at [transform] —
  /// the shape's mesh, the default material and Unity's collider for it.
  GameObject createPrimitive(Primitive shape, {String? name, Pose transform = identityTransform, GameObject? parent}) =>
      build(
        ObjectSpec.primitive(shape, name: name, transform: transform),
        parent: parent,
      );

  // ------------------------------------------ runtime materials (ADR-0026 §4)

  static final math.Random _ids = math.Random.secure();

  /// A material made in code (Unity's `new Material(shader)`): a new
  /// asset id that a `MeshRenderer`'s `material` takes like a cooked one,
  /// drawn with [material]'s values from the next frame and kept across
  /// scene loads until [destroyMaterial]. Change it with [setMaterial].
  /// [createShaderMaterial] makes one of a `.shader` instead. Throws
  /// [StateError] before a project or scene is loaded (the material lives
  /// in that asset store).
  MaterialAsset createMaterial([gen.PbrMaterial material = const gen.PbrMaterial()]) {
    final ref = _MadeMaterial(this, _newAssetId());
    setMaterial(ref, material);
    return ref;
  }

  /// A material of a `.shader` asset made in code (Unity's
  /// `new Material(shader)`, ADR-0031): [shader] is the shader's
  /// [AssetRef] or its path under `assets/` (`'shaders/water.shader'`).
  /// A new asset id that a `MeshRenderer`'s `material` takes like a cooked
  /// one, drawn from the next frame and kept across scene loads until
  /// [destroyMaterial]. Its properties are set from [values] by name, the
  /// rest at the shader's defaults; change one with [setMaterialValue].
  ///
  /// A value is a number, a bool, a [Vec2]/[Vec3]/[Vec4] (a colour is a
  /// [Vec3] or [Vec4], linear), or an [AssetRef] for a texture property.
  /// Values are checked against the shader's header as the cook checks a
  /// material file: a wrong name, type or range throws [ArgumentError]
  /// with the diagnostic. Throws [StateError] before a project is loaded.
  MaterialAsset createShaderMaterial(
    Object shader, {
    Map<String, Object> values = const {},
    gen.AlphaMode alphaMode = gen.AlphaMode.opaque,
    double alphaCutoff = 0.5,
    bool doubleSided = false,
  }) => _withGameplay(
    () => _gameplay!.guard(() {
      final edit = _gameplay!;
      final shaderRef = switch (shader) {
        AssetRef r => r,
        String path =>
          assets.find(path) ?? (throw ArgumentError.value(path, 'shader', 'no such asset in the last cook')),
        _ => throw ArgumentError.value(shader, 'shader', 'an AssetRef or a path under assets/'),
      };
      final ref = _MadeMaterial(this, _newAssetId());
      final doc = <String, Object?>{
        r'$schema': 'https://fynkel.dev/schemas/material.schema.json',
        'formatVersion': 2,
        'id': ref.asset,
        'name': 'Made in code',
        'shader': {'asset': shaderRef.asset},
        'parameters': <String, Object?>{},
        'textures': <String, Object?>{},
        'alpha_mode': alphaMode.name,
        'alpha_cutoff': alphaCutoff,
        'double_sided': doubleSided,
      };
      values.forEach((name, value) => _putValue(doc, name, value));
      _sendShaderMaterial(ref, doc, 'createShaderMaterial');
      edit._shaderMaterials[ref.asset] = doc;
      return ref;
    }),
  );

  /// Sets the property [name] of the shader material [ref] to [value]
  /// (Unity's `material.SetFloat` / `SetColor` / `SetTexture`, ADR-0031):
  /// one [createShaderMaterial] made, or a cooked material of a
  /// `.shader`, changed for this run until [destroyMaterial]. Every
  /// renderer using it changes from the next frame; a new value rewrites
  /// the material's uniform and nothing else, so calling this every frame
  /// is fine. [value] is as [createShaderMaterial]'s `values`; null puts the
  /// shader's default back. A wrong name, type or range throws
  /// [ArgumentError] with the diagnostic, and the material keeps its
  /// values.
  void setMaterialValue(AssetRef ref, String name, Object? value) => _withGameplay(
    () => _gameplay!.guard(() {
      final edit = _gameplay!;
      final doc = _shaderMaterial(ref.asset) ?? _cookedShaderMaterial(ref);
      final changed = _copyDoc(doc);
      _putValue(changed, name, value);
      _sendShaderMaterial(ref, changed, 'setMaterialValue');
      edit._shaderMaterials[ref.asset] = changed;
      edit._codeValues[ref.asset] = {
        ...?_materialCodeValues(ref.asset),
        name: value is List<num> ? List<num>.unmodifiable(value) : value,
      };
    }),
  );

  /// An Inspector edit of the material [id] during Play, saved (as in
  /// Unity): when code changed it with [setMaterialValue], its values for
  /// this run start again from the saved [fields], with what code set on
  /// top — so the edit shows, and a property the code drives stays the
  /// code's. False when code has not touched [id] (the editor draws the
  /// edit over the cooked material itself), or the edit left `.shader`
  /// (a Lit material now: the code's copy is dropped).
  bool applyMaterialEdit(String id, Map<String, Object?> fields) => _withGameplay(
    () => _gameplay!.guard(() {
      final edit = _gameplay!;
      if (_shaderMaterial(id) == null) return false;
      final ref = AssetRef(id);
      if (fields['shader'] is! Map) {
        edit._shaderMaterials[id] = null;
        edit._codeValues[id] = null;
        _sendShaderMaterial(ref, null, 'applyMaterialEdit');
        return false;
      }
      final changed = _copyDoc(fields);
      _materialCodeValues(id)?.forEach((name, value) => _putValue(changed, name, value));
      _sendShaderMaterial(ref, changed, 'applyMaterialEdit');
      edit._shaderMaterials[id] = changed;
      return true;
    }),
  );

  /// The value [setMaterialValue] or [createShaderMaterial] gave the property
  /// [name] of [ref] — a number, bool, list of numbers or [AssetRef] —
  /// or null when code did not set it (the material's own value holds).
  Object? materialValue(AssetRef ref, String name) {
    final doc = _shaderMaterial(ref.asset);
    if (doc == null) return null;
    final texture = (doc['textures'] as Map)[name];
    if (texture is Map) {
      final t = (texture['texture'] as Map).cast<String, Object?>();
      return AssetRef(t['asset'] as String, sub: t['sub'] as String?);
    }
    final value = (doc['parameters'] as Map)[name];
    return value is List ? List<Object?>.unmodifiable(value) : value;
  }

  static T? _resourceView<T>(Map<String, T?>? staged, Map<String, T> committed, String id) =>
      staged != null && staged.containsKey(id) ? staged[id] : committed[id];
  Map<String, Object?>? _shaderMaterial(String id) => _resourceView(_gameplay?._shaderMaterials, _shaderMaterials, id);
  Map<String, Object?>? _materialCodeValues(String id) => _resourceView(_gameplay?._codeValues, _codeValues, id);
  void _checkRuntimeResource(AssetRef ref, String kind) {
    if (ref.bytes == null) throw ArgumentError.value(ref, 'ref', 'not a UUID');
    if (ref.sub?.isNotEmpty ?? false) throw ArgumentError.value(ref, 'ref', 'a runtime resource is a whole asset');
    if (_cacheDir == null) throw StateError('$kind need a loaded project or scene — loadProject or loadScene first');
  }

  final Map<String, Map<String, Object?>> _shaderMaterials = {};

  /// What [setMaterialValue] set, per cooked material: the values an
  /// [applyMaterialEdit] keeps.
  final Map<String, Map<String, Object?>> _codeValues = {};

  /// The cooked shader material [ref]'s document, the start of its
  /// runtime values.
  Map<String, Object?> _cookedShaderMaterial(AssetRef ref) {
    final id = ref.bytes;
    if (id == null) throw ArgumentError.value(ref, 'ref', 'not a UUID');
    if (_cacheDir == null) {
      throw StateError('materials need a loaded project (their asset store) — loadProject first');
    }
    final (status, json) = _gameplay == null
        ? _native.assetJson(id)
        : (FynkelStatus.FYNKEL_STATUS_OK, _gameplay!.native.assetJson(ref.asset));
    if (status != FynkelStatus.FYNKEL_STATUS_OK) {
      throw ArgumentError.value(ref, 'ref', '${json['message'] ?? 'not in the cache'}');
    }
    final shader = json['shader'];
    if (json[r'$schema'] is! String ||
        !(json[r'$schema'] as String).endsWith('/material.schema.json') ||
        shader is! Map) {
      throw ArgumentError.value(
        ref,
        'ref',
        '${json['name'] ?? ref.asset} is not a material of a .shader (a Lit material changes with setMaterial)',
      );
    }
    final doc = _copyDoc(json);
    doc['parameters'] ??= <String, Object?>{};
    doc['textures'] ??= <String, Object?>{};
    return doc;
  }

  static Map<String, Object?> _copyDoc(Map<String, Object?> doc) {
    final copied = (jsonDecode(jsonEncode(doc)) as Map).cast<String, Object?>();
    copied['parameters'] = {...?(copied['parameters'] as Map?)?.cast<String, Object?>()};
    copied['textures'] = {...?(copied['textures'] as Map?)?.cast<String, Object?>()};
    return copied;
  }

  /// [value] as the material document holds the property [name].
  static void _putValue(Map<String, Object?> doc, String name, Object? value) {
    final parameters = doc['parameters'] as Map<String, Object?>;
    final textures = doc['textures'] as Map<String, Object?>;
    parameters.remove(name);
    textures.remove(name);
    switch (value) {
      case null:
        return;
      case AssetRef r:
        textures[name] = {
          'texture': {'asset': r.asset, if (r.sub?.isNotEmpty ?? false) 'sub': r.sub},
        };
      case bool b:
        parameters[name] = b;
      case num n:
        parameters[name] = n;
      case Vec2 v:
        parameters[name] = [v.x, v.y];
      case Vec3 v:
        parameters[name] = [v.x, v.y, v.z];
      case Vec4 v:
        parameters[name] = [v.x, v.y, v.z, v.w];
      case List<num> l:
        parameters[name] = List<num>.unmodifiable(l);
      default:
        throw ArgumentError.value(
          value,
          name,
          'a material value is a number, a bool, a Vec2/Vec3/Vec4, a list of numbers or an AssetRef (a texture)',
        );
    }
  }

  void _sendShaderMaterial(AssetRef ref, Map<String, Object?>? doc, String call) {
    _checkRuntimeResource(ref, 'materials');
    final edit = _gameplay!;
    try {
      edit.native.setRuntimeShaderMaterial(ref.asset, doc == null ? null : jsonEncode(doc));
    } on StateError catch (error) {
      throw ArgumentError('$call: ${error.message}');
    }
    edit._materials[ref.asset] = null;
  }

  /// A new random (v4) asset id: what a made material or mesh is named.
  static AssetRef _newAssetId() {
    final id = Uint8List(16);
    for (var i = 0; i < 16; i++) {
      id[i] = _ids.nextInt(256);
    }
    id[6] = (id[6] & 0x0f) | 0x40; // UUID v4
    id[8] = (id[8] & 0x3f) | 0x80;
    return AssetRef.fromBytes(id, '');
  }

  /// Gives the material [ref] new values (Unity's `material.color = …`):
  /// one [createMaterial] made, or a cooked material, replaced for this
  /// run until [destroyMaterial]. Every renderer using it changes.
  void setMaterial(AssetRef ref, gen.PbrMaterial material) => _setMaterial(ref, material);

  /// Removes the runtime material [ref]: a created one draws as missing
  /// again, a replaced cooked one goes back to its cooked values.
  void destroyMaterial(AssetRef ref) => _setMaterial(ref, null);

  /// The values [setMaterial] or [createMaterial] last gave [ref] (null
  /// for a material this world did not make or replace): the start of a
  /// change, `setMaterial(ref, runtimeMaterial(ref)!.copyWith(…))`.
  gen.PbrMaterial? runtimeMaterial(AssetRef ref) => _resourceView(_gameplay?._materials, _runtimeMaterials, ref.asset);
  final Map<String, gen.PbrMaterial> _runtimeMaterials = {};

  void _setMaterial(AssetRef ref, gen.PbrMaterial? material) => _withGameplay(
    () => _gameplay!.guard(() {
      final edit = _gameplay!;
      _checkRuntimeResource(ref, 'materials');
      edit.native.setRuntimeMaterial(ref.asset, material);
      edit._materials[ref.asset] = material;
      edit._shaderMaterials[ref.asset] = null;
      edit._codeValues[ref.asset] = null;
    }),
  );

  // --------------------------------------------- runtime meshes (ADR-0026 §4)

  /// A mesh made in code (Unity's `new Mesh()`): a new asset id that a
  /// `MeshRenderer`'s `mesh` takes like a cooked one — and a `shape: mesh`
  /// `Collider`'s, for a static collider — drawn from the next frame and
  /// kept across scene loads until [destroyMesh]. Change it with
  /// [setMesh]. Throws [StateError] before a project or scene is loaded
  /// (the mesh lives in that asset store).
  MeshAsset createMesh(MeshData mesh) {
    final ref = _MadeMesh(this, _newAssetId());
    setMesh(ref, mesh);
    return ref;
  }

  /// Gives the mesh [ref] (one [createMesh] made) new geometry: every
  /// renderer and mesh collider using it follows from the next frame.
  void setMesh(AssetRef ref, MeshData mesh) => _setMesh(ref, mesh);

  /// Removes the mesh [ref]: its renderers draw it as missing, and a
  /// collider still naming it reports `meshUnavailable` at the next step
  /// (remove the collider first).
  void destroyMesh(AssetRef ref) => _setMesh(ref, null);

  /// What [createMesh] or [setMesh] last gave [ref] (null for a mesh this
  /// world did not make): the start of a change,
  /// `setMesh(ref, runtimeMesh(ref)!.copyWith(positions: …))`.
  MeshData? runtimeMesh(AssetRef ref) => _resourceView(_gameplay?._meshes, _runtimeMeshes, ref.asset);
  final Map<String, MeshData> _runtimeMeshes = {};

  void _setMesh(AssetRef ref, MeshData? mesh) => _withGameplay(
    () => _gameplay!.guard(() {
      final edit = _gameplay!;
      _checkRuntimeResource(ref, 'meshes');
      edit.native.setRuntimeMesh(ref.asset, mesh?.toPayload());
      edit._meshes[ref.asset] = mesh;
    }),
  );

  // ---------------------------------------------- runtime clips (ADR-0033 § 2)

  /// A clip made in code ([ClipBuilder]; Unity's `new AnimationClip()`):
  /// a new asset id that an `Animator`'s `clip`, a one-shot
  /// (`animation.playOneShot`) and `animation.play` take like an imported
  /// clip, kept across scene loads until [destroyClip]. Change it with
  /// [setClip]. Throws [StateError] before a project or scene is loaded
  /// (the clip lives in that asset store).
  AnimationClipAsset createClip(ClipData clip) {
    final ref = _MadeClip(this, _newAssetId());
    setClip(ref, clip);
    return ref;
  }

  /// Gives the clip [ref] (one [createClip] made) new keys: animators
  /// playing it follow from the next tick. Inside an edit or callback, creation,
  /// replacement and removal publish only on successful commit.
  void setClip(AssetRef ref, ClipData clip) => _setClip(ref, clip);

  /// Removes the clip [ref]; animators already playing it keep it.
  void destroyClip(AssetRef ref) => _setClip(ref, null);

  final Set<String> _runtimeClips = {};

  bool _hasRuntimeClip(String id) => _gameplay?._runtimeClips[id] ?? _runtimeClips.contains(id);

  void _setClip(AssetRef ref, ClipData? clip) => _withGameplay(() {
    final edit = _gameplay!;
    edit.guard(() {
      if (ref.bytes == null) throw ArgumentError.value(ref, 'ref', 'not a UUID');
      if (ref.sub?.isNotEmpty ?? false) {
        throw ArgumentError.value(ref, 'ref', 'a made clip is a whole asset, not a sub-asset of a model');
      }
      if (_cacheDir == null) {
        throw StateError('clips need a loaded project or scene — loadProject or loadScene first');
      }
      edit.native.setRuntimeClip(ref.asset, clip?.toPayload());
      edit._runtimeClips[ref.asset] = clip != null;
    });
  });

  /// The shared instance of the data asset [ref] (ADR-0027 §3):
  /// `world.data<Weapon>(Assets.data.sword)`. Made from the cooked
  /// document the first time and kept for the world's life, across scene
  /// loads — every reference to the asset shares it, and changes to it
  /// last for the run and are never saved. Its data-typed fields resolve
  /// the same way (a missing asset: null, with `FYN-B0004`). An [ArgumentError] for an
  /// asset the cache does not have or that is not a data asset or not a
  /// [T]; a [StateError] when no project is loaded or its type is not
  /// registered.
  /// Inside [edit], new instances share a staged cache, published only on commit.
  /// Abort invalidates those new instances; existing shared instances are reused.
  /// Invalid nested references abort the edit instead of producing warnings.
  /// Arbitrary writes to existing Dart data fields are outside rollback.
  T data<T extends DataAsset>(AssetRef ref) {
    final edit = _gameplay;
    if (edit != null) return edit.data<T>(ref);
    final warnings = <LoadDiagnostic>[];
    final DataAsset d;
    try {
      d = _data.load(ref, warnings);
    } finally {
      if (warnings.isNotEmpty) onLoadDiagnostics(warnings);
    }
    if (d is! T) {
      throw ArgumentError.value(ref, 'ref', 'a ${d.dataType?.id ?? d.runtimeType}, not a $T');
    }
    return d;
  }

  /// The data assets made so far (their shared instances), by asset id.
  Map<String, DataAsset> get loadedData => Map.unmodifiable({..._data.byId, ...?_gameplay?._dataAssets});

  /// A data-typed field's JSON (`{"asset": id}`, a list of them, or null)
  /// as the shared instance(s) of the data type [typeId] — what a load sets
  /// the field to; one that cannot be made is null (left out of a list)
  /// with `FYN-B0004` through [onLoadDiagnostics]. For an editor setting a
  /// script's data-typed field during Play.
  Object? resolveData(Object? json, String typeId, {String where = 'the field'}) {
    final edit = _gameplay;
    if (edit != null) return edit.resolveData(json, typeId);
    final warnings = <LoadDiagnostic>[];
    try {
      return _data.resolve(json, typeId, warnings, where: where);
    } finally {
      if (warnings.isNotEmpty) onLoadDiagnostics(warnings);
    }
  }

  /// An Inspector edit of the data asset [id] during Play (ADR-0027 §7):
  /// the instance this world shares takes the asset's saved [data] — every
  /// field it names, data-typed ones resolved as a load does, and a field
  /// it no longer names back to its default. False when the world has not
  /// made that asset (nothing uses it yet: the next use reads the cache).
  bool applyDataEdit(String id, Map<String, Object?> data) {
    _rejectDuringEdit('applyDataEdit');
    final d = _data.byId[id];
    final type = d?.dataType;
    if (d == null || type == null) return false;
    for (final f in type.fields) {
      final has = data.containsKey(f.name);
      if (f.holdsData) {
        final v = resolveData(data[f.name], f.dataType!, where: '${type.id}.${f.name}');
        f.set(d, v ?? (f.type == 'list' ? const <Object?>[] : null));
      } else if (has || f.defaultValue != null) {
        f.set(d, has ? data[f.name] : f.defaultValue);
      }
    }
    return true;
  }

  late final _DataAssets _data = _DataAssets(this);

  /// Looks a resolved object up by handle.
  GameObject? byEntity(EntityRef entity) {
    if (entity.isNull) return null;
    return _withGameplay(() {
      final edit = _gameplay!;
      final ref = NativeGameplayRef.live(entity);
      return edit.native.contains(ref) ? edit.object(ref) : null;
    });
  }

  // ---------------------------------------------------------------- frame

  /// Starts a host frame of [dtSeconds] real seconds (scaled by
  /// [timeScale] for the clock); run [step] `plan.steps` times, then
  /// presentation writes, then extract/render.
  FramePlan beginFrame(double dtSeconds) {
    _rejectDuringEdit('beginFrame');
    if (Gizmos.compiled) gizmos?.clear();
    final plan = _native.beginFrame(dtSeconds, _timeScale);
    _frameCount = plan.frame;
    _frameDt = plan.gameplayDelta;
    _unscaledDt = plan.realDelta;
    _gameplayElapsed = plan.gameplayElapsed;
    _realElapsed = plan.realElapsed;
    _pendingFrameSteps = plan.steps;
    if (plan.missedDeadline) _collectDiagnostics('begin_frame');
    return plan;
  }

  /// Hands this frame's raw control events to the world (§5.1): once per
  /// frame after [beginFrame], before the ticks. Held state is immediate;
  /// `pressed`/`released` edges wait for the next tick. [frame] does this
  /// for its `input` argument; hosts with their own loop call it directly.
  void captureInput(List<InputEvent> events) {
    _rejectDuringEdit('captureInput');
    input.capture(events);
    // A frame without events after a frame without events changes nothing
    // (no relative control to zero, no frame edge to clear): skip the call.
    if (events.isEmpty && !_inputDirty) return;
    _inputDirty = events.isNotEmpty;
    _inputEvents.clear();
    for (final e in events) {
      _inputEvents.add(e.toRecord());
    }
    // finalize() first: it may grow the buffer (the string tail).
    final length = _inputEvents.finalize();
    final s = _native.captureInputBytes(_inputEvents.backingStore, length);
    _inputEvents.clear();
    _checkFatal(s, 'captureInput');
  }

  /// One fixed tick: executes committed effects and native systems, refreshes
  /// the completed snapshot, and reports native diagnostics.
  StepResult step() {
    _rejectDuringEdit('step');
    // Read since the last drain: in this tick's snapshot already.
    _flushSubscriptions();
    final r = _native.step();
    _checkFatal(r.status, 'step');
    if (_pendingFrameSteps > 0) _pendingFrameSteps--;
    if (++_steps % _observeTicks == 0) _sweepObserved();
    _snapshot = _native.snapshot;
    // entity → record index. Overwrite in place (no growth in steady state,
    // M1 gate); rebuild only when the subscribed set changed size, which is
    // the only way a stale key can remain.
    final d = _snapshot.records;
    final n = _snapshot.recordCount;
    if (_snapshotIndex.length != n) _snapshotIndex.clear();
    for (var i = 0; i < n; i++) {
      _snapshotIndex[readEntity(d, i * gen.SnapshotTransformRecord.wireSize)] = i;
    }
    if (_snapshotIndex.length != n) {
      // Same count, different members: rebuild once.
      _snapshotIndex.clear();
      for (var i = 0; i < n; i++) {
        _snapshotIndex[readEntity(d, i * gen.SnapshotTransformRecord.wireSize)] = i;
      }
    }
    _collectDiagnostics('step');
    return r;
  }

  /// Pull-only event drain; resolves spawn/destroy bookkeeping on the way
  /// and sends the transform subscriptions of handles attached since.
  List<EngineEvent> drainEvents() {
    _rejectDuringEdit('drainEvents');
    final buffer = _native.drainEvents();
    if (buffer.recordCount == 0) {
      _flushSubscriptions();
      return const [];
    }
    final events = decodeEvents(buffer).toList();
    for (final e in events) {
      switch (e) {
        case EntityDestroyed(:final entity):
          final go = _byEntity.remove(entity);
          if (go != null) {
            go._alive = false;
            _behaviours.objectDied();
          }
        default:
          break;
      }
    }
    _flushSubscriptions();
    return events;
  }

  void _flushSubscriptions() {
    if (_toSubscribe.isEmpty) return;
    _native.subscribeTransforms(_toSubscribe);
    _toSubscribe.clear();
  }

  /// The host loop (§5.1) for tests, headless runs and hosts that need no
  /// custom phases: begin the frame; per planned tick run every behaviour's
  /// `fixedUpdate` (and [fixedUpdate]) with the non-physics events drained
  /// since the last tick, `step`, drain the tick's events and run
  /// `afterPhysics` with its contacts for behaviours that opted in; then
  /// `update`/`lateUpdate` (and [update]).
  /// [fixedUpdate] runs before the behaviours, [update] after them. Every
  /// non-physics event reaches exactly one `fixedUpdate`. [input] is the
  /// frame's raw control events ([captureInput]): each tick's callbacks
  /// see the edges that tick consumes (a catch-up tick sees held state
  /// only), `afterPhysics` the same tick's, `update`/`lateUpdate` the
  /// frame's raw edges.
  FramePlan frame(
    double dtSeconds, {
    List<InputEvent> input = const [],
    void Function(int tick)? fixedUpdate,
    void Function()? update,
  }) {
    _rejectDuringEdit('frame');
    if (_native.isDisposed) {
      throw StateError('frame: the world was disposed (World.dispose); it can no longer be used');
    }
    if (_native.isPoisoned) {
      throw _native.failure('frame', FynkelStatus.FYNKEL_STATUS_POISONED);
    }
    final t = frameTimes.._begin();
    _callbackEntryBlocked = false;
    _deferredErrors.clear(); // a frame that threw something else left them
    scenes._perform();
    t._native.start();
    final plan = beginFrame(dtSeconds);
    captureInput(input);
    _carry(drainEvents());
    t._native.stop();
    final stepsBefore = _steps;
    _inFixedPhase = true;
    try {
      _ticks(plan, t, fixedUpdate);
    } finally {
      _inFixedPhase = false;
    }
    if (isDisposed) {
      _pendingFrameSteps = 0;
      t._end(_steps - stepsBefore);
      _throwDeferred();
      return plan;
    }
    this.input.refresh(frameEdges: true);
    t._behaviours.start();
    _inUpdatePhase = true;
    try {
      _behaviours.update(_frameDt);
      if (!isDisposed && update != null) _deferring(() => _callbackGameplay(update));
    } finally {
      _inUpdatePhase = false;
    }
    t._behaviours.stop();
    t._end(plan.steps);
    _throwDeferred();
    return plan;
  }

  /// Runs one complete fixed sequence without advancing real/gameplay
  /// clocks or variable callbacks. Used by editor Step even at timeScale 0.
  FramePlan stepFrame({List<InputEvent> input = const [], void Function(int tick)? fixedUpdate}) {
    _rejectDuringEdit('stepFrame');
    if (_pendingFrameSteps != 0 || _inFixedPhase || _inUpdatePhase) {
      throw StateError('stepFrame requires a completed frame boundary');
    }
    if (_native.isDisposed) {
      throw StateError('stepFrame: the world was disposed');
    }
    if (_native.isPoisoned) {
      throw _native.failure('stepFrame', FynkelStatus.FYNKEL_STATUS_POISONED);
    }
    final t = frameTimes.._begin();
    _callbackEntryBlocked = false;
    _deferredErrors.clear();
    scenes._perform();
    if (Gizmos.compiled) gizmos?.clear();
    t._native.start();
    captureInput(input);
    _carry(drainEvents());
    t._native.stop();
    final plan = FramePlan(
      1,
      0,
      false,
      alpha,
      frame: _frameCount,
      realElapsed: _realElapsed,
      gameplayElapsed: _gameplayElapsed,
    );
    final stepsBefore = _steps;
    _inFixedPhase = true;
    try {
      _ticks(plan, t, fixedUpdate);
    } finally {
      _inFixedPhase = false;
    }
    if (isDisposed) {
      _pendingFrameSteps = 0;
      t._end(_steps - stepsBefore);
      _throwDeferred();
      return plan;
    }
    _checkFatal(_native.snapInterpolation(), 'snapInterpolation');
    t._end(1);
    _throwDeferred();
    return plan;
  }

  /// What this [frame]'s steps, flush and host callbacks threw — the
  /// debug-mode [FynkelError]s (ADR-0014), a [FynkelBridgeError], or the
  /// host's own error — held until the frame is done: thrown from inside
  /// a tick, one lost the rest of the frame — that tick's contacts and
  /// `afterPhysics`, the ticks after it, `update`, `lateUpdate` and the
  /// frame completion (foundation 1.5, 1.6). Unity logs and the frame
  /// goes on; here the frame goes on and then throws.
  final List<(Object, StackTrace)> _deferredErrors = [];
  bool _callbackEntryBlocked = false;
  bool _callbackPreflight = false;

  void _deferring(void Function() f) {
    try {
      f();
    } catch (e, st) {
      if (e is _GameplayCallbackEntrySkipped) return;
      _deferredErrors.add((e, st));
    }
  }

  /// Throws this frame's deferred errors: [FynkelError]s alone as one
  /// (every diagnostic, the first one's context and stack); otherwise the
  /// first error, with the others printed so none is lost silently.
  void _throwDeferred() {
    _callbackEntryBlocked = false;
    if (_deferredErrors.isEmpty) return;
    final errors = List.of(_deferredErrors);
    _deferredErrors.clear();
    final (first, stack) = errors.first;
    if (errors.length > 1 && errors.every((e) => e.$1 is FynkelError)) {
      final all = [for (final (e, _) in errors) ...(e as FynkelError).diagnostics];
      Error.throwWithStackTrace(FynkelError(all, context: (first as FynkelError).context), stack);
    }
    for (final (e, st) in errors.skip(1)) {
      print('[frame] also thrown this frame: $e\n$st');
    }
    Error.throwWithStackTrace(first, stack);
  }

  /// [frame]'s fixed phase: `plan.steps` ticks.
  void _ticks(FramePlan plan, FrameTimes t, void Function(int tick)? fixedUpdate) {
    for (var i = 0; i < plan.steps && !isDisposed; i++) {
      _checkFatal(_native.prepareGameplayTick(), 'prepareGameplayTick');
      try {
        input.refresh();
        t._behaviours.start();
        if (fixedUpdate != null) _deferring(() => _callbackGameplay(() => fixedUpdate(tick)));
        if (!isDisposed) _behaviours.fixedUpdate(tick, _takeCarried());
        t._behaviours.stop();
        if (isDisposed) return;
        final stepped = tick;
        t._native.start();
        _deferring(step);
        final events = drainEvents();
        t._native.stop();
        t._behaviours.start();
        _behaviours.afterPhysics(
          stepped,
          _contacts(events),
          _zoneEvents(events),
          _animationEvents(events),
          _markerEvents(events),
          _playbackEvents(events),
          _stateEvents(events),
        );
        t._behaviours.stop();
        if (isDisposed) return;
        _carry(events);
      } finally {
        if (!isDisposed) _checkFatal(_native.cancelGameplayTick(), 'cancelGameplayTick');
      }
    }
  }

  /// Where the last [frame] spent its time (M3-16, blueprint §15.4). One
  /// object, overwritten every frame — read it after [frame], allocation
  /// free.
  final FrameTimes frameTimes = FrameTimes();

  /// The subscription budget (blueprint C5): a ceiling in bytes on a
  /// tick's snapshot. The first tick over it reports
  /// [DiagnosticCode.snapshotBudget] (`FYN-R0013`) — once, until the
  /// snapshot is back under; the snapshot is still delivered whole, so a
  /// host learns it subscribed more than it meant to pay for. 0 (the
  /// default) means no ceiling. `stats().snapshotBytes` is what the
  /// budget is measured against.
  int get snapshotBudget => _snapshotBudget;
  set snapshotBudget(int bytes) {
    _rejectDuringEdit('snapshotBudget');
    if (bytes < 0) throw ArgumentError.value(bytes, 'bytes', 'must be >= 0');
    _snapshotBudget = bytes;
    _native.setSnapshotBudget(bytes);
  }

  int _snapshotBudget = 0;

  /// Non-physics events waiting for the next `fixedUpdate`.
  List<EngineEvent> _carried = [];

  void _carry(List<EngineEvent> events) {
    for (final e in events) {
      if (e is! Contact) _carried.add(e);
    }
  }

  List<EngineEvent> _takeCarried() {
    if (_carried.isEmpty) return const [];
    final out = _carried;
    _carried = [];
    return out;
  }

  static List<Contact> _contacts(List<EngineEvent> events) {
    if (events.isEmpty) return const [];
    return [
      for (final e in events)
        if (e is Contact) e,
    ];
  }

  static List<ZoneEvent> _zoneEvents(List<EngineEvent> events) {
    if (events.isEmpty) return const [];
    return [
      for (final e in events)
        if (e is ZoneEvent) e,
    ];
  }

  static List<AnimationEnded> _animationEvents(List<EngineEvent> events) {
    if (events.isEmpty) return const [];
    return [
      for (final e in events)
        if (e is AnimationEnded) e,
    ];
  }

  static List<AnimationMarker> _markerEvents(List<EngineEvent> events) {
    if (events.isEmpty) return const [];
    return [
      for (final e in events)
        if (e is AnimationMarker) e,
    ];
  }

  static List<AnimatorStateEvent> _stateEvents(List<EngineEvent> events) {
    if (events.isEmpty) return const [];
    return [
      for (final e in events)
        if (e is AnimatorStateEvent) e,
    ];
  }

  static List<PlaybackEnded> _playbackEvents(List<EngineEvent> events) {
    if (events.isEmpty) return const [];
    return [
      for (final e in events)
        if (e is PlaybackEnded) e,
    ];
  }

  /// A handle for an entity that already exists (loaded from a scene, or
  /// found by a query); the same handle comes back for the same entity.
  /// Its transform rides the snapshot once it is read (the first read
  /// asks the world).
  GameObject attach(EntityRef entity, {String name = '', Pose initial = identityTransform}) {
    final edit = _gameplay;
    if (edit != null) return edit.object(NativeGameplayRef.live(entity));
    if (!_native.contains(entity)) {
      throw StateError('attach: foreign, stale or invalid entity $entity');
    }
    final existing = _byEntity[entity];
    if (existing != null) return existing;
    final go = GameObject._(this, initial, name.isEmpty ? (nameOfEntity(entity) ?? '') : name);
    go._entity = entity;
    _byEntity[entity] = go;
    return go;
  }

  void _checkFatal(FynkelStatus s, String op) {
    switch (s) {
      case FynkelStatus.FYNKEL_STATUS_OK:
      case FynkelStatus.FYNKEL_STATUS_COMMANDS_REJECTED:
        return;
      default:
        throw _native.failure(op, s);
    }
  }

  void _collectDiagnostics(String context) {
    final b = _native.diagnostics;
    if (b.recordCount == 0) return;
    final list = b
        .fixed(gen.DiagnosticRecord.wireSize, gen.DiagnosticRecord.decode)
        .map(FynkelDiagnostic.fromRecord)
        .toList(growable: false);
    if (EditorChannel.enabled) _reportToEditor(list, context);
    if (throwOnDiagnostics && list.any((d) => !d.isInformational)) {
      throw FynkelError(list, context: context);
    }
    onDiagnostics(list);
  }

  /// The runtime diagnostics of one step/submit as §14.2 records: the
  /// wire code's name (`stale_entity`) is the record's `code`, the entity
  /// the authored one when the last load knew an authoring id for it — so
  /// a Console row can select the entity in the scene it came from.
  void _reportToEditor(List<FynkelDiagnostic> list, String context) {
    for (final d in list) {
      final authored = _authoredEntities[d.entity];
      EditorChannel.diagnostic(
        code: d.code.name,
        severity: d.isInformational ? 'info' : 'error',
        message: d.message,
        tick: tick,
        source: context,
        entity: d.entity.isNull ? null : d.entity,
        id: authored,
        document: authored == null ? null : _authoredDocument,
      );
    }
  }

  /// The id an editor shows for the entity with runtime handle [handle]
  /// (its live table's), when one is attached: what a script's object
  /// field reads back as ([referenceJson]). Null otherwise.
  String? Function(EntityRef handle)? liveIdOf;

  /// Authoring ids of the entities the last loads spawned with a behaviour
  /// (the only runtime → authored mapping the SDK has), and the scene
  /// document they came from. Filled only while the editor channel is on.
  final Map<EntityRef, String> _authoredEntities = {};
  String? _authoredDocument;

  // ---------------------------------------------------------------- reads

  gen.SnapshotTransformRecord? _snapshotOf(EntityRef entity) {
    final i = _snapshotIndex[entity];
    if (i == null) return null;
    return gen.SnapshotTransformRecord.decode(
      _snapshot.records,
      i * gen.SnapshotTransformRecord.wireSize,
      _snapshot.tail,
    );
  }

  /// Explicit queries (one FFI call each; not for hot paths).
  List<EntityRef> childrenOf(GameObject go) => _withGameplay(() => go.childEntities);
  EntityRef? parentOf(GameObject go) => _withGameplay(() => go.parentEntity);
  String? nameOf(GameObject go) => _withGameplay(() {
    final edit = _gameplay!;
    return edit.native.name(edit.ref(go));
  });

  String? nameOfEntity(EntityRef entity) {
    final edit = _gameplay;
    if (edit != null) return edit.guard(() => edit.native.name(NativeGameplayRef.live(entity)));
    final b = _native.query(QueryKind.name, entity: entity);
    if (b.recordCount == 0) return null;
    return gen.Name.decode(b.records, 0, b.tail).value;
  }

  List<EntityRef> allEntities() => _native.query(QueryKind.allEntities).entities();

  /// The last step's per-system wall time (`FYNKEL_QUERY_SYSTEMS`, M3-10):
  /// one [SystemTime] per [gen.SystemKind] in tick order, `step` first —
  /// what a HUD shows next to the frame time. One query; not free at
  /// every frame of a shipped game, fine behind a stats toggle.
  List<SystemTime> systemTimes() {
    final b = _native.query(QueryKind.systems);
    return [
      for (var i = 0; i < b.recordCount; i++)
        SystemTime.fromRecord(gen.SystemTimeRecord.decode(b.records, i * gen.SystemTimeRecord.wireSize, b.tail)),
    ];
  }

  // ---------------------------------------------------------------- find (M3-08b)

  /// Finds the first currently visible object with [name], in entity order.
  GameObject? find(String name) => _withGameplay(() {
    final edit = _gameplay!;
    final ref = edit.native.find(name);
    return ref == null ? null : edit.object(ref);
  });

  /// Every currently visible object named [name], in entity order.
  List<GameObject> findAll(String name) =>
      _withGameplay(() => _gameplay!.objects().where((go) => go.name == name).toList());

  /// The first currently visible object carrying the project tag [tag].
  GameObject? findWithTag(String tag) => _withGameplay(() {
    final bit = _tagBit(tag);
    return _gameplay!.objects().where((go) => go._tagBitsNow & bit != 0).firstOrNull;
  });

  /// Every currently visible object carrying the project tag [tag].
  List<GameObject> findAllWithTag(String tag) => _withGameplay(() {
    final bit = _tagBit(tag);
    return _gameplay!.objects().where((go) => go._tagBitsNow & bit != 0).toList();
  });

  /// The first [T] in the world (Unity's `FindObjectOfType`): a behaviour
  /// type in callback order, or a native component type (`Camera`) in
  /// entity order — its value; [findObjectWith] gives its object. Objects
  /// inactive in the hierarchy are skipped unless [includeInactive].
  /// Walks every behaviour or entity: not for every frame.
  T? findObjectOfType<T extends Object>({bool includeInactive = false}) =>
      _findOfType<T>(includeInactive, first: true).firstOrNull;

  /// Every [T] in the world (Unity's `FindObjectsOfType`); see
  /// [findObjectOfType].
  List<T> findObjectsOfType<T extends Object>({bool includeInactive = false}) =>
      _findOfType<T>(includeInactive, first: false);

  /// The first object carrying the native component [T], entity order
  /// (objects inactive in the hierarchy skipped unless [includeInactive]).
  GameObject? findObjectWith<T extends WireRecord>({bool includeInactive = false}) =>
      _objectsWith(GameObject._codecOf(T).id, includeInactive, first: true).firstOrNull;

  /// Every object carrying the native component [T]; see [findObjectWith].
  List<GameObject> findObjectsWith<T extends WireRecord>({bool includeInactive = false}) =>
      _objectsWith(GameObject._codecOf(T).id, includeInactive, first: false);

  List<T> _findOfType<T extends Object>(bool includeInactive, {required bool first}) {
    if (_isBehaviourType<T>()) {
      final out = <T>[];
      for (final b in (_gameplay == null ? _behaviours._all : behaviours)) {
        final go = b.gameObject;
        if (b is! T || b._destroyed || b._destroyRequested || b._retired || !go.isAlive) {
          continue;
        }
        if (!includeInactive && !go._activeForFind) continue;
        out.add(b as T);
        if (first) break;
      }
      return out;
    }
    final id = GameObject._codecOf(T).id;
    return [for (final go in _objectsWith(id, includeInactive, first: first)) go._lookup(id) as T];
  }

  /// Objects with native component [id]: the world's entities in order
  /// (an entity no handle names yet is asked directly, without attaching
  /// it unless it matches), then this tick's pending spawns.
  List<GameObject> _objectsWith(int id, bool includeInactive, {required bool first}) => _withGameplay(() {
    final result = _gameplay!.objects(components: [id], includeInactive: includeInactive);
    return first ? result.take(1).toList() : result;
  });

  /// Whether [T] is a [Behaviour] type (else it must be a native
  /// component type).
  static bool _isBehaviourType<T>() => <T>[] is List<Behaviour>;

  /// The bit of the project tag [tag]; throws naming the project's list.
  int _tagBit(String tag) {
    final bits = layersAndTags.tagBits([tag]);
    if (bits == null) {
      _gameplay?.reject('unknown tag "$tag"');
      throw ArgumentError('unknown tag "$tag"; the project declares ${layersAndTags.tags}');
    }
    return bits;
  }

  /// The index of the project layer [layer]; throws naming the project's
  /// list.
  int _layerIndex(String layer) {
    final i = layersAndTags.layer(layer);
    if (i == null) {
      _gameplay?.reject('unknown layer "$layer"');
      throw ArgumentError('unknown layer "$layer"; the project declares ${layersAndTags.layers}');
    }
    return i;
  }

  /// Every native component of [entity], in component id order. Outside an
  /// edit this explicitly queries committed native data; inside an edit it
  /// reads that edit's logical data. A destroyed entity returns an empty list.
  List<WireRecord> componentsOf(EntityRef entity) {
    final edit = _gameplay;
    if (edit != null) {
      final ref = NativeGameplayRef.live(entity);
      return edit.native.contains(ref) ? edit.object(ref).components : [];
    }
    final out = <WireRecord>[];
    _decodeComponents(_native.query(QueryKind.components, entity: entity), (_, value) => out.add(value));
    return out;
  }

  /// Walks a `FYNKEL_QUERY_COMPONENTS` response: a `ComponentHeader` then
  /// the body per component; a component this build has no codec for is
  /// skipped by its size.
  void _decodeComponents(WireBuffer b, void Function(int id, WireRecord value) sink) {
    final outer = _decoding;
    _decoding = this;
    try {
      _decodeComponentsIn(b, sink);
    } finally {
      _decoding = outer;
    }
  }

  /// The world whose components are being decoded: an object reference
  /// field ([objectRefOf]) names an object of this world.
  static World? _decoding;

  void _decodeComponentsIn(WireBuffer b, void Function(int id, WireRecord value) sink) {
    final d = b.records;
    var off = 0;
    for (var i = 0; i < b.recordCount; i++) {
      final id = d.getUint32(off, Endian.little);
      final size = d.getUint32(off + 4, Endian.little);
      off += gen.ComponentHeader.wireSize;
      final codec = gen.componentCodecs[id];
      if (codec != null && codec.wireSize <= size) {
        sink(id, codec.decode(d, off, b.tail));
      }
      off += size;
    }
  }

  // ---------------------------------------------------------------- physics (M3-08)

  /// The mask of the named project layers (the project's `layers`, for a
  /// collider's `mask` or a query's); throws on a name the project does not
  /// declare.
  int layerMask(Iterable<String> names) {
    final m = layersAndTags.mask(names);
    if (m == null) {
      throw ArgumentError('unknown layer in $names; the project declares ${layersAndTags.layers}');
    }
    return m;
  }

  /// The nearest collider along a ray, or null. Spatial queries read the
  /// world as the last step left it, plus the colliders made since
  /// (`instantiate`, `clone`), as Unity's are at once (foundation 2.7; one
  /// FFI call each; batch with [physicsQuery] when asking many at once).
  /// Inside [World.edit], reads the committed physics scene as it stood on entry;
  /// staged moves, collider edits, spawns and removals do not change those hits.
  /// The query does not refresh physics during an edit. Outside edits, the usual
  /// lazy refresh of committed colliders still applies.
  /// As in Unity, triggers are hit unless [hitSensors] is false, and a ray
  /// or cast does not detect a collider it starts in (foundation 2.8).
  PhysicsHit? raycast(
    Vec3 origin,
    Vec3 direction, {
    double maxDistance = 1000,
    int mask = 0xffffffff,
    GameObject? exclude,
    bool hitSensors = true,
  }) => raycastAll(
    origin,
    direction,
    maxDistance: maxDistance,
    mask: mask,
    exclude: exclude,
    hitSensors: hitSensors,
    maxHits: 1,
  ).firstOrNull;

  /// Every collider along a ray, nearest first, at most [maxHits].
  List<PhysicsHit> raycastAll(
    Vec3 origin,
    Vec3 direction, {
    double maxDistance = 1000,
    int mask = 0xffffffff,
    GameObject? exclude,
    bool hitSensors = true,
    int maxHits = 16,
  }) => _readSimulation(() {
    modules.require(Module.physics, 'raycast');
    _physicsQueries.clear();
    _physicsQueries.add(
      gen.PhysicsQueryRecord(
        id: _nextQueryId++,
        kind: gen.PhysicsQueryKind.raycast,
        shape: gen.ColliderShape.box,
        size: Vec3.zero,
        origin: origin,
        rotation: Quat.identity,
        direction: direction,
        maxDistance: maxDistance,
        mask: mask,
        exclude: _physicsExclude(exclude),
        maxHits: maxHits,
        hitSensors: hitSensors,
      ),
    );
    return _runPhysicsQueries();
  });

  /// Sweeps a `box`/`sphere`/`capsule` of [size] (as `Collider.size`
  /// defines it) from [origin] along [direction]: the first collider it
  /// would touch, or null.
  PhysicsHit? shapeCast(
    gen.ColliderShape shape,
    Vec3 size,
    Vec3 origin,
    Vec3 direction, {
    Quat rotation = Quat.identity,
    double maxDistance = 1000,
    int mask = 0xffffffff,
    GameObject? exclude,
    bool hitSensors = true,
  }) => _readSimulation(() {
    modules.require(Module.physics, 'shapeCast');
    _physicsQueries.clear();
    _physicsQueries.add(
      gen.PhysicsQueryRecord(
        id: _nextQueryId++,
        kind: gen.PhysicsQueryKind.shapeCast,
        shape: shape,
        size: size,
        origin: origin,
        rotation: rotation,
        direction: direction,
        maxDistance: maxDistance,
        mask: mask,
        exclude: _physicsExclude(exclude),
        maxHits: 1,
        hitSensors: hitSensors,
      ),
    );
    return _runPhysicsQueries().firstOrNull;
  });

  /// Every collider intersecting a `box`/`sphere`/`capsule` of [size] at
  /// [origin] (distance 0, normal zero).
  List<PhysicsHit> overlap(
    gen.ColliderShape shape,
    Vec3 size,
    Vec3 origin, {
    Quat rotation = Quat.identity,
    int mask = 0xffffffff,
    GameObject? exclude,
    bool hitSensors = true,
    int maxHits = 16,
  }) => _readSimulation(() {
    modules.require(Module.physics, 'overlap');
    _physicsQueries.clear();
    _physicsQueries.add(
      gen.PhysicsQueryRecord(
        id: _nextQueryId++,
        kind: gen.PhysicsQueryKind.overlap,
        shape: shape,
        size: size,
        origin: origin,
        rotation: rotation,
        direction: Vec3.zero,
        maxDistance: 0,
        mask: mask,
        exclude: _physicsExclude(exclude),
        maxHits: maxHits,
        hitSensors: hitSensors,
      ),
    );
    return _runPhysicsQueries();
  });

  /// Several queries in one FFI call: the hits of every record, in query
  /// order (`PhysicsHitRecord.query` echoes each record's `id`).
  List<gen.PhysicsHitRecord> physicsQuery(List<gen.PhysicsQueryRecord> queries) => _readSimulation(() {
    modules.require(Module.physics, 'physicsQuery');
    _physicsQueries.clear();
    queries.forEach(_physicsQueries.add);
    final length = _physicsQueries.finalize();
    final b = _native.physicsQuery(_physicsQueries.backingStore, length);
    _physicsQueries.clear();
    return b.fixed(gen.PhysicsHitRecord.wireSize, gen.PhysicsHitRecord.decode).toList(growable: false);
  });

  T _readSimulation<T>(T Function() read) {
    final edit = _gameplay;
    return edit == null ? read() : edit.guard(read);
  }

  EntityRef? _physicsExclude(GameObject? object) {
    if (object == null) return null;
    final edit = _gameplay;
    return edit == null ? object._entity : edit.ref(object).live;
  }

  List<PhysicsHit> _runPhysicsQueries() {
    final length = _physicsQueries.finalize();
    final b = _native.physicsQuery(_physicsQueries.backingStore, length);
    _physicsQueries.clear();
    if (b.recordCount == 0) return const [];
    return [
      for (final r in b.fixed(gen.PhysicsHitRecord.wireSize, gen.PhysicsHitRecord.decode)) PhysicsHit.fromRecord(r),
    ];
  }

  // ---------------------------------------------------------------- documents

  /// Where `fynkel cook` writes a project's cache: `<dir>/.fynkel/cache`.
  static String cacheDirOf(String projectDir) => '$projectDir/.fynkel/cache';

  /// Loads the startup scene of the project at [dir] from its cooked cache
  /// ([cacheDir], default [cacheDirOf]). The runtime reads the cache only:
  /// `fynkel cook` (or `fynkel run`, `fynkel test`, which cook first) must
  /// have run for this build, or the load fails with `FYN-R0001`/`R0002`.
  /// Scene entities and validated components publish together; a rejected
  /// activation preserves the previous scene and project-owned roots. The scene's
  /// assets stay resident in the world's store — a viewport rendering this
  /// world draws from it — until the world is disposed. Throws
  /// [FynkelLoadError] when the cache or the scene has errors. A project
  /// whose `fynkel.project.json` names no `startupScene` opens with no
  /// scene loaded — its input, layers, tags, gravity and assets ready, the
  /// world left as it is ([LoadReport.sceneId] null): a game that builds
  /// its world from code (ADR-0026).
  LoadReport loadProject(String dir, {String? cacheDir}) {
    _rejectDuringEdit('loadProject');
    final cache = cacheDir ?? cacheDirOf(dir);
    final report = _load(_native.loadProject(cache), 'loadProject').withProjectDir(dir);
    _cacheDir = cache;
    scenes._projectLoaded(report);
    return report;
  }

  /// Loads the cooked scene with asset id [sceneId] (UUID text, or an
  /// [AssetRef]) from the cache at [cacheDir] (default: the cache of the
  /// last load, [World.cacheDir]); see [loadProject]. [LoadMode.replace]
  /// (the default) first unloads everything the world holds — every
  /// loaded scene, last first, and every runtime spawn — so nothing
  /// survives; [LoadMode.additive] keeps the world and refuses a scene
  /// that is loaded already (`FYN-R0011`). The unloads' `EntityDestroyed`
  /// and `SceneUnloaded` events and the new scene's `SceneReady` arrive
  /// on the next drain, in that order; handles of destroyed objects go
  /// dead then. Throws [FynkelLoadError] when the scene is not in the
  /// cache (`FYN-R0003`) or does not load; [StateError] without a cache.
  LoadReport loadScene(Object sceneId, {String? cacheDir, LoadMode mode = LoadMode.replace}) {
    _rejectDuringEdit('loadScene');
    final id = _assetId(sceneId, 'sceneId');
    final cache = cacheDir ?? _cacheDir;
    if (cache == null) {
      throw StateError('loadScene($id): no cache directory — pass cacheDir, or loadProject first');
    }
    final flags = mode == LoadMode.additive ? NativeWorld.loadAdditive : 0;
    final report = _load(_native.loadSceneWith(cache, id, flags), 'loadScene');
    _cacheDir = cache;
    scenes._loaded(report, mode);
    return report;
  }

  /// Keeps [target] — a root [GameObject], or a component's object — and
  /// everything under it across scene loads (Unity's `DontDestroyOnLoad`,
  /// ADR-0037): a replace load leaves it, and so does unloading the scene
  /// it came from; its scripts stay as they are, and what it draws stays
  /// resident. It takes effect at once, so a script can call it and then
  /// `scenes.transition` in the same callback; an object not spawned yet
  /// is kept the moment the tick makes it. Children made under it later
  /// go with it; given a parent, it stops being kept. Only a root can be
  /// kept: on a child it does nothing and logs a warning, as Unity does.
  /// [persistentObjects] lists what is kept; `destroy` ends it.
  void dontDestroyOnLoad(FynkelObject target) {
    final go = switch (target) {
      GameObject g => g,
      Component c => c.gameObject,
      _ => throw ArgumentError.value(target, 'target', 'a GameObject or a component'),
    };
    _withGameplay(() {
      final edit = _gameplay!;
      if (!edit.native.persist(edit.ref(go))) {
        Debug.logWarning('dontDestroyOnLoad only works for root objects: "${go.name}" has a parent');
      }
    });
  }

  /// The project's persistent object named [name] (ADR-0039, Godot's
  /// autoloads): made from the prefab `fynkel.project.json` lists under
  /// `persistentObjects` when the project loaded, before the startup
  /// scene, and kept across every scene load. Null when no entry has that
  /// name or the object was destroyed; a destroyed one is made again only
  /// by the next [loadProject]. Its scripts are reached as on any object:
  /// `world.persistentObject('Music')?.behaviour<MusicPlayer>()`.
  GameObject? persistentObject(String name) {
    final go = _projectPersistent[name];
    return go != null && go.isAlive ? go : null;
  }

  /// The roots [dontDestroyOnLoad] keeps, in the order they were marked:
  /// alive and still roots, the project's persistent objects among them.
  List<GameObject> get persistentObjects => _withGameplay(() {
    final edit = _gameplay!;
    return List.unmodifiable(edit.native.persistentRoots().map(edit.object));
  });

  /// Unloads the loaded scene [sceneId] (M3-12): its entities that are
  /// still alive are destroyed (their handles go dead on the next drain,
  /// after `EntityDestroyed` events and one `SceneUnloaded`), its assets
  /// released. Runtime spawns — [instantiate]d prefabs, [spawn]ed
  /// objects — are not the scene's and stay. Throws [FynkelUnloadError]
  /// when the world does not hold the scene (`FYN-R0010`).
  UnloadReport unloadScene(Object sceneId) {
    _rejectDuringEdit('unloadScene');
    final id = _assetId(sceneId, 'sceneId');
    final (status, json) = _native.unloadScene(id);
    final report = UnloadReport.fromJson(json);
    switch (status) {
      case FynkelStatus.FYNKEL_STATUS_OK:
        _loadedScenes = report.scenes;
        scenes._unloaded(report);
        return report;
      case FynkelStatus.FYNKEL_STATUS_LOAD_FAILED:
        throw FynkelUnloadError(report);
      default:
        throw _native.failure('unloadScene', status);
    }
  }

  /// The scenes the world holds, load order (from the last load or
  /// unload report; empty before any).
  List<LoadedScene> get loadedScenes => _loadedScenes;
  List<LoadedScene> _loadedScenes = const [];

  /// Stages user bindings (`action → bindings`) over the loaded project's
  /// actions. An empty map restores project defaults. The returned report
  /// describes this request's staged configuration; commit publishes it and
  /// abort discards it. Outside a callback or [World.edit], opens an edit.
  ///
  /// Unknown actions bind nothing and invalid binding syntax appears in
  /// [LoadReport.bindingErrors] without rejecting the edit. A missing project
  /// throws [StateError] and rejects the current edit. Input reads keep their
  /// existing phase sample until the next normal input refresh.
  LoadReport bindActions(Map<String, List<String>> rebinds) {
    final edit = _gameplay;
    if (edit != null) return edit.bindActions(rebinds);
    return this.edit(() => _gameplay!.bindActions(rebinds));
  }

  static String _assetId(Object id, String what) => switch (id) {
    AssetRef r => r.asset,
    String s => s,
    _ => throw ArgumentError.value(id, what, 'an asset id (UUID text) or an AssetRef'),
  };

  LoadReport _load((FynkelStatus, Map<String, Object?>) r, String op) {
    final (status, json) = r;
    var report = LoadReport.fromJson(json);
    switch (status) {
      case FynkelStatus.FYNKEL_STATUS_OK:
        input.invalidate();
        if (op == 'loadProject') {
          physics = report.physics;
          layersAndTags = report.layersAndTags;
          report = _attachPersistentObjects(report);
        }
        if (op != 'bindActions') _loadedScenes = report.scenes;
        if (report.modules case final m?) modules = m;
        report = _attachAuthoredBehaviours(report);
        if (EditorChannel.enabled) _loadedForEditor(report, op);
        if (report.warnings.isNotEmpty) {
          onLoadDiagnostics(report.warnings.toList(growable: false));
        }
        return report;
      case FynkelStatus.FYNKEL_STATUS_LOAD_FAILED:
        if (EditorChannel.enabled) _loadedForEditor(report, op);
        throw FynkelLoadError(report, context: op);
      default:
        throw _native.failure(op, status);
    }
  }

  /// One load as the editor sees it: every document diagnostic as its
  /// §14.2 record (the file, pointer, line and suggestion the loader
  /// reported, so the Console opens the document at the spot) and one
  /// `state` record naming the scene. The behaviours' authoring ids stay
  /// as the runtime → authored map for later step diagnostics.
  void _loadedForEditor(LoadReport report, String op) {
    final document = report.scenePath;
    for (final d in report.diagnostics) {
      EditorChannel.diagnostic(
        code: d.code,
        severity: d.severity,
        message: d.message,
        tick: tick,
        source: op,
        document: d.document ?? document,
        pointer: d.pointer,
        line: d.line,
        column: d.column,
        suggestion: d.suggestion,
      );
    }
    if (!report.ok) {
      EditorChannel.state(
        'loadFailed',
        tick: tick,
        fields: {'op': op, 'scene': report.sceneName, 'document': document},
      );
      return;
    }
    if (op == 'bindActions') return;
    if (report.behaviours.isNotEmpty) {
      _authoredDocument = document;
      for (final b in report.behaviours) {
        if (b.authoringId.isNotEmpty) {
          _authoredEntities[b.entity] = b.authoringId;
        }
      }
    }
    EditorChannel.state(
      'loaded',
      tick: tick,
      fields: {
        'op': op,
        'scene': report.sceneName,
        'sceneId': report.sceneId,
        'document': document,
        'entities': report.entityCount,
        'assets': report.assetCount,
        'behaviours': report.behavioursAttached,
        'loadMs': report.loadMs,
      },
    );
  }

  /// The project's persistent objects a [loadProject] made (ADR-0039):
  /// each root attached under its name and kept, and its scripts attached
  /// before the startup scene's, so theirs are created first and a scene
  /// script finds them in its own `onCreate`.
  LoadReport _attachPersistentObjects(LoadReport report) {
    _projectPersistent.clear();
    final extra = <LoadDiagnostic>[];
    for (final p in report.persistentObjects) {
      final entity = p.entity;
      if (entity == null) continue;
      final go = attach(entity, name: p.name);
      _projectPersistent[p.name] = go;
      if (p.behaviours.isNotEmpty) {
        extra.addAll(_attachBehaviours(p.behaviours, p.path).$2);
      }
    }
    return extra.isEmpty ? report : report.withBehaviours(attached: 0, extra: extra);
  }

  /// Instantiates every authored behaviour of a loaded scene from
  /// [registry] and attaches it to its spawned entity (M2-16: the scene
  /// component `game.Spinner` with `speed: 2.5` becomes a `Spinner` with
  /// that field set). A type the registry lacks is `FYN-B0001`, data the
  /// type rejects is `FYN-B0002`; both are warnings — the entity and its
  /// other components are loaded regardless.
  LoadReport _attachAuthoredBehaviours(LoadReport report) {
    if (report.behaviours.isEmpty) return report;
    final (attached, extra) = _attachBehaviours(report.behaviours, report.scenePath, scene: true);
    return report.withBehaviours(attached: attached, extra: extra);
  }

  /// Attaches every behaviour of one load, then sets their GameObject and
  /// script fields (Unity's object references): a reference resolves
  /// against this load — the handles the native report gives for the
  /// authoring ids, the scripts by component id — so it may name an
  /// object whose script is attached after its own, and a prefab
  /// instance's references reach its own copies. A target that is gone
  /// (deleted, a component removed) or of another type leaves the field
  /// null, with `FYN-B0003`. [scene]: a scene load, whose authoring ids
  /// are the objects' live ids ([GameObject.authoringId]).
  (int, List<LoadDiagnostic>) _attachBehaviours(
    List<AuthoredBehaviour> behaviours,
    String? document, {
    bool scene = false,
  }) {
    final extra = <LoadDiagnostic>[];
    var attached = 0;
    final byComponent = <String, Behaviour>{};
    final withReferences = <(AuthoredBehaviour, Behaviour)>[];
    final withData = <(AuthoredBehaviour, Behaviour)>[];
    for (final b in behaviours) {
      final type = registry.lookup(b.type);
      if (type == null) {
        extra.add(
          LoadDiagnostic(
            code: 'FYN-B0001',
            severity: 'warning',
            message: 'behaviour type ${b.type} is not registered; the component is loaded without a behaviour',
            document: document,
            pointer: b.pointer,
            suggestion: 'call registerBehaviours(world.registry) from the generated behaviours.g.dart',
          ),
        );
        continue;
      }
      try {
        final go = attach(b.entity);
        if (scene && b.authoringId.isNotEmpty) go._authoringId = b.authoringId;
        final behaviour = go.addBehaviourFromData(b.type, fields: b.data, version: b.version)
          .._authoredComponent = b.component.isEmpty ? null : b.component;
        if (b.component.isNotEmpty) byComponent[b.component] = behaviour;
        if (type.fields.any((f) => f.isReference && b.data[f.name] != null)) {
          withReferences.add((b, behaviour));
        }
        if (type.fields.any((f) => f.holdsData && b.data[f.name] != null)) {
          withData.add((b, behaviour));
        }
        attached++;
      } on BehaviourDataError catch (e) {
        extra.add(
          LoadDiagnostic(
            code: 'FYN-B0002',
            severity: 'warning',
            message: '${b.type}${e.field == null ? '' : '.${e.field}'}: ${e.message}',
            document: document,
            pointer: b.pointer == null ? null : '${b.pointer}/data${e.field == null ? '' : '/${e.field}'}',
            suggestion: 'fix the component data or regenerate behaviours.g.dart (`fynkel codegen --project`)',
          ),
        );
      }
    }
    for (final (b, behaviour) in withReferences) {
      for (final f in behaviour.behaviourType!.fields) {
        if (!f.isReference) continue;
        final value = b.data[f.name];
        if (value == null) continue;
        final target = _resolveReference(value, b.references, byComponent, scene: scene);
        String? problem;
        if (target == null) {
          problem =
              'references ${value is Map && value['component'] != null ? 'a script' : 'an object'} that is not in the scene';
        } else {
          try {
            f.set(behaviour, target);
          } on Object {
            problem = f.type == 'component' && target is GameObject
                ? 'references "${target.name}", which has no ${f.component}'
                : 'references a ${target is Behaviour ? target.behaviourType?.id ?? target.runtimeType : 'GameObject'}, '
                      'which does not fit the field';
          }
        }
        if (problem != null) {
          extra.add(
            LoadDiagnostic(
              code: 'FYN-B0003',
              severity: 'warning',
              message: '${b.type}.${f.name} $problem; it is None',
              document: document,
              pointer: b.pointer == null ? null : '${b.pointer}/data/${f.name}',
              suggestion: 'pick the object again in the Inspector',
            ),
          );
        }
      }
    }
    // Data assets (ADR-0027 §4): the shared instances.
    for (final (b, behaviour) in withData) {
      for (final f in behaviour.behaviourType!.fields) {
        if (!f.holdsData) continue;
        final value = _data.resolve(
          b.data[f.name],
          f.dataType!,
          extra,
          where: '${b.type}.${f.name}',
          document: document,
          pointer: b.pointer == null ? null : '${b.pointer}/data/${f.name}',
        );
        if (value != null) f.set(behaviour, value);
      }
    }
    return (attached, extra);
  }

  /// A reference field's JSON as the object it names in one load (null
  /// when it is not there).
  Object? _resolveReference(
    Object? value,
    Map<String, EntityRef> handles,
    Map<String, Behaviour> byComponent, {
    required bool scene,
  }) {
    if (value is! Map) return null;
    final entity = value['entity'], component = value['component'];
    if (component is String) {
      final b = byComponent[component];
      return b != null && !b.isDestroyed ? b : null;
    }
    if (entity is! String) return null;
    final handle = handles[entity];
    if (handle == null) return null;
    final go = byEntity(handle) ?? attach(handle);
    if (scene) go._authoringId ??= entity;
    return go;
  }

  /// [GameObject.clone]'s scripts: each copied through its registered
  /// fields onto the object's copy, then the references re-pointed into
  /// the copied tree.
  void _copyBehaviours(Map<GameObject, GameObject> copies) {
    final edit = _gameplay!;
    final warnings = <LoadDiagnostic>[];
    final made = Map<Behaviour, Behaviour>.identity();
    // Freeze membership and enabled state before any user factory runs. Field
    // accessors still execute in this edit and may have ordinary Dart effects.
    final sources = {
      for (final original in copies.keys)
        original: [
          for (final b in edit.behavioursOf(original))
            if (!b._destroyRequested && !b._destroyed) (b, edit.behaviourEnabled(b)),
        ],
    };
    for (final MapEntry(key: original, value: copy) in copies.entries) {
      for (final (b, enabled) in sources[original]!) {
        if (b._destroyRequested || b._destroyed) continue;
        final type = b._type ?? _registeredTypeOf(b);
        if (type == null) {
          warnings.add(
            LoadDiagnostic(
              code: 'FYN-B0001',
              severity: 'warning',
              message:
                  '${b.runtimeType} on "${original.name}" is not a registered behaviour; the copy has no ${b.runtimeType}',
              suggestion: 'annotate it with @FynkelBehaviour and call registerBehaviours(world.registry) from the generated behaviours.g.dart',
            ),
          );
          continue;
        }
        final c = type.create().._type = type;
        for (final field in type.fields) {
          if (field.isReference) continue;
          // Field failures poison the entire clone, rather than publishing a
          // partly initialized script. Data assets retain their shared identity.
          field.set(c, field.holdsData ? field.target!(b) : field.get(b));
        }
        c._enabled = enabled;
        edit.attachBehaviour(copy, c);
        made[b] = c;
      }
    }
    for (final MapEntry(key: b, value: c) in made.entries) {
      for (final field in c._type!.fields) {
        if (!field.isReference) continue;
        final read = field.target;
        if (read == null) {
          if (field.get(b) != null) {
            edit.reject('${c._type!.id}.${field.name} requires regenerated reference accessors for clone');
          }
          continue;
        }
        final original = read(b);
        final Object? target;
        switch (original) {
          case GameObject g:
            target = edit.native.contains(edit.ref(g)) ? copies[g] ?? g : null;
          case Behaviour script:
            final owner = edit._behaviourOwner(script);
            target = edit.behaviourRemoved(script) || !edit.native.contains(edit.ref(owner))
                ? null
                : copies.containsKey(owner)
                ? made[script]
                : script;
          case NativeComponent component:
            final owner = component.gameObject;
            target = edit.native.contains(edit.ref(owner)) ? copies[owner] ?? component : null;
          default:
            target = original;
        }
        field.set(c, target);
      }
    }
    if (warnings.isNotEmpty) edit.afterCommit(() => onLoadDiagnostics(warnings));
  }

  /// The registered type of a script the game constructed itself (its
  /// [Behaviour.behaviourType] is null): the registry's type whose factory
  /// makes the same class.
  BehaviourType? _registeredTypeOf(Behaviour b) {
    var byClass = _typesByClass;
    if (byClass == null || _typesByClassCount != registry.revision) {
      byClass = _typesByClass = {for (final t in registry.types) (t.dartType ?? t.create().runtimeType): t};
      _typesByClassCount = registry.revision;
    }
    return byClass[b.runtimeType];
  }

  Map<Type, BehaviourType>? _typesByClass;
  int _typesByClassCount = 0;

  // ---------------------------------------------------------------- prefabs (M3-08b)

  /// The sub-asset an imported model's node hierarchy is stored under
  /// (`{ asset: <glb>, sub: 'prefab' }` in a scene, [instantiate] at
  /// runtime), and a rig document's skeleton hierarchy too
  /// (`AssetRef(rig, sub: 'prefab')`; its clips are `anim/<name>`, M3-19b).
  static const importedPrefabSub = 'prefab';

  /// Instantiates the cooked prefab [prefab] (its asset id as UUID text, or
  /// an [AssetRef] — a prefab document, or an imported model or rig
  /// document with its `prefab` sub-asset, `AssetRef(glb, sub: 'prefab')`,
  /// whose node hierarchy then spawns with its meshes and `Skin`s;
  /// M3-19a/b) **now**:
  /// the prefab's root (ADR-0022), named [name] (the prefab's name by
  /// default), at [transform] under [parent], with the prefab's other
  /// entities under it — the shape a scene instance expands to — and the
  /// prefab's authored behaviours attached from [registry]. Returns the
  /// root, already resolved outside an edit (no tick needed). Invalid values
  /// reject the transaction; [lastInstantiate] has the committed report.
  /// Missing prefabs, invalid data or a missing project throw [StateError].
  /// Inside [edit], objects/resources and authored script attachments commit
  /// together; callbacks wait for the next lifecycle boundary. Invalid authored
  /// fields/types/references abort the edit.
  /// Shared data assets use the edit's staged cache. Diagnostics are available
  /// in [lastInstantiate] after commit.
  /// Outside edits the same transaction publishes before returning. Lifecycle
  /// callbacks run at the next normal boundary. Preparation errors throw [StateError].
  GameObject instantiate(Object prefab, {Pose transform = identityTransform, GameObject? parent, String? name}) {
    return _withGameplay(() => _gameplay!.instantiate(prefab, transform, parent, name));
  }

  /// Runs [onValue] every frame with a value going from [from] to [to]
  /// over [seconds] of game time, eased by [ease] (DOTween's `DOTween.To`):
  /// numbers, `Vec2`/`Vec3`/`Vec4` (colours too) in a straight line,
  /// `Quat`s along the shorter arc. The first call is at the next scheduler
  /// boundary, with [from]; the
  /// last, with [to], is the frame the time is up. Returns the running
  /// tween (yield it, `await` its `done`, or `stop()` it); it belongs to
  /// the world — [Behaviour.tween] is the one that stops with its script.
  /// Inside [edit], starting is deferred until a scheduler phase after commit;
  /// abort never calls [onValue]. Duration starts when the tween first executes.
  Coroutine tween<T extends Object>(
    T from,
    T to,
    double seconds,
    void Function(T value) onValue, {
    Ease ease = Ease.outQuad,
    TimeClock clock = TimeClock.gameplay,
  }) => _tweenOn(null, seconds, ease, _between(from, to, onValue), clock);

  /// The report of the last [instantiate] (null before any).
  InstantiateReport? lastInstantiate;

  /// Called with the warnings of a successful load. Defaults to `print`
  /// (silent under the editor channel, as [onDiagnostics] is).
  void Function(List<LoadDiagnostic>) onLoadDiagnostics = _printLoadDiagnostics;

  static void _printDiagnostics(List<FynkelDiagnostic> d) {
    if (!EditorChannel.enabled) print(d.join('\n'));
  }

  static void _printLoadDiagnostics(List<LoadDiagnostic> d) {
    if (!EditorChannel.enabled) print(d.join('\n'));
  }

  BridgeStats stats() => _native.stats();

  /// Frees the native world. Its objects go dead, as destroyed ones do,
  /// keeping their last pose to read; every later call on the world
  /// throws a [StateError] (foundation 4.2). Twice is fine.
  void dispose() {
    final edit = _gameplay;
    if (edit != null && edit.automatic && edit.explicitDepth == 0) {
      edit.afterCommit(dispose);
      return;
    }
    _rejectDuringEdit('dispose');
    if (_native.isDisposed) {
      _releaseResourceOwnership();
      return;
    }
    for (final go in _byEntity.values) {
      // Disposal owns this last readable value; it is never a live-state cache.
      go._retiredPose = go._lastPose;
      if (!_native.isPoisoned && go._entity != null && _native.contains(go._entity!)) {
        final b = _native.query(QueryKind.transform, entity: go._entity!);
        if (b.recordCount != 0) {
          go._retiredPose = gen.SnapshotTransformRecord.decode(b.records, 0, b.tail).local;
        }
      }
      go._alive = false;
    }
    _byEntity.clear();
    _snapshot = WireBuffer.empty;
    _snapshotIndex.clear();
    _behaviours._disposeRoutines();
    scenes._disposed();
    _native.dispose();
    _releaseResourceOwnership();
    if (ownsLibrary) _lib.dispose();
  }

  // These describe resources owned by this world, not independently live assets.
  // This also releases SDK metadata after a host disposed the native world first.
  void _releaseResourceOwnership() {
    _runtimeMaterials.clear();
    _shaderMaterials.clear();
    _codeValues.clear();
    _runtimeMeshes.clear();
    _runtimeClips.clear();
    _data.byId.clear();
  }

  /// Whether [dispose] ran.
  bool get isDisposed => _native.isDisposed;
}

/// The audio engine of a [World] ([World.audio], M3-11, docs/audio.md).
/// Everything a game hears goes through the world: a source on an object
/// ([GameObject.audio]) or a fire-and-forget [play]; the buses
/// `master`/`music`/`sfx` scale it, the settings document drives them
/// ([applySettings]). Without [open] the null backend runs the same
/// clock — every test hears nothing and sees everything.
class AudioSystem {
  AudioSystem._(this._world);
  final World _world;
  bool _open = false;

  /// Opens the platform's output device: from then on the voices the
  /// ticks decided are rendered. False (and an `audio_device`
  /// diagnostic) when the platform has none or the build carries no
  /// device backend; the world keeps the null backend. Idempotent.
  /// `FynkelGameView(audio: true)` calls it after the load; a test never
  /// does.
  bool open() {
    _world._rejectDuringEdit('audio.open');
    if (_open) return true;
    _open = _world._native.audioOpen();
    if (!_open) _world._collectDiagnostics('audio');
    return _open;
  }

  /// The device is open.
  bool get isOpen => _open;

  /// Closes the device (the voices continue on the null backend).
  void close() {
    _world._rejectDuringEdit('audio.close');
    if (!_open) return;
    _world._native.audioClose();
    _open = false;
  }

  /// Pauses (or resumes) the device: the host's lifecycle calls it when
  /// the app leaves the foreground. A no-op without a device.
  void suspend(bool suspended) {
    _world._rejectDuringEdit('audio.suspend');
    _world._native.audioSuspend(suspended);
  }

  /// Plays [clip] once, fire and forget (`fynkel.cmd.PlaySound`): no
  /// object, no event, no handle; the sound is what the tick decided
  /// (simulation lane, applied at the next tick) — or, [presentation],
  /// applied at once through the immediate audio boundary, for UI sounds
  /// outside the ticks (a test replay does not carry those). A clip the
  /// world does not hold is `SOUND_UNAVAILABLE`; [volume] outside `0…1`
  /// or [pitch] outside `0.05…8` is `INVALID_PAYLOAD`. Under the voice
  /// limit the sound steals a voice of a lower or equal [priority] or is
  /// not heard (`stats.refused`). Inside [World.edit], simulation playback is
  /// staged until commit and executes once at the next tick; abort discards it.
  /// Immediate presentation playback is not supported inside an edit.
  void play(
    AssetRef clip, {
    gen.AudioBus bus = gen.AudioBus.sfx,
    double volume = 1,
    double pitch = 1,
    int priority = 0,
    bool presentation = false,
  }) {
    if (presentation) {
      _world._rejectDuringEdit('audio.play(presentation: true)');
      _world.modules.require(Module.audio, 'audio.play');
      final status = _world._native.playSoundNow(
        gen.PlaySoundCommandRecord(clip: clip, bus: bus, volume: volume, pitch: pitch, priority: priority),
      );
      _world._checkFatal(status, 'audio.play');
      if (status == FynkelStatus.FYNKEL_STATUS_COMMANDS_REJECTED) {
        _world._collectDiagnostics('audio.play');
      }
      return;
    }
    _world._withGameplay(() {
      _world.modules.require(Module.audio, 'audio.play');
      final edit = _world._gameplay!;
      edit.guard(
        () => edit.native.playSound(
          gen.PlaySoundCommandRecord(clip: clip, bus: bus, volume: volume, pitch: pitch, priority: priority),
        ),
      );
    });
  }

  /// Sets a bus gain (`0…1`) and the global mute switch in one gameplay
  /// transaction. Outside a callback or [World.edit], opens an edit; inside
  /// one, joins it. Commit publishes audio configuration without a tick; the
  /// device consumes mixer commands asynchronously.
  /// [muted] is shared by all buses, so the last bus write wins for mute.
  void setBus(gen.AudioBus bus, double volume, {bool muted = false}) => _busEdit(() => _stageBus(bus, volume, muted));

  void _stageBus(gen.AudioBus bus, double volume, bool muted) {
    _world.modules.require(Module.audio, 'audio.setBus');
    _world._gameplay!.native.setAudioBus(bus, volume, muted: muted);
  }

  /// Logical bus gain, including writes in the current edit. Outside an edit,
  /// opens the same read-only lifecycle reconciliation as [World.edit]. Reading does not advance simulation.
  double busVolume(gen.AudioBus bus) => _busEdit(() {
    return _world._gameplay!.native.audioBus(bus).$1;
  });

  /// The logical global mute switch shared by all buses.
  bool get muted => _busEdit(() {
    return _world._gameplay!.native.audioBus(gen.AudioBus.master).$2;
  });

  R _busEdit<R>(R Function() operation) {
    final edit = _world._gameplay;
    return edit == null ? _world.edit(operation) : edit.guard(operation);
  }

  /// Applies all three settings gains and global mute atomically. A bad gain
  /// rejects the whole edit, including earlier bus writes. [SettingsDocument]
  /// is immutable; no user settings object is modified by the mixer.
  ///
  /// A no-op when the project turns audio off: settings retain their values.
  void applySettings(SettingsDocument settings) {
    if (!_world.modules.audio) return;
    _busEdit(() {
      _stageBus(gen.AudioBus.master, settings.masterVolume, settings.muted);
      _stageBus(gen.AudioBus.music, settings.musicVolume, settings.muted);
      _stageBus(gen.AudioBus.sfx, settings.sfxVolume, settings.muted);
    });
  }

  /// The engine's counters now (`FYNKEL_QUERY_AUDIO_STATS`).
  AudioStats get stats {
    final b = _world._native.query(QueryKind.audioStats);
    return AudioStats.fromRecord(gen.AudioStatsRecord.decode(b.records, 0, b.tail));
  }
}

/// Stable handle to an entity. Owns no native memory.
/// Playback of one object's `fynkel.Animator` ([GameObject.animation]).
/// Outside edits, methods submit commands for the next tick. Inside [World.edit],
/// clip controls change logical configuration and stage runtime playback until
/// commit and the next tick. Controller-specific operations remain guarded.
/// See [state] for committed playback reads.
class AnimationControl {
  AnimationControl._(this._go);
  final GameObject _go;

  /// Plays [target] at once, replacing what plays, no blend.
  ///
  /// A `String` is a state of the animator's controller (ADR-0032,
  /// Unity's `Play(stateName)`): it starts at [offset] (a fraction of its
  /// motion, 0…1) in [layer] (null: the first layer that has it); an
  /// unknown state is an `invalidPayload` diagnostic that lists the
  /// states. A state inside a sub-state machine is named by its path
  /// (`'Airborne/Fall'`, ADR-0033 § 4); the machine's own path enters
  /// its default state.
  ///
  /// An [AssetRef] is a clip, for an animator without a controller: it
  /// plays from its start (or its end, at a negative [speed]). It may
  /// name a blend space (`<name>.blend.json`, M3-19c): its points are
  /// then mixed by the parameter it names ([setParameter]),
  /// phase-synced, and [state] reports the blended duration. On an
  /// animator with a controller it throws a [StateError]: the controller
  /// owns what plays.
  void play(Object target, {bool looping = true, double speed = 1, int? layer, double offset = 0}) {
    if (target is String) {
      _go._animate(gen.AnimateOp.playState, name: target, layer: layer ?? -1, value: offset);
    } else if (target is AssetRef) {
      _noController('play(clip)', 'play(stateName)');
      _go._animate(gen.AnimateOp.play, clip: target, speed: speed, looping: looping);
    } else {
      _go.world._gameplay?.reject('animation target must be a state name or clip');
      throw ArgumentError.value(target, 'target', 'a state name (String) or a clip (AssetRef)');
    }
  }

  /// Blends from the current pose into [target] over [seconds]: what
  /// played keeps advancing underneath until the fade ends. A fade of 0
  /// is a [play]; a negative fade is rejected (`INVALID_PAYLOAD`).
  ///
  /// A `String` is a state of the animator's controller (Unity's
  /// `CrossFade(stateName, seconds)`), started at [offset] in [layer];
  /// an [AssetRef] a clip, for an animator without one (see [play]).
  void crossfade(
    Object target,
    double seconds, {
    bool looping = true,
    double speed = 1,
    int? layer,
    double offset = 0,
  }) {
    if (target is String) {
      _go._animate(gen.AnimateOp.crossFadeState, name: target, seconds: seconds, layer: layer ?? -1, value: offset);
    } else if (target is AssetRef) {
      _noController('crossfade(clip, …)', 'crossfade(stateName, seconds)');
      _go._animate(gen.AnimateOp.crossfade, clip: target, seconds: seconds, speed: speed, looping: looping);
    } else {
      _go.world._gameplay?.reject('animation target must be a state name or clip');
      throw ArgumentError.value(target, 'target', 'a state name (String) or a clip (AssetRef)');
    }
  }

  /// Stops and rewinds: the nodes keep the pose of the last tick until
  /// another clip plays.
  void stop() => _go._animate(gen.AnimateOp.stop);

  /// Plays [motion] (a clip or a blend space) once in a **slot** of the
  /// animator's controller (ADR-0033 § 2): it covers the layer's state
  /// machine — blending in over [blendIn] seconds, blending out over the
  /// last [blendOut] — while the machine keeps running underneath, so it
  /// is in the right state when the one-shot ends. [slot] is a layer's
  /// name (empty: the base layer); a layer with no states shows only its
  /// slot, which is how an upper-body attack plays over walking legs.
  /// Another one-shot in the same slot blends from the one playing. Its
  /// markers arrive as usual and [Behaviour.onAnimationEnded] fires at
  /// its end (not after [stopOneShot]).
  ///
  /// Requests commit through the gameplay journal and execute at the next
  /// tick. Invalid slots or an animator without a controller throw [StateError].
  /// Use [play] for a clip without a controller.
  void playOneShot(AssetRef motion, {String slot = '', double blendIn = 0.1, double blendOut = 0.2, double speed = 1}) {
    if (_go.world._gameplay == null) {
      return _go.world.edit(() => playOneShot(motion, slot: slot, blendIn: blendIn, blendOut: blendOut, speed: speed));
    }
    _needsController('playOneShot');
    _go._animate(gen.AnimateOp.playOneShot, clip: motion, name: slot, seconds: blendIn, value: blendOut, speed: speed);
  }

  /// Blends [slot]'s one-shot out over [blendOut] seconds.
  void stopOneShot({String slot = '', double blendOut = 0.2}) {
    if (_go.world._gameplay == null) {
      return _go.world.edit(() => stopOneShot(slot: slot, blendOut: blendOut));
    }
    _needsController('stopOneShot');
    _go._animate(gen.AnimateOp.stopOneShot, name: slot, value: blendOut);
  }

  /// Whether the completed tick reports a one-shot in [slot]. Play/stop
  /// requests do not change this result until a tick executes them. An empty
  /// slot and the base layer's name report the same state. A one-shot blending
  /// out after [stopOneShot] has executed does not count.
  bool isPlayingOneShot({String slot = ''}) {
    final all = layers;
    final info = slot.isEmpty ? all.firstOrNull : all.where((l) => l.slot == slot).firstOrNull;
    return info?.oneShot != null;
  }

  /// Where IK goal [goal] of the animator's controller reaches or looks
  /// (ADR-0033 § 5), in world space, from the next tick until
  /// [clearIKTarget]: a hand to a door handle, the head to the nearest
  /// coin. It wins over the object the goal names in the controller;
  /// [weight] (0 → 1) scales the goal's own weight. A `twoBone` goal's tip
  /// reaches [position], a `lookAt` goal's head turns to it (eased, within
  /// its limit); `footPlacement` finds its ground itself.
  ///
  /// An unknown goal is an `invalidPayload` diagnostic that lists the
  /// goals. Throws a [StateError] on an animator without a controller.
  void setIKTarget(String goal, Vec3 position, {double weight = 1}) {
    _needsController('setIKTarget', what: 'IK goals live in a controller');
    if (!(weight >= 0 && weight <= 1)) {
      _go.world._gameplay?.reject('IK weight must be within [0, 1]');
      throw ArgumentError.value(weight, 'weight', 'within [0, 1]');
    }
    _go._animate(gen.AnimateOp.setIkTarget, name: goal, point: position, value: weight);
  }

  /// Drops the target [setIKTarget] gave [goal]: back to the object the
  /// goal names, or to nothing (a goal without a target does not bend).
  void clearIKTarget(String goal) {
    _needsController('clearIKTarget', what: 'IK goals live in a controller');
    _go._animate(gen.AnimateOp.clearIkTarget, name: goal);
  }

  void _needsController(String call, {String? what}) {
    if (!_controlled) {
      _go.world._gameplay?.reject('animation.$call requires an Animator Controller');
      throw StateError(
        'animation.$call: ${_go.name} has no Animator Controller; '
        "${what ?? "a one-shot plays in a controller's slot (play the clip instead)"}",
      );
    }
  }

  /// Holds the current time; [resume] continues from it. Inside [World.edit],
  /// these change logical `Animator.playing` immediately and publish on commit.
  void pause() => _go._animate(gen.AnimateOp.pause);
  void resume() => _go._animate(gen.AnimateOp.resume);

  /// Playback rate from the next tick; negative plays backwards, 0 holds.
  /// With a controller, it scales every state (Unity's `animator.speed`).
  /// Inside [World.edit], configuration reads see the new speed immediately.
  set speed(double value) => _go._animate(gen.AnimateOp.setSpeed, speed: value);

  /// Seeks to [seconds] into the clip (clamped to its length); the pose
  /// there is applied at the next tick without advancing. Not for an
  /// animator with a controller (use [play] with an `offset`).
  set time(double seconds) {
    _noController('time =', "play(stateName, offset: …)");
    _go._animate(gen.AnimateOp.setTime, seconds: seconds);
  }

  /// Layer weight from the next tick, `0…1` (M3-19c): how far this
  /// animator's pose replaces (or, `additive`, is added onto) what the
  /// animators above it left. Outside the range is rejected
  /// (`INVALID_PAYLOAD`).
  set weight(double value) => _go._animate(gen.AnimateOp.setWeight, value: value);

  /// Sets a named blend parameter (M3-19c): a playing blend space reads
  /// the one it names every tick (clamped to its points; the phase is
  /// kept across a change); a plain clip keeps the value for the next
  /// blend space. [value] must be finite and [name] non-empty. With a
  /// controller, use [setFloat].
  void setParameter(String name, double value) {
    _noController('setParameter', 'setFloat');
    _go._animate(gen.AnimateOp.setParameter, name: name, value: value);
  }

  /// Committed animator configuration plus completed playback time, fade and
  /// root-motion results (`FYNKEL_QUERY_ANIMATOR`). During [World.edit], this
  /// excludes staged configuration; use `get<Animator>()` for logical values.
  /// `null` without a committed `Animator` or before the object spawned.
  AnimatorState? get state {
    final e = _go._entity;
    if (e == null || !_go._alive) return null;
    final b = _go.world._native.query(QueryKind.animator, entity: e);
    if (b.recordCount == 0) return null;
    return AnimatorState.fromRecord(gen.AnimatorStateRecord.decode(b.records, 0, b.tail));
  }

  /// True while a clip advances ([AnimatorState.playing]).
  bool get isPlaying => state?.playing ?? false;

  // ------------------------------------ the Animator Controller (ADR-0032)

  /// Sets a typed controller parameter in the native gameplay journal.
  /// Outside a callback or [World.edit], this opens an edit; inside one it
  /// joins that scope. Validation failures abort the edit. Playback consumes
  /// committed requests at the next animation tick.
  void setFloat(String name, double value) =>
      _parameterEdit(() => _go._animate(gen.AnimateOp.setFloat, name: name, value: value));

  /// Sets an int parameter (Unity's `SetInteger`).
  void setInteger(String name, int value) =>
      _parameterEdit(() => _go._animate(gen.AnimateOp.setInteger, name: name, integer: value));

  /// Sets a bool parameter or trigger value (Unity's `SetBool`).
  void setBool(String name, bool value) =>
      _parameterEdit(() => _go._animate(gen.AnimateOp.setBool, name: name, value: value ? 1 : 0));

  /// Sets a trigger: it stays set until a transition tests and consumes it.
  void setTrigger(String name) => _parameterEdit(() => _go._animate(gen.AnimateOp.setTrigger, name: name));

  /// Clears a trigger nothing consumed.
  void resetTrigger(String name) => _parameterEdit(() => _go._animate(gen.AnimateOp.resetTrigger, name: name));

  /// Reads the selected controller's native logical parameter value:
  /// registered defaults or completed values, followed by committed requests
  /// and the current edit's writes. No simulation tick or SDK value cache is
  /// involved. [parameters] separately reports completed playback values.
  ///
  /// Outside an edit, getters open an edit and reconcile live gameplay
  /// work first, like [World.edit]. This can resolve pending objects or publish
  /// queued writes, and invalid queued work can reject the read.
  double getFloat(String name) => (_read(name, gen.AnimatorParameterKind.float) as double?) ?? 0;

  /// Reads a logical int parameter.
  int getInteger(String name) => (_read(name, gen.AnimatorParameterKind.int) as int?) ?? 0;

  /// Reads a logical bool parameter or trigger value.
  bool getBool(String name) => (_read(name, gen.AnimatorParameterKind.bool) as bool?) ?? false;

  R _parameterEdit<R>(R Function() operation) {
    final edit = _go.world._gameplay;
    return edit == null ? _go.world.edit(operation) : edit.guard(operation);
  }

  /// The controller's parameters after the last tick
  /// (`FYNKEL_QUERY_ANIMATOR_PARAMETERS`); empty without a running
  /// controller.
  List<AnimatorParameter> get parameters {
    final e = _go._entity;
    if (e == null || !_go._alive) return const [];
    final b = _go.world._native.query(QueryKind.animatorParameters, entity: e);
    return [
      for (var i = 0; i < b.recordCount; i++)
        AnimatorParameter.fromRecord(
          gen.AnimatorParameterRecord.decode(b.records, i * gen.AnimatorParameterRecord.wireSize, b.tail),
        ),
    ];
  }

  /// Every layer's state after the last tick
  /// (`FYNKEL_QUERY_ANIMATOR_CONTROLLER`); empty without a running
  /// controller.
  List<AnimatorStateInfo> get layers {
    final e = _go._entity;
    if (e == null || !_go._alive) return const [];
    final b = _go.world._native.query(QueryKind.animatorController, entity: e);
    return [
      for (var i = 0; i < b.recordCount; i++)
        AnimatorStateInfo.fromRecord(
          gen.AnimatorLayerStateRecord.decode(b.records, i * gen.AnimatorLayerStateRecord.wireSize, b.tail),
        ),
    ];
  }

  /// One layer's state after the last tick (Unity's
  /// `GetCurrentAnimatorStateInfo`, with `next` for
  /// `GetNextAnimatorStateInfo`); null without a running controller or
  /// such a layer.
  AnimatorStateInfo? stateOf({int layer = 0}) {
    final all = layers;
    return layer >= 0 && layer < all.length ? all[layer] : null;
  }

  bool get _controlled => _go.get<gen.Animator>()?.controller != null;

  void _noController(String call, String instead) {
    if (_controlled) {
      _go.world._gameplay?.reject('animation.$call requires an animator without a controller');
      throw StateError(
        'animation.$call: ${_go.name} runs an Animator Controller, which owns what plays; '
        'use animation.$instead',
      );
    }
  }

  static bool _fits(gen.AnimatorParameterKind have, gen.AnimatorParameterKind want) => switch (want) {
    gen.AnimatorParameterKind.float => have == gen.AnimatorParameterKind.float,
    gen.AnimatorParameterKind.int => have == gen.AnimatorParameterKind.int,
    // A bool value fits a bool and a trigger, as in the engine.
    _ => have == gen.AnimatorParameterKind.bool || have == gen.AnimatorParameterKind.trigger,
  };

  Object? _read(String name, gen.AnimatorParameterKind kind) => _parameterEdit(() {
    final edit = _go.world._gameplay!;
    final parameter = edit.native.controllerParameter(edit.ref(_go), name);
    if (parameter == null) return null;
    if (!_fits(parameter.$1, kind)) {
      throw ArgumentError.value(name, 'name', 'parameter `$name` is a ${parameter.$1.name}, not a ${kind.name}');
    }
    return parameter.$2;
  });
}

/// Playback of one object's `fynkel.AudioSource` ([GameObject.audio],
/// M3-11). Outside edits, methods submit commands for the next tick. Inside
/// [World.edit], gain, pitch and playing configuration change immediately in
/// the logical view; playback changes only after commit at the next audio tick.
/// [state] reports the actual voice, with source configuration when no voice exists.
class AudioControl {
  AudioControl._(this._go);
  final GameObject _go;

  /// Plays the source's clip from its start, restarting one that plays.
  /// A non-looping clip ends with `onPlaybackEnded`; the source's
  /// `playing` clears then, after [stop], and when the engine stole or
  /// refused the voice.
  void play() => _go._audio(gen.AudioOp.play);

  /// Stops the voice (no event).
  void stop() => _go._audio(gen.AudioOp.stop);

  /// Holds the voice; [resume] continues from where it was.
  void pause() => _go._audio(gen.AudioOp.pause);
  void resume() => _go._audio(gen.AudioOp.resume);

  /// The source's gain, `0…1`, written to the component and the voice.
  set volume(double value) => _go._audio(gen.AudioOp.setVolume, value: value);

  /// The source's playback rate, `0.05…8`, written to the component and
  /// the voice.
  set pitch(double value) => _go._audio(gen.AudioOp.setPitch, value: value);

  /// The source's playback after the last tick (`FYNKEL_QUERY_AUDIO`);
  /// `null` without an `AudioSource` or before the object spawned.
  AudioState? get state {
    final e = _go._entity;
    if (e == null || !_go._alive) return null;
    final b = _go.world._native.query(QueryKind.audio, entity: e);
    if (b.recordCount == 0) return null;
    return AudioState.fromRecord(gen.AudioStateRecord.decode(b.records, 0, b.tail));
  }

  /// True while the source holds a voice ([AudioState.playing]).
  bool get isPlaying => state?.playing ?? false;
}

class GameObject extends FynkelObject {
  GameObject._(World this._world, this._initial, this._name) : _spec = null {
    transform = Transform._(this);
  }

  /// [ObjectSpec.object] before its spec is built (ADR-0036 §4): a script
  /// field or a component can hold it, and [World.build] makes it the
  /// built object. Anything else throws until then.
  GameObject._placeholder(ObjectSpec this._spec) : _world = null, _initial = _spec.transform, _name = _spec.name ?? '' {
    transform = Transform._(this);
    _alive = false;
  }

  /// The world the object is in.
  World get world => _world ?? _gameplay?.world ?? _notBuilt();
  World? _world;

  // An unbuilt spec handle has no committed world. Its identity belongs only
  // to the synchronous edit until publication promotes it to a normal object.
  _GameplayAdapter? get _gameplay {
    if (_world != null) return _world!._gameplay;
    final edit = Zone.current[_gameplayZoneKey] as _GameplayAdapter?;
    if (edit == null) return null;
    edit.checkOpen();
    return edit.pending.containsKey(this) ? edit : null;
  }

  Never _notBuilt() =>
      throw StateError('the object of $_spec is not built yet: world.build the spec, then use spec.object');

  /// The declaration this handle belongs to until its first successful build.
  ObjectSpec? _spec;
  final Pose _initial;
  String _name;

  /// The current logical name. Setting it uses the same transaction as [set].
  /// Retired objects retain their last committed name for diagnostics.
  @override
  String get name {
    final current = _gameplay;
    if (current != null) return current.native.name(current.ref(this)) ?? '';
    if (_spec != null || !_alive || world.isDisposed) return _name;
    return world._withGameplay(() {
      final edit = world._gameplay!;
      return edit.native.name(edit.ref(this)) ?? '';
    });
  }

  set name(String value) => set(gen.Name(value: value));

  /// The id the scene document gives this object, when a scene load
  /// spawned it and told the SDK (it has a script, or a script field
  /// references it); null for what the game made, and inside a prefab
  /// instance.
  String? get authoringId => _authoringId;
  String? _authoringId;
  EntityRef? _entity;
  bool _alive = true;

  // Disposal-only fallback, never live gameplay state.
  Pose? _retiredPose;

  /// Snapshot membership (foundation 4.4, [World._observe]).
  bool _observed = false;
  int _readStep = 0;

  TransformWriter _stage() => TransformWriter.of(localTransform);

  late final Transform transform;

  final List<Behaviour> _behaviours = [];

  /// The behaviours attached to this object, in attach order.
  List<Behaviour> get behaviours =>
      world._gameplay?.behavioursOf(this) ?? List.unmodifiable(_behaviours.where((b) => !b._retired));

  /// Attaches [behaviour]; its `onCreate`/`onEnable` run at the next safe
  /// boundary (once the object is spawned). Returns it for chaining.
  T addBehaviour<T extends Behaviour>(T behaviour) => world._withGameplay(() {
    world._gameplay!.attachBehaviour(this, behaviour);
    return behaviour;
  });

  /// Builds a behaviour of the registered type [typeId] from authored
  /// [fields] (see [BehaviourRegistry.instantiate]) and attaches it atomically.
  Behaviour addBehaviourFromData(String typeId, {Map<String, Object?> fields = const {}, int? version}) =>
      world._withGameplay(
        () => world._gameplay!.guard(
          () => addBehaviour(world.registry.instantiate(typeId, fields: fields, version: version)),
        ),
      );

  /// The first attached behaviour of type [T].
  T? behaviour<T extends Behaviour>() {
    for (final b in behaviours) {
      if (b is T && !b._retired) return b;
    }
    return null;
  }

  Iterable<T> behavioursOf<T extends Behaviour>() => behaviours.where((b) => !b._retired).whereType<T>();

  bool get isPending => _entity == null;
  @override
  bool get isAlive {
    final edit = _gameplay;
    if (edit == null) {
      if (!_alive || _world == null || world.isDisposed) return false;
      final entity = _entity;
      return entity != null && world._native.contains(entity);
    }
    if (!_alive && !edit.pending.containsKey(this)) return false;
    return edit.guard(() => edit.native.contains(edit.ref(this)));
  }

  /// The first component of type [T] on this object (Unity's
  /// `GetComponent<T>()`): the [transform] when [T] is `Transform` (or
  /// `Component`, as in Unity the transform comes first), else the first
  /// attached script of type [T].
  T? getComponent<T extends Component>() {
    final t = transform;
    if (t is T) return t as T;
    for (final b in behaviours) {
      if (b is T && !b._retired) return b as T;
    }
    // A script type never matches a built-in component.
    if (<T>[] is List<Behaviour>) return null;
    for (final codec in gen.componentCodecs.values) {
      if (codec.id == gen.Transform.componentId || codec.id == gen.Name.componentId) {
        continue;
      }
      if (_lookup(codec.id) == null) continue;
      final handle = codec.withType(<X extends WireRecord>() => _handle<X>(codec.id));
      if (handle is T) return handle as T;
    }
    return null;
  }

  /// The built-in component [T] on this object as a live handle
  /// ([NativeComponent], Unity's `GetComponent<Rigidbody>()`), or null
  /// when the object has none. The transform is [transform]; a `Name` is
  /// [name].
  NativeComponent<T>? component<T extends WireRecord>() {
    final codec = _codecOf(T);
    if (codec.id == gen.Transform.componentId || codec.id == gen.Name.componentId) {
      final use = codec.id == gen.Name.componentId ? 'name' : 'transform';
      world._gameplay?.reject('${codec.name} is not a component handle: use $use');
      throw ArgumentError('${codec.name} is not a component handle: use $use');
    }
    return _lookup(codec.id) == null ? null : _handle<T>(codec.id);
  }

  NativeComponent<T> _handle<T extends WireRecord>(int id) =>
      (_handles ??= {})[id] as NativeComponent<T>? ?? (_handles![id] = NativeComponent<T>._(this));
  Map<int, NativeComponent>? _handles;

  /// Runtime handle; throws while pending.
  EntityRef get entity => _requireResolved('entity');

  EntityRef _requireResolved(String what) {
    final edit = world._gameplay;
    if (edit != null) {
      return edit.guard(() {
        if (what != 'entity') edit.reject('$what is not supported inside World.edit');
        final reference = edit.ref(this);
        if (!edit.native.contains(reference)) edit.reject('object was destroyed');
        return reference.live ?? edit.reject('entity is not published until World.edit commits');
      });
    }
    final e = _entity;
    if (e == null) {
      throw StateError('GameObject "$_name" is pending; $what needs the edit to commit');
    }
    if (!_alive) {
      throw StateError('GameObject "$name" (0x${e.toString()}) was destroyed');
    }
    return e;
  }

  /// Destroys this object and its subtree on transaction commit. Lifecycle
  /// callbacks run at the next safe boundary.
  @override
  void destroy() => world._withGameplay(() {
    final edit = world._gameplay!;
    edit.guard(() => edit.native.destroy(edit.ref(this)));
  });

  /// Copies this object's logical subtree, native components and registered
  /// script fields through the gameplay transaction. Internal object and script
  /// references point into the copy; external references and data assets stay
  /// shared. Runtime playback, physics state and script fields not registered
  /// with [BehaviourType] are not copied.
  ///
  /// Without [parent], the root uses this object's logical world pose; with a
  /// parent it keeps the source local pose. [transform] and [name] override
  /// those defaults. Pending sources and staged edits participate in the copy.
  /// An unrepresentable world pose, an expired or foreign object reference,
  /// or a failing field copy aborts the edit. Script membership and enabled
  /// state are captured before factories run; field accessors run within the
  /// edit. Their ordinary Dart side effects cannot be rolled back.
  ///
  /// Inside gameplay callbacks or [World.edit], the result stays pending until
  /// commit and creation callbacks wait for the next lifecycle boundary.
  /// Outside an edit, publication completes before returning; creation callbacks
  /// still wait for the normal lifecycle boundary. Unregistered scripts are omitted with an
  /// `FYN-B0001` warning delivered only after a successful commit.
  GameObject clone({GameObject? parent, Pose? transform, String? name}) =>
      world._withGameplay(() => world._gameplay!.clone(this, parent: parent, transform: transform, name: name));

  /// Reparents atomically, retaining world space when requested. Unrepresentable
  /// local transforms or invalid parent relationships roll back the edit.
  void setParent(GameObject? parent, {bool keepWorld = false}) => world._withGameplay(() {
    final edit = world._gameplay!;
    edit.guard(
      () => edit.native.setParent(edit.ref(this), parent == null ? null : edit.ref(parent), keepWorld: keepWorld),
    );
  });

  GameObject? get _parentObject {
    if (!_alive && world._gameplay == null) return null;
    return world._withGameplay(() {
      final edit = world._gameplay!;
      return edit.guard(() {
        final parent = edit.native.parent(edit.ref(this));
        return parent == null ? null : edit.object(parent);
      });
    });
  }

  /// Current logical parent, including changes in the active edit.
  GameObject? get parent => _parentObject;

  /// Current logical children in sibling order.
  List<GameObject> get children => world._withGameplay(() {
    final edit = world._gameplay!;
    return edit.guard(() => edit.native.children(edit.ref(this)).map(edit.object).toList());
  });

  EntityRef? get parentEntity => parent?.entity;
  List<EntityRef> get childEntities => children.map((child) => child.entity).toList();

  /// This object's own flag; inactive parents do not change it.
  bool get activeSelf {
    if (!_alive && world._gameplay == null) return false;
    return world._withGameplay(() {
      final edit = world._gameplay!;
      return edit.guard(() => edit.native.active(edit.ref(this)).$1);
    });
  }

  /// Current logical activity including every ancestor's flag.
  bool get activeInHierarchy {
    if (!_alive && world._gameplay == null) return false;
    return world._withGameplay(() {
      final edit = world._gameplay!;
      return edit.guard(() => edit.native.active(edit.ref(this)).$2);
    });
  }

  /// Publishes activity atomically. Behaviour callbacks follow at the next
  /// lifecycle boundary; children retain their own flags.
  void setActive(bool active) => world._withGameplay(() {
    final edit = world._gameplay!;
    edit.guard(() => edit.native.setActive(edit.ref(this), active));
  });

  /// The direct child named [name] (scene order), or null; a `/` walks
  /// down: `findChild('Arm/Hand')` is the `Hand` under the `Arm` under
  /// this object. Reads the hierarchy as the last tick left it (see
  /// [World.find]); the result is an attached [GameObject].
  GameObject? findChild(String path) => world._withGameplay(() {
    final edit = world._gameplay!;
    return edit.guard(() {
      var at = this;
      for (final segment in path.split('/').where((s) => s.isNotEmpty)) {
        final child = at.children.where((go) => go.name == segment).firstOrNull;
        if (child == null) return null;
        at = child;
      }
      return at;
    });
  });

  /// The first descendant named [name], depth first in scene order (a
  /// child before its siblings' subtrees), or null. See [findChild].
  GameObject? findDescendant(String name) => world._withGameplay(() {
    final edit = world._gameplay!;
    return edit.guard(() {
      final stack = [...children.reversed];
      while (stack.isNotEmpty) {
        final go = stack.removeLast();
        if (go.name == name) return go;
        stack.addAll(go.children.reversed);
      }
      return null;
    });
  });

  /// The first [T] on this object or below it, depth first in sibling
  /// order (Unity's `GetComponentInChildren`, which also looks at the
  /// object itself): a behaviour type, or a native component type (its
  /// value). Inactive objects and their subtrees are skipped unless
  /// [includeInactive]. This tick's writes count (see [children]).
  T? getInChildren<T extends Object>({bool includeInactive = false}) =>
      _inChildren<T>(includeInactive, first: true).firstOrNull;

  /// Every [T] on this object and below it; see [getInChildren].
  List<T> getAllInChildren<T extends Object>({bool includeInactive = false}) =>
      _inChildren<T>(includeInactive, first: false);

  /// The first [T] on this object or an ancestor, nearest first (Unity's
  /// `GetComponentInParent`); see [getInChildren].
  T? getInParent<T extends Object>({bool includeInactive = false}) => world._withGameplay(() {
    for (GameObject? go = this; go != null; go = go._parentObject) {
      if (!includeInactive && !go._activeForFind) continue;
      final v = go._own<T>();
      if (v != null) return v;
    }
    return null;
  });

  List<T> _inChildren<T extends Object>(bool includeInactive, {required bool first}) => world._withGameplay(() {
    final out = <T>[];
    final stack = <GameObject>[this];
    while (stack.isNotEmpty) {
      final go = stack.removeLast();
      if (!includeInactive && !go._activeForFind) continue;
      for (final v in go._allOwn<T>()) {
        out.add(v);
        if (first) return out;
      }
      stack.addAll(go.children.reversed);
    }
    return out;
  });

  /// The first logical behaviour or native component on this object.
  T? _own<T extends Object>() {
    if (World._isBehaviourType<T>()) {
      for (final b in behaviours) {
        if (b is T && !b._destroyed && !b._retired) return b as T;
      }
      return null;
    }
    if (T == Pose) return localTransform as T;
    return _lookup(_codecOf(T).id) as T?;
  }

  Iterable<T> _allOwn<T extends Object>() sync* {
    if (World._isBehaviourType<T>()) {
      for (final b in behaviours) {
        if (b is T && !b._destroyed && !b._retired) yield b as T;
      }
      return;
    }
    final v = _own<T>();
    if (v != null) yield v;
  }

  /// Active for the find helpers: its own flag as staged this tick, and
  /// its ancestors' as the last tick left them (a pending spawn counts as
  /// active until the tick says otherwise).
  bool get _activeForFind => isAlive && activeInHierarchy;

  // ------------------------------------------------ layer and tags (ADR-0020)

  /// The object's layer, one of the project's `layers` (`default` without
  /// a `fynkel.Layer`). Read-your-writes like [get]. Setting it is a
  /// `fynkel.Layer` write published with the transaction — its colliders change
  /// collision groups in that tick; an unknown name throws.
  String get layer {
    final i = get<gen.Layer>()?.layer ?? 0;
    final layers = world.layersAndTags.layers;
    return i < layers.length ? layers[i] : '#$i';
  }

  set layer(String name) => set(gen.Layer(layer: world._layerIndex(name)));

  /// The object's tags, in the project's `tags` order. Read-your-writes
  /// like [get]; the set is a copy — change the object with [addTag] and
  /// [removeTag].
  Set<String> get tags {
    final bits = _tagBitsNow;
    final names = world.layersAndTags.tags;
    return {
      for (var i = 0; i < 32; i++)
        if (bits & (1 << i) != 0) i < names.length ? names[i] : '#$i',
    };
  }

  /// Whether the object carries [tag]; throws on a tag the project does
  /// not declare.
  bool hasTag(String tag) => _tagBitsNow & world._tagBit(tag) != 0;

  /// Gives the object [tag] through the gameplay transaction; nothing when
  /// it has it already. Throws on a tag the project does not declare.
  void addTag(String tag) {
    final bits = _tagBitsNow, bit = world._tagBit(tag);
    if (bits & bit == 0) set(gen.Tags(bits: bits | bit));
  }

  /// Takes [tag] from the object through the gameplay transaction; nothing
  /// when it lacks it. Throws on a tag the project does not declare.
  void removeTag(String tag) {
    final bits = _tagBitsNow, bit = world._tagBit(tag);
    if (bits & bit != 0) set(gen.Tags(bits: bits & ~bit));
  }

  int get _tagBitsNow => get<gen.Tags>()?.bits ?? 0;

  // ------------------------------------------------ native components (M3-07a)

  static gen.ComponentCodec _codecOf(Type t) {
    final id = gen.componentIds[t];
    if (id == null) {
      (Zone.current[_gameplayZoneKey] as _GameplayAdapter?)?.reject('$t is not a native component');
      throw ArgumentError('$t is not a native component (componentIds lists them)');
    }
    return gen.componentCodecs[id]!;
  }

  /// The native component of type [T], including staged writes in the
  /// current gameplay transaction. Outside a transaction this reads native
  /// authority immediately.
  T? get<T extends WireRecord>() => world._withGameplay(() => world._gameplay!.get<T>(this));

  bool has<T extends WireRecord>() => get<T>() != null;

  /// Every native component in id order, including staged writes.
  List<WireRecord> get components => world._withGameplay(() {
    final edit = world._gameplay!;
    final ids = gen.componentCodecs.keys.toList()..sort();
    return [
      for (final id in ids) ...[edit.lookup(this, id)].whereType<WireRecord>(),
    ];
  });

  WireRecord? _lookup(int id) => world._withGameplay(() => world._gameplay!.lookup(this, id));

  /// Inserts or replaces a component through the gameplay transaction.
  /// Outside a callback this publishes immediately. Invalid writes throw;
  /// inside a callback they abort its transaction.
  void set<T extends WireRecord>(T value) => _setComponent(value, presentation: false);

  /// [set] for readability when the component is new.
  void add<T extends WireRecord>(T value) => set(value);

  /// Writes persistent component data through the shared transaction.
  /// A `Transform` uses [setPresentationTransform] with the same commit boundary.
  void setPresentation<T extends WireRecord>(T value) => _setComponent(value, presentation: true);

  void _setComponent(WireRecord value, {required bool presentation}) {
    if (presentation && value is Pose) {
      setPresentationTransform(value);
      return;
    }
    world._withGameplay(() => world._gameplay!.set(this, value));
  }

  /// Removes the component through the current gameplay transaction, or
  /// publishes immediately outside one. `Transform` and `Name` are required.
  void remove<T extends WireRecord>() => world._withGameplay(
    () => world._gameplay!.guard(() => world._gameplay!.native.remove<T>(world._gameplay!.ref(this))),
  );

  // ------------------------------------------------ rigid body (M3-08)

  /// The body's motion as the last step left it, or null without a
  /// `RigidBody` (one query; not for a hot loop).
  BodyState? bodyState() => world._readSimulation(() {
    final e = _entity;
    if (e == null || !_alive) return null;
    final b = world._native.query(QueryKind.body, entity: e);
    if (b.recordCount == 0) return null;
    return BodyState.fromRecord(gen.BodyStateRecord.decode(b.records, 0, b.tail));
  });

  /// Validates a body request against logical configuration and commits it
  /// for the next simulation tick. Outside an edit, opens an implicit edit;
  /// inside one, joins it. Invalid requests throw immediately and abort the
  /// enclosing edit. Completed physics results change only during simulation.
  void _body(gen.BodyOp op, Vec3 v, [Quat q = Quat.identity]) => world._withGameplay(() {
    world.modules.require(Module.physics, 'body command');
    final edit = world._gameplay!;
    edit.guard(() => edit.native.bodyEffect(edit.ref(this), op, v, q));
  });

  /// Newtons, world space, applied over the next tick (dynamic bodies).
  void applyForce(Vec3 force) => _body(gen.BodyOp.applyForce, force);

  /// Newton-seconds, world space: an instant velocity change (dynamic).
  void applyImpulse(Vec3 impulse) => _body(gen.BodyOp.applyImpulse, impulse);

  /// Newton-metres, world space, over the next tick (dynamic).
  void applyTorque(Vec3 torque) => _body(gen.BodyOp.applyTorque, torque);

  void applyTorqueImpulse(Vec3 impulse) => _body(gen.BodyOp.applyTorqueImpulse, impulse);

  /// Metres per second, world space (dynamic).
  void setLinearVelocity(Vec3 velocity) => _body(gen.BodyOp.setLinearVelocity, velocity);

  /// Radians per second, world space (dynamic).
  void setAngularVelocity(Vec3 velocity) => _body(gen.BodyOp.setAngularVelocity, velocity);

  /// Where a kinematic body should be after the next tick: it moves there
  /// with the velocity that implies, pushing dynamic bodies on the way.
  void setKinematicTarget(Vec3 position, [Quat rotation = Quat.identity]) =>
      _body(gen.BodyOp.setKinematicTarget, position, rotation);

  /// Moves any body instantly (no interpolation, no contacts on the way);
  /// its velocity is kept. On a static body this is a transform write.
  void teleport(Vec3 position, [Quat rotation = Quat.identity]) => _body(gen.BodyOp.teleport, position, rotation);

  /// Walks a `CharacterController` (M3-09) at [velocity] (metres per
  /// second, world space, the vertical part ignored) over the next tick;
  /// no `move` in a tick is standing still. Rejected immediately on an
  /// object without a controller. A controller with `moveAction` set is
  /// walked by the runtime already; a `move` in the same tick wins. A newly
  /// created controller first places its capsule in physics; explicit move/jump
  /// requests survive that setup tick and run when it is ready.
  void move(Vec3 velocity) => _body(gen.BodyOp.move, velocity);

  /// Starts a jump at [speed] metres per second upward (the controller's
  /// `jumpSpeed` when 0): the caller decides when — typically on a
  /// pressed edge while [characterState] says `grounded`.
  void jump([double speed = 0]) => _body(gen.BodyOp.jump, Vec3(0, speed, 0));

  /// The character's state after the last tick (`FYNKEL_QUERY_CHARACTER`);
  /// `null` without a `CharacterController` or before it spawned.
  CharacterState? characterState() => world._readSimulation(() {
    final e = _entity;
    if (e == null || !_alive) return null;
    final b = world._native.query(QueryKind.character, entity: e);
    if (b.recordCount == 0) return null;
    return CharacterState.fromRecord(gen.CharacterStateRecord.decode(b.records, 0, b.tail));
  });

  // ------------------------------------------------- animation (M3-19)

  /// Animation requests validate through the gameplay transaction and execute
  /// at the next simulation tick. [AnimationControl.state] reports completed
  /// playback. A missing animator rejects immediately, including outside edits.
  AnimationControl get animation => _animation ??= AnimationControl._(this);
  AnimationControl? _animation;

  void _animate(
    gen.AnimateOp op, {
    AssetRef clip = AssetRef.none,
    double seconds = 0,
    double speed = 1,
    bool looping = true,
    double value = 0,
    String name = '',
    int integer = 0,
    int layer = -1,
    Vec3 point = Vec3.zero,
  }) {
    world._withGameplay(() {
      world.modules.require(Module.animation, 'animation control');
      final edit = world._gameplay!;
      edit.guard(
        () => edit.native.animateControl(
          edit.ref(this),
          gen.AnimateCommandRecord(
            entity: EntityRef.nil,
            op: op,
            seconds: seconds,
            speed: speed,
            looping: looping,
            clip: clip,
            value: value,
            name: name,
            integer: integer,
            layer: layer,
            point: point,
          ),
        ),
      );
    });
  }

  // ------------------------------------------------- audio (M3-11)

  /// Audio requests validate through the gameplay transaction and execute at
  /// the next simulation tick. [AudioControl.state] reports completed playback.
  /// A missing source rejects immediately, including outside edits.
  AudioControl get audio => _audioControl ??= AudioControl._(this);
  AudioControl? _audioControl;

  void _audio(gen.AudioOp op, {double value = 0}) => world._withGameplay(() {
    world.modules.require(Module.audio, 'audio control');
    final edit = world._gameplay!;
    edit.guard(() => edit.native.audioControl(edit.ref(this), op, value));
  });

  /// Current logical local pose, including writes in the active edit.
  Pose get localTransform {
    if (world.isDisposed) return _retiredPose ?? _initial;
    return world._withGameplay(() {
      final pose = world._gameplay!.get<Pose>(this)!;
      world._observe(this);
      return pose;
    });
  }

  /// Safe fallback for disposal when native state cannot be queried.
  Pose get _lastPose => _retiredPose ?? world._snapshotOf(_entity ?? EntityRef.nil)?.local ?? _initial;

  /// Current logical world pose; null for a destroyed object.
  Pose? get worldTransform => !_alive ? null : world._withGameplay(() => transform.world);

  void setLocalTransform(Pose pose) => world._withGameplay(() => world._gameplay!.set(this, pose));
  void writeLocal(TransformWriter writer) => setLocalTransform(writer.toTransform());

  /// Partial writes preserve the current logical values of other fields.
  void setLocalPosition(double x, double y, double z) =>
      world._withGameplay(() => _submitLocal(_stage().setPosition(x, y, z)));
  void setLocalRotation(double x, double y, double z, double w) =>
      world._withGameplay(() => _submitLocal(_stage().setRotation(x, y, z, w)));
  void setLocalRotationAxisAngle(double ax, double ay, double az, double radians) =>
      world._withGameplay(() => _submitLocal(_stage().setRotationAxisAngle(ax, ay, az, radians)));
  void setLocalScale(double x, double y, double z) =>
      world._withGameplay(() => _submitLocal(_stage().setScale(x, y, z)));
  void _submitLocal(TransformWriter writer) => setLocalTransform(writer.toTransform());

  void _setPresentationInEdit(Pose pose) {
    final edit = world._gameplay!;
    if (edit.get<gen.RigidBody>(this)?.bodyType == gen.BodyType.dynamic) {
      edit.reject('presentation transform is physics-owned; use a gameplay transform write');
    }
    edit.set(this, pose);
  }

  /// Persistent presentation placement through the logical transaction.
  /// This changes gameplay data and survives later frames. There is no
  /// transient render-only transform layer; use a separate visual child
  /// for independently animated graphics.
  /// Dynamic bodies require an ordinary gameplay transform write.
  void setPresentationTransform(Pose pose) => world._withGameplay(() => _setPresentationInEdit(pose));
  void writePresentation(TransformWriter writer) => setPresentationTransform(writer.toTransform());
  void setPresentationPosition(double x, double y, double z) =>
      world._withGameplay(() => _submitPresentation(_stage().setPosition(x, y, z)));
  void setPresentationRotation(double x, double y, double z, double w) =>
      world._withGameplay(() => _submitPresentation(_stage().setRotation(x, y, z, w)));
  void _submitPresentation(TransformWriter writer) => setPresentationTransform(writer.toTransform());

  @override
  String toString() => _spec != null
      ? 'GameObject($name, the object of $_spec)'
      : 'GameObject($name, ${_entity == null ? 'pending' : '0x${_entity!.toString()}'})';
}

/// Property-style access with read-your-writes semantics.
/// Which axes [Transform.translate] and [Transform.rotate] use:
/// the object's own ([self], Unity's `Space.Self`) or the world's.
enum Space { self, world }

/// An object's transform the way a Unity script reads it (docs/scripting.md
/// § The transform): [position] and [rotation] are in the **world**,
/// [localPosition], [localRotation] and [localScale] relative to the
/// parent — the same for a root object. Reads use the current logical hierarchy,
/// including staged ancestor writes. Mutations join the active gameplay
/// transaction or open one that commits before the call returns.
class Transform extends Component {
  Transform._(this._go);
  final GameObject _go;

  @override
  GameObject get gameObject => _go;

  @override
  Transform get transform => this;

  @override
  bool get isAlive => _go.isAlive;

  /// Refused, as in Unity: destroy the [gameObject] instead.
  @override
  void destroy() {
    _go.world._rejectDuringEdit('destroy Transform');
    throw UnsupportedError('a transform cannot be destroyed; destroy its GameObject ("$name")');
  }

  /// The local transform, whole.
  Pose get value => _go.localTransform;
  set value(Pose t) => _go.setLocalTransform(t);

  /// The parent's transform; null for a root.
  Transform? get parent => _go._parentObject?.transform;

  Vec3 get localPosition => value.position;
  set localPosition(Vec3 p) =>
      _editOperation(() => value = Pose(position: p, rotation: value.rotation, scale: value.scale));

  Quat get localRotation => value.rotation;
  set localRotation(Quat q) =>
      _editOperation(() => value = Pose(position: value.position, rotation: q, scale: value.scale));

  Vec3 get localScale => value.scale;
  set localScale(Vec3 s) =>
      _editOperation(() => value = Pose(position: value.position, rotation: value.rotation, scale: s));

  /// [localScale] (kept from before world positions: Unity has no
  /// settable world scale either — [lossyScale] reads it).
  Vec3 get scale => localScale;
  set scale(Vec3 s) => localScale = s;

  /// Exact local-to-world affine matrix, including staged ancestor edits.
  Affine3 get localToWorldMatrix => _go.world._withGameplay(() {
    final matrix = _go.world._gameplay!.matrix(_go);
    _go.world._observe(_go);
    return matrix;
  });

  /// Exact inverse; throws [StateError] for a singular transform.
  Affine3 get worldToLocalMatrix => localToWorldMatrix.inverse;

  /// Convenient world pose. Position is exact; rotation follows authored
  /// quaternion composition and scale contains basis lengths. A TRS pose
  /// cannot reproduce hierarchy shear; use [localToWorldMatrix] for geometry.
  Pose get world => _editOperation(() => Pose(position: position, rotation: rotation, scale: lossyScale));

  /// World position.
  Vec3 get position => localToWorldMatrix.translation;
  set position(Vec3 p) => _editOperation(() {
    final parent = this.parent;
    localPosition = parent == null ? p : parent.inverseTransformPoint(p);
  });

  T _editOperation<T>(T Function() operation) => _go.world._withGameplay(() => _go.world._gameplay!.guard(operation));

  /// Authored world orientation, composed from local quaternions without
  /// scale. Under shear this is not a decomposition of the world matrix.
  Quat get rotation => _editOperation(() {
    var result = localRotation;
    for (var p = parent; p != null; p = p.parent) {
      result = p.localRotation * result;
    }
    return result;
  });

  set rotation(Quat q) => _editOperation(() {
    final parent = this.parent;
    localRotation = parent == null ? q : parent.rotation.inverse * q;
  });

  /// World scale, read only.
  Vec3 get lossyScale => localToWorldMatrix.lossyScale;

  /// [rotation] as Euler degrees ([Quat.euler]), what the Inspector shows
  /// for a root object; [localEulerAngles] for the local one.
  Vec3 get eulerAngles => rotation.eulerAngles;
  set eulerAngles(Vec3 degrees) => rotation = Quat.euler(degrees);

  Vec3 get localEulerAngles => localRotation.eulerAngles;
  set localEulerAngles(Vec3 degrees) => localRotation = Quat.euler(degrees);

  /// The object's own axes in the world. Forward is [Vec3.forward], −Z
  /// (docs/unity-parity.md § On purpose); setting it turns the object to
  /// face that way with its up kept near the world's.
  Vec3 get forward => rotation.rotate(Vec3.forward);
  set forward(Vec3 direction) => rotation = Quat.lookRotation(direction);
  Vec3 get right => rotation.rotate(Vec3.right);
  Vec3 get up => rotation.rotate(Vec3.up);

  /// Moves by [translation]: along the object's own axes ([Space.self],
  /// Unity's default; its scale does not count) or the world's.
  void translate(Vec3 translation, {Space relativeTo = Space.self}) => _editOperation(() {
    position += relativeTo == Space.self ? rotation.rotate(translation) : translation;
  });

  /// Turns by Euler [degrees] ([Quat.euler]) about the object's own axes
  /// ([Space.self], Unity's default) or the world's.
  void rotate(Vec3 degrees, {Space relativeTo = Space.self}) => _editOperation(() {
    final turn = Quat.euler(degrees);
    if (relativeTo == Space.self) {
      localRotation = localRotation * turn;
    } else {
      rotation = turn * rotation;
    }
  });

  /// Turns by [degrees] about the world [axis] through [point], moving
  /// the object around it.
  void rotateAround(Vec3 point, Vec3 axis, double degrees) => _editOperation(() {
    final turn = Quat.axisAngle(axis.normalized, degrees * math.pi / 180);
    final w = world;
    value = Pose(
      position: _localPoint(point + turn.rotate(w.position - point)),
      rotation: _localRotation(turn * w.rotation),
      scale: localScale,
    );
  });

  /// Turns the object so its forward points at [target] (a world point)
  /// with its up as near [worldUp] as it can be.
  void lookAt(Vec3 target, {Vec3 worldUp = Vec3.up}) => _editOperation(() {
    final d = target - position;
    if (d.lengthSquared < 1e-12) return;
    rotation = Quat.lookRotation(d, worldUp);
  });

  /// [lookAt] another object's world position.
  void lookAtObject(GameObject target, {Vec3 worldUp = Vec3.up}) => lookAt(target.transform.position, worldUp: worldUp);

  // ---- tweens (ADR-0026 §7, tween.dart)

  /// Moves the object to the world [position] over [seconds] of game time
  /// (DOTween's `DOMove`), from where it is now, eased by [ease]. Returns
  /// the running tween: yield it in a coroutine, `await` its `done`, or
  /// `stop()` it (the object stays where it got to). It ends early when
  /// the object is destroyed. It writes the transform every frame, so it
  /// is for objects physics does not move.
  Coroutine moveTo(Vec3 position, double seconds, {Ease ease = Ease.outQuad, TimeClock clock = TimeClock.gameplay}) {
    final from = this.position;
    return _go.world._tweenOn(_go, seconds, ease, (t) => this.position = from.lerp(position, t), clock);
  }

  /// [moveTo] in the parent's space (DOTween's `DOLocalMove`).
  Coroutine moveLocalTo(
    Vec3 position,
    double seconds, {
    Ease ease = Ease.outQuad,
    TimeClock clock = TimeClock.gameplay,
  }) {
    final from = localPosition;
    return _go.world._tweenOn(_go, seconds, ease, (t) => localPosition = from.lerp(position, t), clock);
  }

  /// Turns the object to the world [rotation] along the shorter arc
  /// (DOTween's `DORotateQuaternion`); see [moveTo].
  Coroutine rotateTo(Quat rotation, double seconds, {Ease ease = Ease.outQuad, TimeClock clock = TimeClock.gameplay}) {
    final from = this.rotation;
    return _go.world._tweenOn(_go, seconds, ease, (t) => this.rotation = from.slerp(rotation, t), clock);
  }

  /// Scales the object to the local [scale] (DOTween's `DOScale`); see
  /// [moveTo].
  Coroutine scaleTo(Vec3 scale, double seconds, {Ease ease = Ease.outQuad, TimeClock clock = TimeClock.gameplay}) {
    final from = localScale;
    return _go.world._tweenOn(_go, seconds, ease, (t) => localScale = from.lerp(scale, t), clock);
  }

  /// A point in this object's space, in the world, including scale and shear.
  Vec3 transformPoint(Vec3 p) => localToWorldMatrix.transformPoint(p);

  /// A world point in local space; throws for singular transforms.
  Vec3 inverseTransformPoint(Vec3 p) => worldToLocalMatrix.transformPoint(p);

  /// A displacement in world space, including scale and shear, without translation.
  Vec3 transformVector(Vec3 v) => localToWorldMatrix.transformVector(v);

  /// A world displacement in local space; throws for singular transforms.
  Vec3 inverseTransformVector(Vec3 v) => worldToLocalMatrix.transformVector(v);

  /// A direction in this object's space, in the world (rotation only).
  Vec3 transformDirection(Vec3 d) => rotation.rotate(d);

  /// A world direction, in this object's space (rotation only).
  Vec3 inverseTransformDirection(Vec3 d) => rotation.inverse.rotate(d);

  Vec3 _localPoint(Vec3 p) => parent?.inverseTransformPoint(p) ?? p;
  Quat _localRotation(Quat q) {
    final parent = this.parent;
    return parent == null ? q : parent.rotation.inverse * q;
  }
}

/// The world's clock as a [TimeService].
class _WorldTime implements TimeService {
  _WorldTime(this.world);
  final World world;

  @override
  double get delta => world._inFixedPhase ? world.fixedDt : world._frameDt;
  @override
  double get unscaledDelta => world._unscaledDt;
  @override
  double get timeScale => world.timeScale;
  @override
  set timeScale(double value) => world.timeScale = value;
  @override
  double get fixedDelta => world.fixedDt;
  @override
  int get tick => world.tick;
  @override
  double get elapsed => world._gameplayElapsed;
  @override
  double get realElapsed => world._realElapsed;
  @override
  double get simulationElapsed => world._native.simulationElapsed;
  @override
  int get frame => world._frameCount;
  @override
  double get alpha => world.alpha;
}

/// Where one [World.frame] spent its time (M3-16, blueprint §15.4), in
/// microseconds: [behavioursUs] is the Dart side — every `fixedUpdate`,
/// `afterPhysics`, `update`/`lateUpdate` and the host's own callbacks;
/// [nativeUs] the engine — `beginFrame`, the input capture, every `step`
/// (the staged commands flushed, the tick, the snapshot indexed, the
/// diagnostics read) and the event drains; [presentationUs] the
/// presentation-lane flush; [totalUs] the whole call (the rest is the
/// SDK's own event decoding). [ticks] is the frame's fixed ticks.
class FrameTimes {
  int behavioursUs = 0, nativeUs = 0, presentationUs = 0, totalUs = 0, ticks = 0;

  final Stopwatch _behaviours = Stopwatch(), _native = Stopwatch(), _total = Stopwatch();

  void _begin() {
    _behaviours.reset();
    _native.reset();
    _total
      ..reset()
      ..start();
  }

  void _end(int steps) {
    _total.stop();
    behavioursUs = _behaviours.elapsedMicroseconds;
    nativeUs = _native.elapsedMicroseconds;
    presentationUs = 0; // No separately flushed presentation command phase.
    totalUs = _total.elapsedMicroseconds;
    ticks = steps;
  }

  @override
  String toString() =>
      'FrameTimes(total=$totalUs µs behaviours=$behavioursUs µs native=$nativeUs µs presentation=$presentationUs µs ticks=$ticks)';
}

/// Generated codecs (ADR-0035): an object reference field on the wire —
/// a published object's handle. Pending references use the gameplay journal's
/// scoped reference codec and cannot escape into the legacy wire format. `0` is None.
EntityRef objectRefBits(GameObject? go) => go == null ? EntityRef.nil : go.entity;

/// Generated codecs (ADR-0035): the object a handle names, in the world
/// decoding the component. The native side reads a destroyed target as
/// `0`, so a reference to one is null, as in Unity.
GameObject? objectRefOf(EntityRef bits) {
  if (bits.isNull) return null;
  final w = World._decoding;
  if (w == null) return null;
  return w.byEntity(bits) ?? w.attach(bits);
}
