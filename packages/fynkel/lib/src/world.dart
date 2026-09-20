// The Dart gameplay API (design/dart-sdk.md §21.2, ADR-0008): World,
// GameObject, TransformRef with a staged-write overlay, and (behaviour.dart)
// the Behaviour lifecycle the world drives at the §5.1 points of a host
// frame. Scenes come from the cooked cache through `loadProject`/`loadScene`.

import 'dart:typed_data';

import '../generated/schema.g.dart' as gen;
import '../math.dart';
import 'context.dart';
import 'diagnostics.dart';
import 'events.dart';
import 'input.dart';
import 'native/bridge.dart';
import 'transform_writer.dart';
import 'wire/buffers.dart';
import 'wire/support.dart' show WireRecord;

export 'native/bridge.dart' show BridgeStats, CommandLane, FramePlan, StepResult;

part 'behaviour.dart';

/// Immutable local transform value (position, rotation, scale).
typedef Transform = gen.Transform;

const Transform identityTransform = Transform(position: Vec3.zero, rotation: Quat.identity, scale: Vec3.one);

/// Runtime entity/component container over one native world.
class World {
  World._(this._lib, this._native, {required this.ownsLibrary, required this.fixedDt, BehaviourRegistry? registry})
      : registry = registry ?? BehaviourRegistry() {
    input = WorldInput(() => _native.inputView);
    context = GameContext(time: _WorldTime(this), input: input);
    _behaviours = _BehaviourSystem(this);
  }

  /// Creates a world on a fresh [NativeLibrary] (or the given one).
  factory World.create({
    NativeLibrary? library,
    int worldId = 0,
    double fixedDt = 0,
    int maxStepsPerFrame = 0,
    double maxFrameDt = 0,
    BehaviourRegistry? registry,
  }) {
    final lib = library ?? NativeLibrary.open();
    final native = lib.createWorld(
        worldId: worldId, fixedDt: fixedDt, maxStepsPerFrame: maxStepsPerFrame, maxFrameDt: maxFrameDt);
    return World._(lib, native, ownsLibrary: library == null, fixedDt: fixedDt > 0 ? fixedDt : 1 / 60, registry: registry);
  }

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

  late final _BehaviourSystem _behaviours;

  /// Every attached behaviour, in callback order.
  List<Behaviour> get behaviours => List.unmodifiable(_behaviours._all);

  /// Host frames begun and the last frame's dt (for [TimeService]).
  int _frameCount = 0;
  double _frameDt = 0;

  final NativeLibrary _lib;
  final NativeWorld _native;
  final bool ownsLibrary;

  /// Low-level access for hosts and tests.
  NativeWorld get native => _native;

  final CommandBatch _simulation = CommandBatch();
  final CommandBatch _presentation = CommandBatch();

  /// Command index → GameObject for overlay clearing on rejection.
  final _Targets _simulationTargets = _Targets();
  final _Targets _presentationTargets = _Targets();

  final Map<int, GameObject> _byEntity = {};
  final Map<int, GameObject> _pendingByTempId = {};
  int _nextTempId = 1;

  /// Last snapshot: entity → record (read in place, decoded on access).
  WireBuffer _snapshot = WireBuffer.empty;
  final Map<int, int> _snapshotIndex = {};

  final List<int> _toSubscribe = [];

  /// Called with the diagnostics of each step/submit in release mode
  /// (debug mode throws [FynkelError] instead). Defaults to `print`.
  void Function(List<FynkelDiagnostic>) onDiagnostics = (d) => print(d.join('\n'));

  /// Throw [FynkelError] on error diagnostics (rejections, protocol errors).
  /// Informational ones ([FynkelDiagnostic.isInformational], e.g. a missed
  /// deadline) always go to [onDiagnostics]. Defaults to [isDebugMode].
  bool throwOnDiagnostics = isDebugMode;

  int get tick => _native.tick;
  double get alpha => _native.alpha;
  bool get isPoisoned => _native.isPoisoned;
  Iterable<GameObject> get objects => _byEntity.values;

  // ---------------------------------------------------------------- spawn

