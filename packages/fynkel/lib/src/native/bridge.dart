// Thin typed layer over the ffigen bindings (ABI v1). One FFI call per
// operation family (blueprint §5.3); buffers in and out, no per-property
// calls. The gameplay API in lib/src/world.dart builds on this.

import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import '../../generated/schema.g.dart' as gen;
import '../wire/buffers.dart';
import 'fynkel_bindings.g.dart';

export 'fynkel_bindings.g.dart' show FynkelStatus;

enum CommandLane { simulation, presentation }

enum QueryKind { transform, children, parent, name, allEntities }

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
  int _scratchLen = 0;

  int ffiCalls = 0, dartCopies = 0, dartBytesCopied = 0;

  bool get isDisposed => _w == nullptr;

  /// Raw `FynkelWorld*` for platform viewports that render this world's
  /// extract (e.g. `fynkel_macos_viewport_render`). Valid until [dispose].
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

  SubmitResult submit(CommandLane lane, Uint8List bytes) {
    if (bytes.length > _scratchLen) {
      if (_scratch != nullptr) calloc.free(_scratch);
      _scratchLen = bytes.length * 2;
      _scratch = calloc<Uint8>(_scratchLen);
    }
    _scratch.asTypedList(_scratchLen).setRange(0, bytes.length, bytes);
    dartCopies++;
    dartBytesCopied += bytes.length;
    ffiCalls++;
    final s = fynkel_world_submit_commands(_w, lane.index, _scratch, bytes.length, _submit);
    return SubmitResult(s, _submit.ref.accepted, _submit.ref.rejected);
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

  WireBuffer _wrap(FynkelBufferView v) =>
      v.length == 0 ? WireBuffer.empty : WireBuffer(v.data.asTypedList(v.length));

  /// Subscribed transforms after the last step (read in place, no copy).
  WireBuffer get snapshot {
    ffiCalls++;
    return _wrap(fynkel_world_snapshot(_w));
  }

  WireBuffer get diagnostics {
    ffiCalls++;
    return _wrap(fynkel_world_diagnostics(_w));
  }

  /// Pull-only: moves pending events into the returned view.
  WireBuffer drainEvents() {
    ffiCalls++;
    return _wrap(fynkel_world_drain_events(_w));
  }

  WireBuffer get afterPhysicsEvents {
    ffiCalls++;
    return _wrap(fynkel_world_after_physics_events(_w));
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
    final b = _wrap(_view.ref);
    assert(b.requestId == _requestId);
    return b;
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
