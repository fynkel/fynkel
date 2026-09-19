// The Dart gameplay API seed (design/dart-sdk.md §21.2, ADR-0008): World,
// GameObject, TransformRef with a staged-write overlay. Behaviours and the
// scene/asset layers come with M2; this is what the bridge contract needs
// to be exercised end to end from Dart.

import 'dart:typed_data';

import '../generated/schema.g.dart' as gen;
import '../math.dart';
import 'diagnostics.dart';
import 'events.dart';
import 'native/bridge.dart';
import 'wire/buffers.dart';

export 'native/bridge.dart' show BridgeStats, CommandLane, FramePlan, StepResult;

/// Immutable local transform value (position, rotation, scale).
typedef Transform = gen.Transform;

const Transform identityTransform = Transform(position: Vec3.zero, rotation: Quat.identity, scale: Vec3.one);

/// Runtime entity/component container over one native world.
class World {
  World._(this._lib, this._native, {required this.ownsLibrary});

  /// Creates a world on a fresh [NativeLibrary] (or the given one).
  factory World.create({
    NativeLibrary? library,
    int worldId = 0,
    double fixedDt = 0,
    int maxStepsPerFrame = 0,
    double maxFrameDt = 0,
  }) {
    final lib = library ?? NativeLibrary.open();
    final native = lib.createWorld(
        worldId: worldId, fixedDt: fixedDt, maxStepsPerFrame: maxStepsPerFrame, maxFrameDt: maxFrameDt);
    return World._(lib, native, ownsLibrary: library == null);
  }

  final NativeLibrary _lib;
  final NativeWorld _native;
  final bool ownsLibrary;

  /// Low-level access for hosts and tests.
  NativeWorld get native => _native;

  final CommandBatch _simulation = CommandBatch();
  final CommandBatch _presentation = CommandBatch();

  /// Command index → GameObject for overlay clearing on rejection.
  final List<GameObject?> _simulationTargets = [];
  final List<GameObject?> _presentationTargets = [];

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

  /// Throw [FynkelError] on diagnostics. Defaults to [isDebugMode].
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
    final plan = _native.beginFrame(dtSeconds);
    if (plan.missedDeadline) _collectDiagnostics('begin_frame');
    return plan;
  }

  /// One fixed tick: flushes staged simulation commands, steps the native
  /// world, resolves spawns, refreshes the snapshot, reports diagnostics.
  StepResult step() {
    _flush(CommandLane.simulation, _simulation, _simulationTargets, 'submit');
    final r = _native.step();
    _checkFatal(r.status, 'step');
    _snapshot = _native.snapshot;
    _snapshotIndex.clear();
    final d = _snapshot.records;
    for (var i = 0; i < _snapshot.recordCount; i++) {
      _snapshotIndex[d.getUint64(i * gen.SnapshotTransformRecord.wireSize, Endian.little)] = i;
    }
    // Overlays are consumed by the tick they were staged for.
    for (final go in _byEntity.values) {
      go._overlay = null;
    }
    _collectDiagnostics('step');
    return r;
  }

  /// Pull-only event drain; resolves spawn/destroy bookkeeping on the way.
  List<EngineEvent> drainEvents() {
    final events = decodeEvents(_native.drainEvents()).toList();
    for (final e in events) {
      switch (e) {
        case EntitySpawned(:final tempId, :final entity):
          final go = _pendingByTempId.remove(tempId);
          if (go != null) {
            go._entity = entity;
            _byEntity[entity] = go;
            _toSubscribe.add(entity);
          }
        case EntityDestroyed(:final entity):
          _byEntity.remove(entity)?._alive = false;
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

  /// Convenience host loop for tests and headless runs: begin frame, drain
  /// events, run [fixedUpdate] and step per planned tick, flush
  /// presentation writes.
  FramePlan frame(double dtSeconds, {void Function(int tick)? fixedUpdate, void Function()? update}) {
    final plan = beginFrame(dtSeconds);
    for (var i = 0; i < plan.steps; i++) {
      drainEvents();
      fixedUpdate?.call(tick);
      step();
    }
    drainEvents();
    update?.call();
    flushPresentation();
    return plan;
  }

  void _flush(CommandLane lane, CommandBatch batch, List<GameObject?> targets, String context) {
    if (batch.isEmpty) return;
    final r = _native.submit(lane, batch.encoded());
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
        targets[d.commandIndex]?._overlay = null;
      }
      if (d.entity != 0) _byEntity[d.entity]?._overlay = null;
    }
    if (context == 'step') _simulationTargets.clear();
    if (throwOnDiagnostics) throw FynkelError(list, context: context);
    onDiagnostics(list);
  }

  // ---------------------------------------------------------------- reads

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

  String? nameOf(GameObject go) {
    final b = _native.query(QueryKind.name, entity: go._requireResolved('name'));
    if (b.recordCount == 0) return null;
    return gen.Name.decode(b.records, 0, b.tail).value;
  }

  List<int> allEntities() => _native.query(QueryKind.allEntities).u64s();

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

  /// Staged local transform not yet acknowledged by a snapshot (§5.4).
  Transform? _overlay;

  late final TransformRef transform;

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

  /// Current local transform: staged overlay, else last snapshot, else the
  /// spawn value.
  Transform get localTransform => _overlay ?? world._snapshotOf(_entity ?? -1)?.local ?? _initial;

  /// World-space transform from the last snapshot (no overlay: it depends
  /// on the parent chain and is only known after a tick).
  Transform? get worldTransform => world._snapshotOf(_entity ?? -1)?.world;

  /// Simulation-lane write: applied at the next tick, readable now.
  void setLocalTransform(Transform t) {
    final e = _requireResolved('setLocalTransform');
    _overlay = t;
    world._simulation.setTransform(gen.SetTransformCommand(entity: e, transform: t));
    world._simulationTargets.add(this);
  }

  /// Presentation-lane write (lateUpdate/camera): shown this frame, staged
  /// as authoritative for the next tick. Rejected for physics-owned objects.
  void setPresentationTransform(Transform t) {
    final e = _requireResolved('setPresentationTransform');
    _overlay = t;
    world._presentation.setTransform(gen.SetTransformCommand(entity: e, transform: t));
    world._presentationTargets.add(this);
  }

  @override
  String toString() => 'GameObject($name, ${_entity == null ? 'pending #$_tempId' : '0x${_entity!.toRadixString(16)}'})';
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