  /// Queues a spawn for the next fixed tick. The returned handle is
  /// [GameObject.isPending] until that tick resolves it; writes to a pending
  /// object throw, reads return the initial transform.
  GameObject spawn({Transform transform = identityTransform, GameObject? parent, String name = ''}) {
    if (parent != null) parent._requireResolved('spawn parent');
    final tempId = _nextTempId++;
    final go = GameObject._(this, tempId, transform, name);
    _pendingByTempId[tempId] = go;
    _simulation.spawn(gen.SpawnCommand(tempId: tempId, transform: transform, parent: parent?._entity, name: name));
    _simulationTargets.add(go);
    return go;
  }

  /// Looks a resolved object up by handle.
  GameObject? byEntity(int entity) => _byEntity[entity];

  // ---------------------------------------------------------------- frame

  /// Starts a host frame; run [step] `plan.steps` times, then presentation
  /// writes, then extract/render.
  FramePlan beginFrame(double dtSeconds) {
    _presentation.clear();
    _presentationTargets.clear();
    _frameCount++;
    _frameDt = dtSeconds;
    final plan = _native.beginFrame(dtSeconds);
    if (plan.missedDeadline) _collectDiagnostics('begin_frame');
    return plan;
  }

  /// Hands this frame's raw control events to the world (§5.1): once per
  /// frame after [beginFrame], before the ticks. Held state is immediate;
  /// `pressed`/`released` edges wait for the next tick. [frame] does this
  /// for its `input` argument; hosts with their own loop call it directly.
  void captureInput(List<InputEvent> events) {
    // A frame without events after a frame without events changes nothing
    // (no relative control to zero, no frame edge to clear): skip the call.
    if (events.isEmpty && !_inputDirty) return;
    _inputDirty = events.isNotEmpty;
    _inputEvents.clear();
    for (final e in events) {
      _inputEvents.add(e.toRecord());
    }
    final s = _native.captureInputBytes(_inputEvents.backingStore, _inputEvents.finalize());
    _inputEvents.clear();
    _checkFatal(s, 'captureInput');
  }

  /// One fixed tick: flushes staged simulation commands, steps the native
  /// world, resolves spawns, refreshes the snapshot, reports diagnostics.
  StepResult step() {
    _flush(CommandLane.simulation, _simulation, _simulationTargets, 'submit');
    final r = _native.step();
    _checkFatal(r.status, 'step');
    _snapshot = _native.snapshot;
    // entity → record index. Overwrite in place (no growth in steady state,
    // M1 gate); rebuild only when the subscribed set changed size, which is
    // the only way a stale key can remain.
    final d = _snapshot.records;
    final n = _snapshot.recordCount;
    if (_snapshotIndex.length != n) _snapshotIndex.clear();
    for (var i = 0; i < n; i++) {
      _snapshotIndex[d.getUint64(i * gen.SnapshotTransformRecord.wireSize, Endian.little)] = i;
    }
    if (_snapshotIndex.length != n) {
      // Same count, different members: rebuild once.
      _snapshotIndex.clear();
      for (var i = 0; i < n; i++) {
        _snapshotIndex[d.getUint64(i * gen.SnapshotTransformRecord.wireSize, Endian.little)] = i;
      }
    }
    // Overlays are consumed by the tick they were staged for.
    for (final go in _byEntity.values) {
      go._clearOverlay();
    }
    _collectDiagnostics('step');
    return r;
  }

  /// Pull-only event drain; resolves spawn/destroy bookkeeping on the way.
  List<EngineEvent> drainEvents() {
    final buffer = _native.drainEvents();
    if (buffer.recordCount == 0) return const [];
    final events = decodeEvents(buffer).toList();
    for (final e in events) {
      switch (e) {
        case EntitySpawned(:final tempId, :final entity):
          final go = _pendingByTempId.remove(tempId);
          if (go != null) {
            go._entity = entity;
            _byEntity[entity] = go;
            _toSubscribe.add(entity);
            go._flushPendingComponents();
            if (go._behaviours.isNotEmpty) _behaviours.objectChanged();
          }
        case EntityDestroyed(:final entity):
          final go = _byEntity.remove(entity);
          if (go != null) {
            go._alive = false;
            if (go._behaviours.isNotEmpty) _behaviours.objectChanged();
          }
        default:
          break;
      }
    }
    if (_toSubscribe.isNotEmpty) {
      _native.subscribeTransforms(_toSubscribe);
      _toSubscribe.clear();
    }
    return events;
  }

