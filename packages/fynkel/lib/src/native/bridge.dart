// Thin typed layer over the ffigen bindings (ABI v1). One FFI call per
// operation family (blueprint §5.3); buffers in and out, no per-property
// calls. The gameplay API in lib/src/world.dart builds on this.

import 'dart:convert';
import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import '../../generated/schema.g.dart' as gen;
import '../wire/buffers.dart';
import 'fynkel_bindings.g.dart';

export 'fynkel_bindings.g.dart' show FynkelStatus;

enum CommandLane { simulation, presentation }

enum QueryKind { transform, children, parent, name, allEntities, components, body, entities, character, systems }

class FramePlan {
  const FramePlan(this.steps, this.droppedSeconds, this.clamped, this.alpha);
  final int steps;
  final double droppedSeconds;
  final bool clamped;
  final double alpha;
  bool get missedDeadline => droppedSeconds > 0;
  @override
  String toString() => 'FramePlan(steps: $steps, dropped: $droppedSeconds, clamped: $clamped, alpha: $alpha)';
}

class SubmitResult {
  const SubmitResult(this.status, this.accepted, this.rejected);
  final FynkelStatus status;
  final int accepted, rejected;
}

class StepResult {
  const StepResult(this.status, this.tick, this.applied, this.rejected, this.snapshotRecords, this.eventRecords,
      this.diagnosticRecords, this.liveEntities);
  final FynkelStatus status;
  final int tick, applied, rejected, snapshotRecords, eventRecords, diagnosticRecords, liveEntities;
  @override
  String toString() =>
      'StepResult($status tick=$tick applied=$applied rejected=$rejected snapshot=$snapshotRecords events=$eventRecords diag=$diagnosticRecords live=$liveEntities)';
}

/// Native-side counters plus the Dart side's own (blueprint §5.7 gate).
class BridgeStats {
  BridgeStats({
    required this.tick,
    required this.frame,
    required this.submitCalls,
    required this.stepCalls,
    required this.queryCalls,
    required this.bytesIn,
    required this.bytesOut,
    required this.liveEntities,
    required this.subscriptions,
    required this.stagedCommands,
    required this.poisoned,
    required this.ffiCalls,
    required this.dartCopies,
    required this.dartBytesCopied,
  });
  final int tick, frame, submitCalls, stepCalls, queryCalls, bytesIn, bytesOut;
  final int liveEntities, subscriptions, stagedCommands;
  final bool poisoned;

  /// Every FFI call made through this bridge (all families).
  final int ffiCalls;

  /// Dart→native memcpys of command bytes, and their total size. Output
  /// buffers are read in place (zero copies).
  final int dartCopies, dartBytesCopied;

  @override
  String toString() =>
      'BridgeStats(tick=$tick frame=$frame ffi=$ffiCalls submit=$submitCalls step=$stepCalls query=$queryCalls in=${bytesIn}B out=${bytesOut}B copies=$dartCopies/${dartBytesCopied}B live=$liveEntities subs=$subscriptions staged=$stagedCommands)';
}

class NativeLibrary {
  NativeLibrary._(this._runtime);

  final Pointer<FynkelRuntime> _runtime;

  /// Verifies ABI version and schema fingerprint and creates the runtime.
  ///
  /// The native code is the `package:fynkel/fynkel_native` code asset that
  /// `hook/build.dart` provides (ADR-0015); the bindings are `@Native` and
  /// the Dart/Flutter toolchain resolves them, so there is no library path.
  static NativeLibrary open() {
    final v = fynkel_abi_version();
    if (v != gen.abiVersion) {
      throw StateError('fynkel_native speaks ABI $v, this SDK speaks ${gen.abiVersion}');
    }
    final fp = fynkel_schema_fingerprint();
    if (fp != gen.schemaFingerprint) {
      throw StateError('schema fingerprint mismatch: library ${_hex(fp)} vs SDK ${_hex(gen.schemaFingerprint)}; '
          'rerun `fynkel codegen` and rebuild');
    }
    final rt = fynkel_runtime_create();
    if (rt == nullptr) throw StateError('fynkel_runtime_create failed');
    return NativeLibrary._(rt);
  }

  static String _hex(int v) =>
      '0x${(v >>> 32).toRadixString(16).padLeft(8, '0')}${v.toUnsigned(32).toRadixString(16).padLeft(8, '0')}';

