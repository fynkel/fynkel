// The Dart gameplay API (design/dart-sdk.md §21.2, ADR-0008): World,
// GameObject, TransformRef with a staged-write overlay, and (behaviour.dart)
// the Behaviour lifecycle the world drives at the §5.1 points of a host
// frame. Scenes come from the cooked cache through `loadProject`/`loadScene`.

import 'dart:async' show Completer;
import 'dart:convert' show jsonEncode;
import 'dart:typed_data';

import '../generated/schema.g.dart' as gen;
import '../math.dart';
import 'animation.dart';
import 'asset_ref.dart';
import 'context.dart';
import 'diagnostics.dart';
import 'events.dart';
import 'input.dart';
import 'native/bridge.dart';
import 'physics.dart';
import 'settings.dart';
import 'systems.dart';
import 'transform_writer.dart';
import 'wire/buffers.dart';
import 'wire/support.dart' show WireRecord;

export 'native/bridge.dart' show BridgeStats, CommandLane, FramePlan, StepResult;

part 'behaviour.dart';
part 'projection.dart';
part 'scenes.dart';

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
    scenes = SceneManager._(this);
    projection = ProjectionView._(this);
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

  /// Scene flow (M3-12): the scenes the world holds, `transition(to)`
  /// performed at the start of the next [frame], its observable state.
  late final SceneManager scenes;

  /// Named zone events (M3-12): `zones.on('collect', (e) { … })` runs
  /// right after the tick whose step produced the `ZoneEntered`.
  final ZoneEvents zones = ZoneEvents();

  /// Projected world anchors (M3-13, docs/hud.md): where entities land on
  /// the screen this frame, for labels drawn over the viewport. Register
  /// anchors, set the viewport, `update()` once per frame after [frame]
  /// (`FynkelGameView` does).
  late final ProjectionView projection;

  /// The cooked cache the last successful load came from; the default
  /// [loadScene] cache.
  String? get cacheDir => _cacheDir;
  String? _cacheDir;

  /// Installs [store] as [context]'s settings and attaches it to this
  /// world (M3-12): its rebinds go over the project's actions now and
  /// after every update. `world.useSettings(SettingsStore.open(
  /// SettingsStore.defaultPath('My Game'), world: world))` is the usual
  /// startup line, after `loadProject`.
  void useSettings(SettingsStore store) {
    context.settings = store;
    store.attach(this);
  }

  /// The physics settings of the last `loadProject` (gravity, layer names);
  /// the defaults before a load. [layerMask] reads the names.
  PhysicsSettings physics = const PhysicsSettings();

  final RecordBatch _physicsQueries = RecordBatch(BufferKind.physicsQueries, gen.PhysicsQueryRecord.wireSize);
  int _nextQueryId = 1;

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
  /// [GameObject.isPending] until that tick resolves it. Simulation-lane
  /// writes on a pending object (`add`, `set`, `remove`, `setLocal…`,
  /// body commands, `setParent`, `destroy`, spawning a child under it) go
  /// into the same batch as a *temp reference* the world resolves at the
  /// tick, so an object is spawned and fully built in one `fixedUpdate`;
  /// presentation-lane writes and native reads need the real handle and
  /// wait for the tick. Reads return the initial transform and the writes
  /// staged so far.
  GameObject spawn({Transform transform = identityTransform, GameObject? parent, String name = ''}) {
    final tempId = _nextTempId++;
    final go = GameObject._(this, tempId, transform, name);
    _pendingByTempId[tempId] = go;
    _simulation.spawn(
      gen.SpawnCommand(tempId: tempId, transform: transform, parent: parent?._simulationEntity('spawn parent'), name: name),
    );
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
    _entityTable = null;
    _collectDiagnostics('step');
    return r;
  }

  /// Pull-only event drain; resolves spawn/destroy bookkeeping on the way
  /// and sends the transform subscriptions of handles attached since.
  List<EngineEvent> drainEvents() {
    final buffer = _native.drainEvents();
    if (buffer.recordCount == 0) {
      _flushSubscriptions();
      return const [];
    }
    final events = decodeEvents(buffer).toList();
    for (final e in events) {
      switch (e) {
        case EntitySpawned(:final tempId, :final entity):
          final go = _pendingByTempId.remove(tempId);
          if (go != null) {
            go._entity = entity;
            _byEntity[entity] = go;
            _toSubscribe.add(entity);
            // The staged writes were applied with the spawn (a rejection
            // cleared them at the step); they stay readable until the
            // first snapshot, and the next read of a native component asks
            // the world.
            go._componentsQueried = false;
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
    _flushSubscriptions();
    return events;
  }

  void _flushSubscriptions() {
    if (_toSubscribe.isEmpty) return;
    _native.subscribeTransforms(_toSubscribe);
    _toSubscribe.clear();
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
    scenes._perform();
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
      _behaviours.afterPhysics(stepped, _contacts(events), _zoneEvents(events), _animationEvents(events));
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

  static List<ZoneEvent> _zoneEvents(List<EngineEvent> events) {
    if (events.isEmpty) return const [];
    return [for (final e in events) if (e is ZoneEvent) e];
  }

  static List<AnimationEnded> _animationEvents(List<EngineEvent> events) {
    if (events.isEmpty) return const [];
    return [for (final e in events) if (e is AnimationEnded) e];
  }

  /// A handle for an entity that already exists (loaded from a scene, or
  /// found by a query); the same handle comes back for the same entity.
  /// Its transform is subscribed from the next tick; until then
  /// [GameObject.localTransform] reads [initial].
  GameObject attach(int entity, {String name = '', Transform initial = identityTransform}) {
    final existing = _byEntity[entity];
    if (existing != null) return existing;
    final go = GameObject._(this, 0, initial, name.isEmpty ? (nameOfEntity(entity) ?? '') : name);
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

  /// Every live entity with its parent and name as the last step left
  /// them (`FYNKEL_QUERY_ENTITIES`), fetched once per tick on the first
  /// [find]; objects spawned this tick are not in it yet.
  _EntityTable get _entities => _entityTable ??= _EntityTable.query(_native);
  _EntityTable? _entityTable;

  /// The first entity named [name] in the world (scene order), as a
  /// [GameObject] — an existing handle when there is one, else one
  /// attached now ([attach]). Null when no entity carries the name. Reads
  /// the world as the last tick left it: an object spawned or renamed
  /// this tick is found from the next one. One query per tick, then a map
  /// lookup — fine in `start`, not meant for every `fixedUpdate` of a
  /// thousand behaviours (keep the handle).
  GameObject? find(String name) {
    final e = _entities.firstNamed(name);
    return e == null ? null : attach(e, name: name);
  }

  /// Every entity named [name], scene order (see [find]).
  List<GameObject> findAll(String name) => [for (final e in _entities.allNamed(name)) attach(e, name: name)];

  /// The entities under [parent] (direct children in scene order), from
  /// the per-tick table rather than a query per node.
  List<int> _childrenFromTable(int parent) => _entities.childrenOf(parent);
  String? _nameFromTable(int entity) => _entities.nameOf(entity);

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

  // ---------------------------------------------------------------- physics (M3-08)

  /// The collision mask of the named project layers (`physics.layers`);
  /// throws on a name the project does not declare.
  int layerMask(Iterable<String> names) {
    final m = physics.mask(names);
    if (m == null) throw ArgumentError('unknown physics layer in $names; the project declares ${physics.layers}');
    return m;
  }

  /// The nearest collider along a ray, or null. Spatial queries read the
  /// world as the last step left it (one FFI call each; batch with
  /// [physicsQuery] when asking many at once).
  PhysicsHit? raycast(Vec3 origin, Vec3 direction,
          {double maxDistance = 1000, int mask = 0xffffffff, GameObject? exclude, bool hitSensors = false}) =>
      raycastAll(origin, direction, maxDistance: maxDistance, mask: mask, exclude: exclude, hitSensors: hitSensors, maxHits: 1)
          .firstOrNull;

  /// Every collider along a ray, nearest first, at most [maxHits].
  List<PhysicsHit> raycastAll(Vec3 origin, Vec3 direction,
      {double maxDistance = 1000, int mask = 0xffffffff, GameObject? exclude, bool hitSensors = false, int maxHits = 16}) {
    _physicsQueries.clear();
    _physicsQueries.add(gen.PhysicsQueryRecord(
      id: _nextQueryId++,
      kind: gen.PhysicsQueryKind.raycast,
      shape: gen.ColliderShape.box,
      size: Vec3.zero,
      origin: origin,
      rotation: Quat.identity,
      direction: direction,
      maxDistance: maxDistance,
      mask: mask,
      exclude: exclude?._entity,
      maxHits: maxHits,
      hitSensors: hitSensors,
    ));
    return _runPhysicsQueries();
  }

  /// Sweeps a `box`/`sphere`/`capsule` of [size] (as `Collider.size`
  /// defines it) from [origin] along [direction]: the first collider it
  /// would touch, or null.
  PhysicsHit? shapeCast(gen.ColliderShape shape, Vec3 size, Vec3 origin, Vec3 direction,
      {Quat rotation = Quat.identity,
      double maxDistance = 1000,
      int mask = 0xffffffff,
      GameObject? exclude,
      bool hitSensors = false}) {
    _physicsQueries.clear();
    _physicsQueries.add(gen.PhysicsQueryRecord(
      id: _nextQueryId++,
      kind: gen.PhysicsQueryKind.shapeCast,
      shape: shape,
      size: size,
      origin: origin,
      rotation: rotation,
      direction: direction,
      maxDistance: maxDistance,
      mask: mask,
      exclude: exclude?._entity,
      maxHits: 1,
      hitSensors: hitSensors,
    ));
    return _runPhysicsQueries().firstOrNull;
  }

  /// Every collider intersecting a `box`/`sphere`/`capsule` of [size] at
  /// [origin] (distance 0, normal zero).
  List<PhysicsHit> overlap(gen.ColliderShape shape, Vec3 size, Vec3 origin,
      {Quat rotation = Quat.identity, int mask = 0xffffffff, GameObject? exclude, bool hitSensors = false, int maxHits = 16}) {
    _physicsQueries.clear();
    _physicsQueries.add(gen.PhysicsQueryRecord(
      id: _nextQueryId++,
      kind: gen.PhysicsQueryKind.overlap,
      shape: shape,
      size: size,
      origin: origin,
      rotation: rotation,
      direction: Vec3.zero,
      maxDistance: 0,
      mask: mask,
      exclude: exclude?._entity,
      maxHits: maxHits,
      hitSensors: hitSensors,
    ));
    return _runPhysicsQueries();
  }

  /// Several queries in one FFI call: the hits of every record, in query
  /// order (`PhysicsHitRecord.query` echoes each record's `id`).
  List<gen.PhysicsHitRecord> physicsQuery(List<gen.PhysicsQueryRecord> queries) {
    _physicsQueries.clear();
    queries.forEach(_physicsQueries.add);
    final b = _native.physicsQuery(_physicsQueries.backingStore, _physicsQueries.finalize());
    _physicsQueries.clear();
    return b.fixed(gen.PhysicsHitRecord.wireSize, gen.PhysicsHitRecord.decode).toList(growable: false);
  }

  List<PhysicsHit> _runPhysicsQueries() {
    final b = _native.physicsQuery(_physicsQueries.backingStore, _physicsQueries.finalize());
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
  /// Entities exist immediately; components go through the command path
  /// (rejections reach [onDiagnostics] on the next step). The scene's
  /// assets stay resident in the world's store — a viewport rendering this
  /// world draws from it — until the world is disposed. Throws
  /// [FynkelLoadError] when the cache or the scene has errors.
  LoadReport loadProject(String dir, {String? cacheDir}) {
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

  /// Unloads the loaded scene [sceneId] (M3-12): its entities that are
  /// still alive are destroyed (their handles go dead on the next drain,
  /// after `EntityDestroyed` events and one `SceneUnloaded`), its assets
  /// released. Runtime spawns — [instantiate]d prefabs, [spawn]ed
  /// objects — are not the scene's and stay. Throws [FynkelUnloadError]
  /// when the world does not hold the scene (`FYN-R0010`).
  UnloadReport unloadScene(Object sceneId) {
    final id = _assetId(sceneId, 'sceneId');
    final (status, json) = _native.unloadScene(id);
    final report = UnloadReport.fromJson(json);
    switch (status) {
      case FynkelStatus.FYNKEL_STATUS_OK:
        _entityTable = null;
        _loadedScenes = report.scenes;
        scenes._unloaded(report);
        return report;
      case FynkelStatus.FYNKEL_STATUS_LOAD_FAILED:
        throw FynkelUnloadError(report);
      default:
        throw FynkelBridgeError('unloadScene: $status');
    }
  }

  /// The scenes the world holds, load order (from the last load or
  /// unload report; empty before any).
  List<LoadedScene> get loadedScenes => _loadedScenes;
  List<LoadedScene> _loadedScenes = const [];

  /// Lays the user's [rebinds] (`action → bindings`, the grammar of
  /// [docs/input.md]) over the project's actions (M3-12): an empty map
  /// restores the project's. They stay over every later load. An action
  /// the project lacks binds nothing; a bad binding is a `bindingErrors`
  /// entry on the report, never fatal. The report's [LoadReport.inputActions]
  /// show what is bound (`rebound` per action). Throws [FynkelLoadError]
  /// before a project is loaded.
  LoadReport bindActions(Map<String, List<String>> rebinds) {
    final json = jsonEncode(rebinds);
    return _load(_native.bindActions(json), 'bindActions');
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
        _entityTable = null;
        if (op == 'loadProject') physics = report.physics;
        if (op != 'bindActions') _loadedScenes = report.scenes;
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
    final (attached, extra) = _attachBehaviours(report.behaviours, report.scenePath);
    return report.withBehaviours(attached: attached, extra: extra);
  }

  (int, List<LoadDiagnostic>) _attachBehaviours(List<AuthoredBehaviour> behaviours, String? document) {
    final extra = <LoadDiagnostic>[];
    var attached = 0;
    for (final b in behaviours) {
      if (!registry.contains(b.type)) {
        extra.add(LoadDiagnostic(
          code: 'FYN-B0001',
          severity: 'warning',
          message: 'behaviour type ${b.type} is not registered; the component is loaded without a behaviour',
          document: document,
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
          document: document,
          pointer: b.pointer == null ? null : '${b.pointer}/data${e.field == null ? '' : '/${e.field}'}',
          suggestion: 'fix the component data or regenerate behaviours.g.dart (`fynkel codegen --project`)',
        ));
      }
    }
    return (attached, extra);
  }

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
  /// an anchor object named [name] (the prefab's
  /// name by default) at [transform] under [parent], whose children are
  /// the prefab's entities — the shape a scene instance expands to — with
  /// the prefab's authored behaviours attached from [registry]. Returns
  /// the anchor, already resolved (no tick needed); the prefab's
  /// components apply through the command path like a scene's, so a
  /// rejected value is a diagnostic on the next step. [lastInstantiate]
  /// has the report. Throws [FynkelInstantiateError] when the prefab is
  /// not in the loaded cache, does not decode (recook) or [parent] is not
  /// spawned yet; throws [StateError] before any project is loaded.
  GameObject instantiate(
    Object prefab, {
    Transform transform = identityTransform,
    GameObject? parent,
    String? name,
  }) {
    final id = switch (prefab) {
      AssetRef r when (r.sub ?? '').isEmpty || r.sub == importedPrefabSub => r.asset,
      AssetRef r => throw ArgumentError.value(
          prefab, 'prefab', 'sub "${r.sub}" is not a prefab (a prefab document has none; an imported model\'s is "$importedPrefabSub")'),
      String s => s,
      _ => throw ArgumentError.value(prefab, 'prefab', 'a prefab asset id (UUID text) or an AssetRef'),
    };
    final parentEntity = parent?._requireResolved('instantiate parent') ?? 0;
    final (status, json) = _native.instantiate(
      id,
      name ?? '',
      parentEntity,
      [transform.position.x, transform.position.y, transform.position.z],
      [transform.rotation.x, transform.rotation.y, transform.rotation.z, transform.rotation.w],
      [transform.scale.x, transform.scale.y, transform.scale.z],
    );
    var report = InstantiateReport.fromJson(json);
    switch (status) {
      case FynkelStatus.FYNKEL_STATUS_OK:
        break;
      case FynkelStatus.FYNKEL_STATUS_LOAD_FAILED:
        throw FynkelInstantiateError(report);
      default:
        throw FynkelBridgeError('instantiate: $status');
    }
    _entityTable = null;
    final anchor = attach(report.anchor, name: report.name, initial: transform);
    if (report.behaviours.isNotEmpty) {
      final (attached, extra) = _attachBehaviours(report.behaviours, report.prefabPath);
      report = report.withBehaviours(attached: attached, extra: extra);
    }
    lastInstantiate = report;
    if (report.warnings.isNotEmpty) onLoadDiagnostics(report.warnings.toList(growable: false));
    return anchor;
  }

  /// The report of the last [instantiate] (null before any).
  InstantiateReport? lastInstantiate;

  /// Called with the warnings of a successful load. Defaults to `print`.
  void Function(List<LoadDiagnostic>) onLoadDiagnostics = (d) => print(d.join('\n'));

  BridgeStats stats() => _native.stats();

  void dispose() {
    _native.dispose();
    if (ownsLibrary) _lib.dispose();
  }
}

/// Stable handle to an entity. Owns no native memory.
/// Playback of one object's `fynkel.Animator` ([GameObject.animation]).
/// Every method is a command for the next tick; [state] reads the last.
class AnimationControl {
  AnimationControl._(this._go);
  final GameObject _go;

  /// Plays [clip] from its start (or its end, at a negative [speed]),
  /// replacing what plays, no blend.
  void play(AssetRef clip, {bool looping = true, double speed = 1}) =>
      _go._animate(gen.AnimateOp.play, clip: clip, speed: speed, looping: looping);

  /// Blends from the current pose into [clip] over [seconds]: the old clip
  /// keeps advancing underneath until the fade ends. A fade of 0 is a
  /// [play]; a negative fade is rejected (`INVALID_PAYLOAD`).
  void crossfade(AssetRef clip, double seconds, {bool looping = true, double speed = 1}) =>
      _go._animate(gen.AnimateOp.crossfade, clip: clip, seconds: seconds, speed: speed, looping: looping);

  /// Stops and rewinds: the nodes keep the pose of the last tick until
  /// another clip plays.
  void stop() => _go._animate(gen.AnimateOp.stop);

  /// Holds the current time; [resume] continues from it.
  void pause() => _go._animate(gen.AnimateOp.pause);
  void resume() => _go._animate(gen.AnimateOp.resume);

  /// Playback rate from the next tick; negative plays backwards, 0 holds.
  set speed(double value) => _go._animate(gen.AnimateOp.setSpeed, speed: value);

  /// Seeks to [seconds] into the clip (clamped to its length); the pose
  /// there is applied at the next tick without advancing.
  set time(double seconds) => _go._animate(gen.AnimateOp.setTime, seconds: seconds);

  /// The animator's playback after the last tick (`FYNKEL_QUERY_ANIMATOR`);
  /// `null` without an `Animator` or before the object spawned.
  AnimatorState? get state {
    final e = _go._entity;
    if (e == null || !_go._alive) return null;
    final b = _go.world._native.query(QueryKind.animator, entity: e);
    if (b.recordCount == 0) return null;
    return AnimatorState.fromRecord(gen.AnimatorStateRecord.decode(b.records, 0, b.tail));
  }

  /// True while a clip advances ([AnimatorState.playing]).
  bool get isPlaying => state?.playing ?? false;
}

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
        w.set(_queriedTransform() ?? _initial);
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

  /// The handle a simulation-lane command names: the real one, or — while
  /// the spawn is still in this tick's batch — the temp reference the
  /// world resolves at the tick (`Entity::temp`: world 0, generation 0,
  /// slot = temp id). Throws once destroyed.
  int _simulationEntity(String what) {
    final e = _entity;
    if (e != null) return _requireResolved(what);
    if (!_alive) throw StateError('GameObject "$name" (temp #$_tempId) was destroyed');
    return _tempId;
  }

  /// Queues destruction of this object and its subtree for the next tick.
  void destroy() {
    final e = _simulationEntity('destroy');
    world._simulation.destroy(gen.DestroyCommand(entity: e));
    world._simulationTargets.add(this);
  }

  void setParent(GameObject? parent, {bool keepWorld = false}) {
    final e = _simulationEntity('setParent');
    world._simulation.setParent(
      gen.SetParentCommand(child: e, parent: parent?._simulationEntity('setParent'), keepWorld: keepWorld),
    );
    world._simulationTargets.add(this);
  }

  List<int> get children => world.childrenOf(this);
  int? get parent => world.parentOf(this);

  /// The direct child named [name] (scene order), or null; a `/` walks
  /// down: `findChild('Arm/Hand')` is the `Hand` under the `Arm` under
  /// this object. Reads the hierarchy as the last tick left it (see
  /// [World.find]); the result is an attached [GameObject].
  GameObject? findChild(String path) {
    var at = _requireResolved('findChild');
    for (final segment in path.split('/')) {
      if (segment.isEmpty) continue;
      int? next;
      for (final c in world._childrenFromTable(at)) {
        if (world._nameFromTable(c) == segment) {
          next = c;
          break;
        }
      }
      if (next == null) return null;
      at = next;
    }
    return at == _entity ? this : world.attach(at);
  }

  /// The first descendant named [name], depth first in scene order (a
  /// child before its siblings' subtrees), or null. See [findChild].
  GameObject? findDescendant(String name) {
    final root = _requireResolved('findDescendant');
    final stack = <int>[...world._childrenFromTable(root).reversed];
    while (stack.isNotEmpty) {
      final e = stack.removeLast();
      if (world._nameFromTable(e) == name) return world.attach(e, name: name);
      stack.addAll(world._childrenFromTable(e).reversed);
    }
    return null;
  }

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
  /// a `Name` renames. On an object not spawned yet the write goes into
  /// the spawn's own batch (a temp reference), so `spawn` then `add` in
  /// one `fixedUpdate` is one tick. A value the world rejects (a negative
  /// light range, a removed `Transform`) is a diagnostic on the next step
  /// and the read falls back to what the world has.
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
    final e = presentation ? _requireResolved('setPresentation') : _simulationEntity('set');
    (_components ??= {})[codec.id] = value;
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
    final e = _simulationEntity('remove');
    (_components ??= {})[codec.id] = null;
    world._simulation.removeComponent(e, codec.id);
    world._simulationTargets.add(this);
  }

  // ------------------------------------------------ rigid body (M3-08)

  /// The body's motion as the last step left it, or null without a
  /// `RigidBody` (one query; not for a hot loop).
  BodyState? bodyState() {
    final e = _entity;
    if (e == null || !_alive) return null;
    final b = world._native.query(QueryKind.body, entity: e);
    if (b.recordCount == 0) return null;
    return BodyState.fromRecord(gen.BodyStateRecord.decode(b.records, 0, b.tail));
  }

  /// A body command for the next tick (simulation lane): rejected there
  /// with a diagnostic when the object has no `RigidBody`, the body type
  /// does not take it (forces need `dynamic`, a kinematic target needs
  /// `kinematic`) or a value is not finite. Allowed on a pending object
  /// whose body was `add`ed in the same batch: the tick creates the body,
  /// then applies the command.
  void _body(gen.BodyOp op, Vec3 v, [Quat q = Quat.identity]) {
    final e = _simulationEntity('body command');
    world._simulation.body(e, op, v.x, v.y, v.z, q.x, q.y, q.z, q.w);
    world._simulationTargets.add(this);
  }

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
  /// no `move` in a tick is standing still. Rejected at the tick on an
  /// object without a controller. A controller with `moveAction` set is
  /// walked by the runtime already; a `move` in the same tick wins.
  void move(Vec3 velocity) => _body(gen.BodyOp.move, velocity);

  /// Starts a jump at [speed] metres per second upward (the controller's
  /// `jumpSpeed` when 0): the caller decides when — typically on a
  /// pressed edge while [characterState] says `grounded`.
  void jump([double speed = 0]) => _body(gen.BodyOp.jump, Vec3(0, speed, 0));

  /// The character's state after the last tick (`FYNKEL_QUERY_CHARACTER`);
  /// `null` without a `CharacterController` or before it spawned.
  CharacterState? characterState() {
    final e = _entity;
    if (e == null || !_alive) return null;
    final b = world._native.query(QueryKind.character, entity: e);
    if (b.recordCount == 0) return null;
    return CharacterState.fromRecord(gen.CharacterStateRecord.decode(b.records, 0, b.tail));
  }

  // ------------------------------------------------- animation (M3-19)

  /// The object's `fynkel.Animator` (M3-19): `animation.play(clip)`,
  /// `crossfade`, `stop`, `pause`, `resume`, `speed`, `time` are commands
  /// for the next tick (simulation lane; rejected there with
  /// `MISSING_COMPONENT` on an object without an animator), `state` the
  /// last tick's playback. Allowed on a pending object whose animator was
  /// `add`ed in the same batch.
  AnimationControl get animation => _animation ??= AnimationControl._(this);
  AnimationControl? _animation;

  void _animate(gen.AnimateOp op,
      {AssetRef clip = AssetRef.none, double seconds = 0, double speed = 1, bool looping = true}) {
    final e = _simulationEntity('animation command');
    world._simulation.animate(e, op, clip: clip, seconds: seconds, speed: speed, looping: looping);
    world._simulationTargets.add(this);
  }

  /// Current local transform: staged overlay, else last snapshot, else
  /// (an object attached this tick, its subscription not yet served) the
  /// world's value through the component query, else the spawn value.
  Transform get localTransform {
    final o = _overlay;
    if (o != null) return o;
    if (_stagedValid) return _overlay = _staged!.toTransform();
    return world._snapshotOf(_entity ?? -1)?.local ?? _queriedTransform() ?? _initial;
  }

  /// The world's own local transform before the first snapshot (one
  /// query per object per tick, shared with [get]), so a partial write at
  /// the first tick keeps the authored pose instead of seeding from
  /// identity.
  Transform? _queriedTransform() {
    if (_entity == null || !_alive) return null;
    return _lookup(gen.Transform.componentId) as Transform?;
  }

  /// World-space transform from the last snapshot (no overlay: it depends
  /// on the parent chain and is only known after a tick).
  Transform? get worldTransform => world._snapshotOf(_entity ?? -1)?.world;

  /// Simulation-lane write: applied at the next tick, readable now.
  void setLocalTransform(Transform t) {
    final e = _simulationEntity('setLocalTransform');
    _overlay = t;
    _stagedValid = false;
    world._simulation.setTransformOf(e, t);
    world._simulationTargets.add(this);
  }

  /// Simulation-lane write from a reusable writer (allocation-free, C1).
  void writeLocal(TransformWriter w) {
    final e = _simulationEntity('writeLocal');
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
    final e = _simulationEntity('setLocal');
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

/// The world's entities, parents and names as one query (M3-08b); built
/// lazily per tick by [World.find] and friends.
class _EntityTable {
  _EntityTable._(this._parents, this._names, this._byName, this._children);

  factory _EntityTable.query(NativeWorld native) {
    final b = native.query(QueryKind.entities);
    final parents = <int, int>{};
    final names = <int, String>{};
    final byName = <String, List<int>>{};
    final children = <int, List<int>>{};
    final size = gen.EntityInfoRecord.wireSize;
    for (var i = 0; i < b.recordCount; i++) {
      final r = gen.EntityInfoRecord.decode(b.records, i * size, b.tail);
      if (r.parent != 0) {
        parents[r.entity] = r.parent;
        (children[r.parent] ??= []).add(r.entity);
      }
      if (r.name.isNotEmpty) {
        names[r.entity] = r.name;
        (byName[r.name] ??= []).add(r.entity);
      }
    }
    return _EntityTable._(parents, names, byName, children);
  }

  final Map<int, int> _parents;
  final Map<int, String> _names;
  final Map<String, List<int>> _byName;
  final Map<int, List<int>> _children;

  int? firstNamed(String name) => _byName[name]?.first;
  List<int> allNamed(String name) => _byName[name] ?? const [];
  String? nameOf(int entity) => _names[entity];
  int? parentOf(int entity) => _parents[entity];
  List<int> childrenOf(int parent) => _children[parent] ?? const [];
}