  /// Flushes presentation-lane writes (call after lateUpdate, before
  /// extraction). [step] does not flush this lane.
  void flushPresentation() => _flush(CommandLane.presentation, _presentation, _presentationTargets, 'presentation');

  /// The host loop (§5.1) for tests, headless runs and hosts that need no
  /// custom phases: begin the frame; per planned tick run every behaviour's
  /// `fixedUpdate` (and [fixedUpdate]) with the non-physics events drained
  /// since the last tick, `step`, drain the tick's events and run
  /// `afterPhysics` with its contacts for behaviours that opted in; then
  /// `update`/`lateUpdate` (and [update]) and the presentation flush.
  /// [fixedUpdate] runs before the behaviours, [update] after them. Every
  /// non-physics event reaches exactly one `fixedUpdate`. [input] is the
  /// frame's raw control events ([captureInput]): each tick's callbacks
  /// see the edges that tick consumes (a catch-up tick sees held state
  /// only), `afterPhysics` the same tick's, `update`/`lateUpdate` the
  /// frame's raw edges.
  FramePlan frame(double dtSeconds,
      {List<InputEvent> input = const [], void Function(int tick)? fixedUpdate, void Function()? update}) {
    final plan = beginFrame(dtSeconds);
    captureInput(input);
    _carry(drainEvents());
    for (var i = 0; i < plan.steps; i++) {
      this.input.refresh();
      fixedUpdate?.call(tick);
      _behaviours.fixedUpdate(tick, _takeCarried());
      final stepped = tick;
      step();
      final events = drainEvents();
      _behaviours.afterPhysics(stepped, _contacts(events));
      _carry(events);
    }
    this.input.refresh(frameEdges: true);
    _behaviours.update(dtSeconds);
    update?.call();
    flushPresentation();
    return plan;
  }

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
    return [for (final e in events) if (e is Contact) e];
  }

  /// A handle for an entity that already exists (loaded from a scene, or
  /// found by a query); the same handle comes back for the same entity.
  /// Its transform is subscribed from the next tick.
  GameObject attach(int entity, {String name = ''}) {
    final existing = _byEntity[entity];
    if (existing != null) return existing;
    final go = GameObject._(this, 0, identityTransform, name.isEmpty ? (nameOfEntity(entity) ?? '') : name);
    go._entity = entity;
    _byEntity[entity] = go;
    _toSubscribe.add(entity);
    return go;
  }

  void _flush(CommandLane lane, CommandBatch batch, _Targets targets, String context) {
    if (batch.isEmpty) return;
    final r = _native.submitBytes(lane, batch.backingStore, batch.finalize());
    batch.clear();
    _checkFatal(r.status, context);
    if (lane == CommandLane.presentation) {
      targets.clear();
      _collectDiagnostics(context);
    }
    // Simulation-lane targets stay until the step reports on them.
  }

  void _checkFatal(FynkelStatus s, String op) {
    switch (s) {
      case FynkelStatus.FYNKEL_STATUS_OK:
      case FynkelStatus.FYNKEL_STATUS_COMMANDS_REJECTED:
        return;
      default:
        throw FynkelBridgeError('$op: $s');
    }
  }

  void _collectDiagnostics(String context) {
    final b = _native.diagnostics;
    if (b.recordCount == 0) return;
    final list = b
        .fixed(gen.DiagnosticRecord.wireSize, gen.DiagnosticRecord.decode)
        .map(FynkelDiagnostic.fromRecord)
        .toList(growable: false);
    // A rejected command clears its target's overlay (§5.4).
    final targets = context == 'step' ? _simulationTargets : _presentationTargets;
    for (final d in list) {
      if (d.isCommandRejection && d.commandIndex < targets.length) {
        targets[d.commandIndex]?._clearOverlay();
      }
      if (d.entity != 0) _byEntity[d.entity]?._clearOverlay();
    }
    if (context == 'step') _simulationTargets.clear();
    if (throwOnDiagnostics && list.any((d) => !d.isInformational)) {
      throw FynkelError(list, context: context);
    }
    onDiagnostics(list);
  }

  // ---------------------------------------------------------------- reads

  /// Seeds [w] with the entity's last-snapshot local transform straight
  /// from the record bytes (no value objects); false when not subscribed.
  bool _seedLocal(int entity, TransformWriter w) {
    final i = _snapshotIndex[entity];
    if (i == null) return false;
    w.decode(_snapshot.records, i * gen.SnapshotTransformRecord.wireSize + 8);
    return true;
  }

  gen.SnapshotTransformRecord? _snapshotOf(int entity) {
    final i = _snapshotIndex[entity];
    if (i == null) return null;
    return gen.SnapshotTransformRecord.decode(_snapshot.records, i * gen.SnapshotTransformRecord.wireSize, _snapshot.tail);
  }

  /// Explicit queries (one FFI call each; not for hot paths).
  List<int> childrenOf(GameObject go) => _native.query(QueryKind.children, entity: go._requireResolved('children')).u64s();

  int? parentOf(GameObject go) {
    final p = _native.query(QueryKind.parent, entity: go._requireResolved('parent')).u64s();
    return p.isEmpty || p[0] == 0 ? null : p[0];
  }

  String? nameOf(GameObject go) => nameOfEntity(go._requireResolved('name'));

  String? nameOfEntity(int entity) {
    final b = _native.query(QueryKind.name, entity: entity);
    if (b.recordCount == 0) return null;
    return gen.Name.decode(b.records, 0, b.tail).value;
  }

  List<int> allEntities() => _native.query(QueryKind.allEntities).u64s();

  /// Every native component of [entity] as the last step left it (one
  /// query; `Transform`, `Name` and the renderable components in id order,
  /// decoded through [gen.componentCodecs]). [GameObject.get] is the
  /// read-your-writes view over this.
  List<WireRecord> componentsOf(int entity) {
    final out = <WireRecord>[];
    _decodeComponents(_native.query(QueryKind.components, entity: entity), (_, v) => out.add(v));
    return out;
  }

  /// Walks a `FYNKEL_QUERY_COMPONENTS` response: a `ComponentHeader` then
  /// the body per component; a component this build has no codec for is
  /// skipped by its size.
  void _decodeComponents(WireBuffer b, void Function(int id, WireRecord value) sink) {
    final d = b.records;
    var off = 0;
    for (var i = 0; i < b.recordCount; i++) {
      final id = d.getUint32(off, Endian.little);
      final size = d.getUint32(off + 4, Endian.little);
      off += gen.ComponentHeader.wireSize;
      final codec = gen.componentCodecs[id];
      if (codec != null && codec.wireSize <= size) sink(id, codec.decode(d, off, b.tail));
      off += size;
    }
  }

  // ---------------------------------------------------------------- documents

  /// Where `fynkel cook` writes a project's cache: `<dir>/.fynkel/cache`.
  static String cacheDirOf(String projectDir) => '$projectDir/.fynkel/cache';

  /// Loads the startup scene of the project at [dir] from its cooked cache
  /// ([cacheDir], default [cacheDirOf]). The runtime reads the cache only:
  /// `fynkel cook` (or `fynkel run`, `fynkel test`, which cook first) must
  /// have run for this build, or the load fails with `FYN-R0001`/`R0002`.
  /// Entities exist immediately; components go through the command path
  /// (rejections reach [onDiagnostics] on the next step). The scene's
  /// assets stay resident in the world's store — a viewport rendering this
  /// world draws from it — until the world is disposed. Throws
  /// [FynkelLoadError] when the cache or the scene has errors.
  LoadReport loadProject(String dir, {String? cacheDir}) {
    final cache = cacheDir ?? cacheDirOf(dir);
    return _load(_native.loadProject(cache), 'loadProject').withProjectDir(dir);
  }

  /// Loads the cooked scene with asset id [sceneId] (UUID text) from the
  /// cache at [cacheDir]; see [loadProject]. Throws [FynkelLoadError] when
  /// the scene is not in the cache (`FYN-R0003`) or does not load.
  LoadReport loadScene(String cacheDir, String sceneId) => _load(_native.loadScene(cacheDir, sceneId), 'loadScene');

  LoadReport _load((FynkelStatus, Map<String, Object?>) r, String op) {
    final (status, json) = r;
    var report = LoadReport.fromJson(json);
    switch (status) {
      case FynkelStatus.FYNKEL_STATUS_OK:
        input.invalidate();
        report = _attachAuthoredBehaviours(report);
        if (report.warnings.isNotEmpty) onLoadDiagnostics(report.warnings.toList(growable: false));
        return report;
      case FynkelStatus.FYNKEL_STATUS_LOAD_FAILED:
        throw FynkelLoadError(report, context: op);
      default:
        throw FynkelBridgeError('$op: $status');
    }
  }

  /// Instantiates every authored behaviour of a loaded scene from
  /// [registry] and attaches it to its spawned entity (M2-16: the scene
  /// component `game.Spinner` with `speed: 2.5` becomes a `Spinner` with
  /// that field set). A type the registry lacks is `FYN-B0001`, data the
  /// type rejects is `FYN-B0002`; both are warnings — the entity and its
  /// other components are loaded regardless.
  LoadReport _attachAuthoredBehaviours(LoadReport report) {
    if (report.behaviours.isEmpty) return report;
    final extra = <LoadDiagnostic>[];
    var attached = 0;
    for (final b in report.behaviours) {
      if (!registry.contains(b.type)) {
        extra.add(LoadDiagnostic(
          code: 'FYN-B0001',
          severity: 'warning',
          message: 'behaviour type ${b.type} is not registered; the component is loaded without a behaviour',
          document: report.scenePath,
          pointer: b.pointer,
          suggestion: 'call registerBehaviours(world.registry) from the generated behaviours.g.dart',
        ));
        continue;
      }
      try {
        attach(b.entity).addBehaviourFromData(b.type, fields: b.data, version: b.version);
        attached++;
      } on BehaviourDataError catch (e) {
        extra.add(LoadDiagnostic(
          code: 'FYN-B0002',
          severity: 'warning',
          message: '${b.type}${e.field == null ? '' : '.${e.field}'}: ${e.message}',
          document: report.scenePath,
          pointer: b.pointer == null ? null : '${b.pointer}/data${e.field == null ? '' : '/${e.field}'}',
          suggestion: 'fix the component data or regenerate behaviours.g.dart (`fynkel codegen --project`)',
        ));
      }
    }
    return report.withBehaviours(attached: attached, extra: extra);
  }

  /// Called with the warnings of a successful load. Defaults to `print`.
  void Function(List<LoadDiagnostic>) onLoadDiagnostics = (d) => print(d.join('\n'));

  BridgeStats stats() => _native.stats();

  void dispose() {
    _native.dispose();
    if (ownsLibrary) _lib.dispose();
  }
}