  NativeWorld createWorld({
    int worldId = 0,
    double fixedDt = 0,
    int maxStepsPerFrame = 0,
    double maxFrameDt = 0,
  }) {
    final desc = calloc<FynkelWorldDesc>();
    try {
      desc.ref.world_id = worldId;
      desc.ref.fixed_dt_seconds = fixedDt;
      desc.ref.max_steps_per_frame = maxStepsPerFrame;
      desc.ref.max_frame_dt_seconds = maxFrameDt;
      final w = fynkel_world_create(_runtime, desc);
      if (w == nullptr) throw StateError('fynkel_world_create failed');
      return NativeWorld._(w);
    } finally {
      calloc.free(desc);
    }
  }

  void dispose() => fynkel_runtime_destroy(_runtime);
}

class _ViewCache {
  WireBuffer? buffer;
  int address = 0, length = 0;
}

class NativeWorld {
  NativeWorld._(this._w)
      : _plan = calloc<FynkelFramePlan>(),
        _submit = calloc<FynkelSubmitResult>(),
        _step = calloc<FynkelStepResult>(),
        _query = calloc<FynkelQueryDesc>(),
        _view = calloc<FynkelBufferView>(),
        _stats = calloc<FynkelWorldStats>();

  Pointer<FynkelWorld> _w;
  final Pointer<FynkelFramePlan> _plan;
  final Pointer<FynkelSubmitResult> _submit;
  final Pointer<FynkelStepResult> _step;
  final Pointer<FynkelQueryDesc> _query;
  final Pointer<FynkelBufferView> _view;
  final Pointer<FynkelWorldStats> _stats;

  // Reused native scratch for command bytes (grown on demand) so a submit is
  // one memcpy + one FFI call, no allocation in steady state.
  Pointer<Uint8> _scratch = nullptr;
  Uint8List _scratchView = Uint8List(0);
  int _scratchLen = 0;

  int ffiCalls = 0, dartCopies = 0, dartBytesCopied = 0;

  bool get isDisposed => _w == nullptr;

  /// Raw `FynkelWorld*` for platform viewports that render this world's
  /// extract (e.g. `fynkel_apple_viewport_render`). Valid until [dispose].
  int get handleAddress => _w.address;

  bool get isPoisoned {
    ffiCalls++;
    return fynkel_world_is_poisoned(_w) != 0;
  }

  int get tick {
    ffiCalls++;
    return fynkel_world_tick(_w);
  }

  double get alpha {
    ffiCalls++;
    return fynkel_world_alpha(_w);
  }

  FramePlan beginFrame(double dtSeconds) {
    ffiCalls++;
    final s = fynkel_world_begin_frame(_w, dtSeconds, _plan);
    _check(s, 'begin_frame');
    final p = _plan.ref;
    return FramePlan(p.steps, p.dropped_seconds, p.clamped != 0, p.alpha);
  }

  SubmitResult submit(CommandLane lane, Uint8List bytes) => submitBytes(lane, bytes, bytes.length);

  /// Submits the first [length] bytes of [bytes]: one memcpy into reused
  /// native scratch, one FFI call, no Dart allocation besides the result.
  SubmitResult submitBytes(CommandLane lane, Uint8List bytes, int length) {
    if (length > _scratchLen) {
      if (_scratch != nullptr) calloc.free(_scratch);
      _scratchLen = length * 2;
      _scratch = calloc<Uint8>(_scratchLen);
      _scratchView = _scratch.asTypedList(_scratchLen);
    }
    _scratchView.setRange(0, length, bytes);
    dartCopies++;
    dartBytesCopied += length;
    ffiCalls++;
    final s = fynkel_world_submit_commands(_w, lane.index, _scratch, length, _submit);
    return SubmitResult(s, _submit.ref.accepted, _submit.ref.rejected);
  }

  /// Hands the frame's raw control events (an input-events buffer, the
  /// first [length] bytes of [bytes]) to the world: one memcpy into the
  /// reused scratch, one FFI call.
  FynkelStatus captureInputBytes(Uint8List bytes, int length) {
    if (length > _scratchLen) {
      if (_scratch != nullptr) calloc.free(_scratch);
      _scratchLen = length * 2;
      _scratch = calloc<Uint8>(_scratchLen);
      _scratchView = _scratch.asTypedList(_scratchLen);
    }
    _scratchView.setRange(0, length, bytes);
    dartCopies++;
    dartBytesCopied += length;
    ffiCalls++;
    return fynkel_world_capture_input(_w, _scratch, length);
  }