/// Stable handle to an entity. Owns no native memory.
class GameObject {
  GameObject._(this.world, this._tempId, this._initial, this.name) {
    transform = TransformRef._(this);
  }

  final World world;
  final int _tempId;
  final Transform _initial;
  final String name;
  int? _entity;
  bool _alive = true;

  /// Staged local transform not yet acknowledged by a snapshot (§5.4):
  /// scalars in [_staged] (valid while [_stagedValid]), materialised into
  /// [_overlay] only when read. Writes never allocate; a read after a
  /// scalar write allocates one `Transform` until the next write.
  Transform? _overlay;
  TransformWriter? _staged;
  bool _stagedValid = false;

  /// Native components as read this tick (one query per object per tick,
  /// on the first read) merged with this tick's staged writes — `null` is a
  /// staged removal. Cleared with the transform overlay after every step
  /// and on a rejection, so the next read sees what the world applied
  /// (§5.4: reads are the snapshot or the staged state, never a mirror).
  Map<int, WireRecord?>? _components;
  bool _componentsQueried = false;

  /// Writes staged before the spawn resolved; sent once it has.
  List<(gen.ComponentCodec, WireRecord?)>? _pendingComponents;

  void _clearOverlay() {
    _overlay = null;
    _stagedValid = false;
    _components = null;
    _componentsQueried = false;
  }

  /// The staged writer, seeded from the current local value when nothing is
  /// staged yet, so partial writes (position only) keep the rest.
  TransformWriter _stage() {
    final w = _staged ??= TransformWriter();
    if (!_stagedValid) {
      final o = _overlay;
      if (o != null) {
        w.set(o);
      } else if (!world._seedLocal(_entity ?? -1, w)) {
        w.set(_initial);
      }
      _stagedValid = true;
    }
    _overlay = null;
    return w;
  }

  late final TransformRef transform;

  final List<Behaviour> _behaviours = [];

  /// The behaviours attached to this object, in attach order.
  List<Behaviour> get behaviours => List.unmodifiable(_behaviours);

  /// Attaches [behaviour]; its `onCreate`/`onEnable` run at the next safe
  /// boundary (once the object is spawned). Returns it for chaining.
  T addBehaviour<T extends Behaviour>(T behaviour) {
    world._behaviours.attach(this, behaviour);
    return behaviour;
  }

  /// Builds a behaviour of the registered type [typeId] from authored
  /// [fields] (see [BehaviourRegistry.instantiate]) and attaches it.
  Behaviour addBehaviourFromData(String typeId, {Map<String, Object?> fields = const {}, int? version}) =>
      addBehaviour(world.registry.instantiate(typeId, fields: fields, version: version));