  /// The bound actions as the next tick (and this frame) sees them: one
  /// `InputActionRecord` per action in declaration order, read in place.
  WireBuffer get inputView {
    ffiCalls++;
    return _wrap(fynkel_world_input_view(_w), _inputView);
  }

  StepResult step() {
    ffiCalls++;
    final s = fynkel_world_step(_w, _step);
    final r = _step.ref;
    return StepResult(s, r.tick, r.commands_applied, r.commands_rejected, r.snapshot_records, r.event_records,
        r.diagnostic_records, r.live_entities);
  }

  FynkelStatus subscribeTransforms(List<int> entities, {bool enable = true}) {
    if (entities.isEmpty) return FynkelStatus.FYNKEL_STATUS_OK;
    final p = calloc<Uint64>(entities.length);
    try {
      p.asTypedList(entities.length).setAll(0, entities);
      ffiCalls++;
      return fynkel_world_subscribe(_w, gen.Transform.componentId, p, entities.length, enable ? 1 : 0);
    } finally {
      calloc.free(p);
    }
  }

  // One cached view per output family: the native side reuses its buffers,
  // so in steady state the address and length repeat and the WireBuffer is
  // reparsed instead of re-created (no allocation).
  final _snapshotView = _ViewCache();
  final _diagnosticsView = _ViewCache();
  final _eventsView = _ViewCache();
  final _afterPhysicsView = _ViewCache();
  final _queryView = _ViewCache();
  final _inputView = _ViewCache();
  final _physicsView = _ViewCache();

  WireBuffer _wrap(FynkelBufferView v, _ViewCache cache) {
    if (v.length == 0) return WireBuffer.empty;
    final address = v.data.address;
    final cached = cache.buffer;
    if (cached != null && cache.address == address && cache.length == v.length) {
      cached.reparse();
      return cached;
    }
    cache.address = address;
    cache.length = v.length;
    return cache.buffer = WireBuffer(v.data.asTypedList(v.length));
  }

  /// Subscribed transforms after the last step (read in place, no copy).
  WireBuffer get snapshot {
    ffiCalls++;
    return _wrap(fynkel_world_snapshot(_w), _snapshotView);
  }

  WireBuffer get diagnostics {
    ffiCalls++;
    return _wrap(fynkel_world_diagnostics(_w), _diagnosticsView);
  }

  /// Pull-only: moves pending events into the returned view.
  WireBuffer drainEvents() {
    ffiCalls++;
    return _wrap(fynkel_world_drain_events(_w), _eventsView);
  }

  WireBuffer get afterPhysicsEvents {
    ffiCalls++;
    return _wrap(fynkel_world_after_physics_events(_w), _afterPhysicsView);
  }

  int _requestId = 0;

  /// Explicit query; the view is valid until the next mutating call or query.
  WireBuffer query(QueryKind kind, {int entity = 0}) {
    _query.ref.kind = kind.index + 1;
    _query.ref.request_id = ++_requestId;
    _query.ref.entity = entity;
    ffiCalls++;
    final s = fynkel_world_query(_w, _query, _view);
    _check(s, 'query');
    final b = _wrap(_view.ref, _queryView);
    assert(b.requestId == _requestId);
    return b;
  }

  /// Answers a `PhysicsQueries` buffer (the first [length] bytes of
  /// [bytes], M3-08): one memcpy into the reused scratch, one FFI call; the
  /// hits come back as `PhysicsHitRecord`s in a query buffer read in
  /// place, valid until the next query or mutating call.
  WireBuffer physicsQuery(Uint8List bytes, int length) {
    if (length > _scratchLen) {
      if (_scratch != nullptr) calloc.free(_scratch);
      _scratchLen = length * 2;
      _scratch = calloc<Uint8>(_scratchLen);
      _scratchView = _scratch.asTypedList(_scratchLen);
    }
    _scratchView.setRange(0, length, bytes);
    dartCopies++;
    dartBytesCopied += length;
    ffiCalls++;
    final s = fynkel_world_physics_query(_w, _scratch, length, _view);
    _check(s, 'physics_query');
    return _wrap(_view.ref, _physicsView);
  }