  /// The first attached behaviour of type [T].
  T? behaviour<T extends Behaviour>() {
    for (final b in _behaviours) {
      if (b is T) return b;
    }
    return null;
  }

  Iterable<T> behavioursOf<T extends Behaviour>() => _behaviours.whereType<T>();

  bool get isPending => _entity == null;
  bool get isAlive => _alive;

  /// Runtime handle; throws while pending.
  int get entity => _requireResolved('entity');

  int _requireResolved(String what) {
    final e = _entity;
    if (e == null) throw StateError('GameObject "$name" (temp #$_tempId) is not spawned yet; $what needs the next tick');
    if (!_alive) throw StateError('GameObject "$name" (0x${e.toRadixString(16)}) was destroyed');
    return e;
  }

  /// Queues destruction of this object and its subtree for the next tick.
  void destroy() {
    final e = _requireResolved('destroy');
    world._simulation.destroy(gen.DestroyCommand(entity: e));
    world._simulationTargets.add(this);
  }

  void setParent(GameObject? parent, {bool keepWorld = false}) {
    final e = _requireResolved('setParent');
    world._simulation.setParent(gen.SetParentCommand(child: e, parent: parent?._requireResolved('setParent'), keepWorld: keepWorld));
    world._simulationTargets.add(this);
  }

  List<int> get children => world.childrenOf(this);
  int? get parent => world.parentOf(this);

  // ------------------------------------------------ native components (M3-07a)

  static gen.ComponentCodec _codecOf(Type t) {
    final id = gen.componentIds[t];
    if (id == null) throw ArgumentError('$t is not a native component (componentIds lists them)');
    return gen.componentCodecs[id]!;
  }

  /// The native component of type [T] (`MeshRenderer`, `PointLight`,
  /// `Camera`, …, `Transform`, `Name`), or null when the object has none.
  /// Read-your-writes: a value [set] this tick reads back before the tick
  /// applies it; otherwise the value the last step left, fetched with one
  /// query per object per tick. Not for a hot loop over thousands of
  /// objects — that is what snapshots are for.
  T? get<T extends WireRecord>() {
    if (T == Transform) return localTransform as T;
    return _lookup(_codecOf(T).id) as T?;
  }

  bool has<T extends WireRecord>() => T == Transform || _lookup(_codecOf(T).id) != null;

  /// Every native component this object has, id order (`Transform` first,
  /// as [localTransform]), staged writes included.
  List<WireRecord> get components {
    if (!_componentsQueried) _queryComponents();
    final c = _components!;
    final ids = c.keys.where((id) => c[id] != null).toList()..sort();
    return [for (final id in ids) id == gen.Transform.componentId ? localTransform : c[id]!];
  }

  WireRecord? _lookup(int id) {
    final c = _components ??= {};
    if (c.containsKey(id)) return c[id];
    if (!_componentsQueried) {
      _queryComponents();
      return c[id];
    }
    return null;
  }

  void _queryComponents() {
    _componentsQueried = true;
    final c = _components ??= {};
    final e = _entity;
    if (e == null || !_alive) return;
    world._decodeComponents(world._native.query(QueryKind.components, entity: e), (id, v) {
      if (!c.containsKey(id)) c[id] = v; // staged writes win
    });
  }

  /// Simulation-lane write: inserts or replaces the component at the next
  /// tick, readable now through [get]. A `Transform` is [setLocalTransform],
  /// a `Name` renames. On an object not spawned yet the write waits for
  /// the spawn. A value the world rejects (a negative light range, a
  /// removed `Transform`) is a diagnostic on the next step and the read
  /// falls back to what the world has.
  void set<T extends WireRecord>(T value) => _setComponent(value, presentation: false);

  /// [set] for readability when the component is new.
  void add<T extends WireRecord>(T value) => set(value);

  /// Presentation-lane write (lateUpdate): plain data applies immediately
  /// and stays; a `Transform` is [setPresentationTransform].
  void setPresentation<T extends WireRecord>(T value) => _setComponent(value, presentation: true);

  void _setComponent(WireRecord value, {required bool presentation}) {
    if (value is Transform) {
      presentation ? setPresentationTransform(value) : setLocalTransform(value);
      return;
    }
    final codec = _codecOf(value.runtimeType);
    (_components ??= {})[codec.id] = value;
    final e = _entity;
    if (e == null) {
      if (!_alive) _requireResolved('set');
      (_pendingComponents ??= []).add((codec, value));
      return;
    }
    _requireResolved('set');
    final batch = presentation ? world._presentation : world._simulation;
    if (value is gen.Name) {
      batch.setName(gen.SetNameCommand(entity: e, name: value.value));
    } else {
      batch.setComponent(e, codec, value);
    }
    (presentation ? world._presentationTargets : world._simulationTargets).add(this);
  }

  /// Removes the component of type [T] at the next tick ([has] reads false
  /// now). `Transform` and `Name` cannot be removed.
  void remove<T extends WireRecord>() {
    final codec = _codecOf(T);
    if (codec.id == gen.Transform.componentId || codec.id == gen.Name.componentId) {
      throw ArgumentError('${codec.name} cannot be removed');
    }
    (_components ??= {})[codec.id] = null;
    final e = _entity;
    if (e == null) {
      if (!_alive) _requireResolved('remove');
      (_pendingComponents ??= []).add((codec, null));
      return;
    }
    _requireResolved('remove');
    world._simulation.removeComponent(e, codec.id);
    world._simulationTargets.add(this);
  }

  void _flushPendingComponents() {
    final pending = _pendingComponents;
    if (pending == null) return;
    _pendingComponents = null;
    final e = _entity!;
    for (final (codec, value) in pending) {
      if (value == null) {
        world._simulation.removeComponent(e, codec.id);
      } else if (value is gen.Name) {
        world._simulation.setName(gen.SetNameCommand(entity: e, name: value.value));
      } else {
        world._simulation.setComponent(e, codec, value);
      }
      world._simulationTargets.add(this);
    }
  }

  /// Current local transform: staged overlay, else last snapshot, else the
  /// spawn value.
  Transform get localTransform {
    final o = _overlay;
    if (o != null) return o;
    if (_stagedValid) return _overlay = _staged!.toTransform();
    return world._snapshotOf(_entity ?? -1)?.local ?? _initial;
  }

  /// World-space transform from the last snapshot (no overlay: it depends
  /// on the parent chain and is only known after a tick).
  Transform? get worldTransform => world._snapshotOf(_entity ?? -1)?.world;

  /// Simulation-lane write: applied at the next tick, readable now.
  void setLocalTransform(Transform t) {
    final e = _requireResolved('setLocalTransform');
    _overlay = t;
    _stagedValid = false;
    world._simulation.setTransformOf(e, t);
    world._simulationTargets.add(this);
  }