  /// `fynkel_world_load_project`: the startup scene of the cooked cache at
  /// [cacheDir]; the status and the JSON report (decoded) of that call.
  (FynkelStatus, Map<String, Object?>) loadProject(String cacheDir) =>
      _withUtf8(cacheDir, (c, cl) => _load(() => fynkel_world_load_project(_w, c, cl)));

  /// `fynkel_world_load_scene`: the cooked scene [sceneId] (UUID text) from
  /// the cache at [cacheDir].
  (FynkelStatus, Map<String, Object?>) loadScene(String cacheDir, String sceneId) => _withUtf8(
        cacheDir,
        (c, cl) => _withUtf8(sceneId, (s, sl) => _load(() => fynkel_world_load_scene(_w, c, cl, s, sl))),
      );

  /// `fynkel_world_instantiate`: the cooked prefab [prefabId] (UUID text)
  /// under a new anchor entity named [name] (empty: the prefab's) with the
  /// given local transform, child of [parent] (0: a root); the status and
  /// the JSON report of that call.
  (FynkelStatus, Map<String, Object?>) instantiate(
    String prefabId,
    String name,
    int parent,
    List<double> position,
    List<double> rotation,
    List<double> scale,
  ) =>
      _withUtf8(prefabId, (p, pl) => _withUtf8(name, (n, nl) {
            final d = calloc<FynkelInstantiateDesc>();
            try {
              d.ref.prefab_id_utf8 = p;
              d.ref.prefab_id_len = pl;
              d.ref.name_utf8 = n;
              d.ref.name_len = nl;
              d.ref.parent = parent;
              for (var i = 0; i < 3; i++) {
                d.ref.position[i] = position[i];
                d.ref.scale[i] = scale[i];
              }
              for (var i = 0; i < 4; i++) {
                d.ref.rotation[i] = rotation[i];
              }
              return _load(() => fynkel_world_instantiate(_w, d));
            } finally {
              calloc.free(d);
            }
          }));

  T _withUtf8<T>(String text, T Function(Pointer<Uint8>, int) body) {
    final bytes = utf8.encode(text);
    final p = calloc<Uint8>(bytes.length + 1);
    try {
      p.asTypedList(bytes.length).setAll(0, bytes);
      return body(p, bytes.length);
    } finally {
      calloc.free(p);
    }
  }

  (FynkelStatus, Map<String, Object?>) _load(FynkelStatus Function() call) {
    ffiCalls += 2;
    final s = call();
    final v = fynkel_world_load_report(_w);
    if (v.length == 0) return (s, const {});
    dartCopies++;
    dartBytesCopied += v.length;
    final report = jsonDecode(utf8.decode(v.data.asTypedList(v.length))) as Map<String, Object?>;
    return (s, report);
  }

  BridgeStats stats() {
    ffiCalls++;
    _check(fynkel_world_stats(_w, _stats), 'stats');
    final s = _stats.ref;
    return BridgeStats(
      tick: s.tick,
      frame: s.frame,
      submitCalls: s.submit_calls,
      stepCalls: s.step_calls,
      queryCalls: s.query_calls,
      bytesIn: s.bytes_in,
      bytesOut: s.bytes_out,
      liveEntities: s.live_entities,
      subscriptions: s.subscriptions,
      stagedCommands: s.staged_commands,
      poisoned: s.poisoned != 0,
      ffiCalls: ffiCalls,
      dartCopies: dartCopies,
      dartBytesCopied: dartBytesCopied,
    );
  }

  void _check(FynkelStatus s, String op) {
    if (s != FynkelStatus.FYNKEL_STATUS_OK) throw StateError('fynkel_world_$op: $s');
  }

  void dispose() {
    if (_w == nullptr) return;
    fynkel_world_destroy(_w);
    _w = nullptr;
    calloc.free(_plan);
    calloc.free(_submit);
    calloc.free(_step);
    calloc.free(_query);
    calloc.free(_view);
    calloc.free(_stats);
    if (_scratch != nullptr) calloc.free(_scratch);
  }
}