  /// Simulation-lane write from a reusable writer (allocation-free, C1).
  void writeLocal(TransformWriter w) {
    final e = _requireResolved('writeLocal');
    _stage().copyFrom(w);
    world._simulation.setTransformFrom(e, w);
    world._simulationTargets.add(this);
  }

  /// Allocation-free partial writes; unspecified parts keep their current
  /// (staged or last-snapshot) value.
  void setLocalPosition(double x, double y, double z) => _submitLocal(_stage().setPosition(x, y, z));
  void setLocalRotation(double x, double y, double z, double w) => _submitLocal(_stage().setRotation(x, y, z, w));
  void setLocalRotationAxisAngle(double ax, double ay, double az, double radians) =>
      _submitLocal(_stage().setRotationAxisAngle(ax, ay, az, radians));
  void setLocalScale(double x, double y, double z) => _submitLocal(_stage().setScale(x, y, z));

  void _submitLocal(TransformWriter w) {
    final e = _requireResolved('setLocal');
    world._simulation.setTransformFrom(e, w);
    world._simulationTargets.add(this);
  }

  /// Presentation-lane write (lateUpdate/camera): shown this frame, staged
  /// as authoritative for the next tick. Rejected for physics-owned objects.
  void setPresentationTransform(Transform t) {
    final e = _requireResolved('setPresentationTransform');
    _overlay = t;
    _stagedValid = false;
    world._presentation.setTransformOf(e, t);
    world._presentationTargets.add(this);
  }

  /// Presentation-lane write from a reusable writer (allocation-free, C1).
  void writePresentation(TransformWriter w) {
    final e = _requireResolved('writePresentation');
    _stage().copyFrom(w);
    world._presentation.setTransformFrom(e, w);
    world._presentationTargets.add(this);
  }

  void setPresentationPosition(double x, double y, double z) => _submitPresentation(_stage().setPosition(x, y, z));
  void setPresentationRotation(double x, double y, double z, double w) =>
      _submitPresentation(_stage().setRotation(x, y, z, w));

  void _submitPresentation(TransformWriter w) {
    final e = _requireResolved('setPresentation');
    world._presentation.setTransformFrom(e, w);
    world._presentationTargets.add(this);
  }

  @override
  String toString() => 'GameObject($name, ${_entity == null ? 'pending #$_tempId' : '0x${_entity!.toRadixString(16)}'})';
}

/// The objects a command batch targets, by command index (for rejection
/// diagnostics). A growable `List` releases its backing store on `clear()`
/// and regrows every tick; this keeps its capacity (zero steady-state
/// allocation, M1 gate).
class _Targets {
  List<GameObject?> _items = List.filled(64, null);
  int _count = 0;

  int get length => _count;
  GameObject? operator [](int i) => _items[i];

  void add(GameObject? go) {
    if (_count == _items.length) {
      _items = List.filled(_items.length * 2, null)..setRange(0, _count, _items);
    }
    _items[_count++] = go;
  }

  void clear() {
    _items.fillRange(0, _count, null);
    _count = 0;
  }
}

/// Property-style access with read-your-writes semantics.
class TransformRef {
  TransformRef._(this._go);
  final GameObject _go;

  Transform get value => _go.localTransform;
  set value(Transform t) => _go.setLocalTransform(t);

  Vec3 get position => value.position;
  set position(Vec3 p) => value = Transform(position: p, rotation: value.rotation, scale: value.scale);

  Quat get rotation => value.rotation;
  set rotation(Quat q) => value = Transform(position: value.position, rotation: q, scale: value.scale);

  Vec3 get scale => value.scale;
  set scale(Vec3 s) => value = Transform(position: value.position, rotation: value.rotation, scale: s);
}

/// The world's clock as a [TimeService].
class _WorldTime implements TimeService {
  _WorldTime(this.world);
  final World world;

  @override
  double get delta => world._frameDt;
  @override
  double get fixedDelta => world.fixedDt;
  @override
  int get tick => world.tick;
  @override
  double get elapsed => world.tick * world.fixedDt;
  @override
  int get frame => world._frameCount;
  @override
  double get alpha => world.alpha;
}
