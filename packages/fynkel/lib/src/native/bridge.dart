// Thin typed layer over the ffigen bindings (ABI v5). One FFI call per
// operation family (blueprint §5.3); buffers in and out, no per-property
// calls. The gameplay API in lib/src/world.dart builds on this.

import '../asset_ref.dart';
import 'dart:convert';
import 'dart:ffi';
import 'dart:typed_data';

import '../entity_ref.dart';
import '../../math.dart' show Vec3, Quat;
import '../world.dart' show GameObject;

import 'package:ffi/ffi.dart';

import '../../generated/schema.g.dart' as gen;
import '../wire/buffers.dart';
import '../wire/support.dart' show WireStrings, WireRecord;
import '../diagnostics.dart' show FynkelBridgeError, FynkelPanicError;
import 'fynkel_bindings.g.dart';

export 'fynkel_bindings.g.dart' show FynkelStatus;

part 'gameplay.dart';

enum QueryKind {
  transform,
  children,
  parent,
  name,
  allEntities,
  components,
  body,
  entities,
  character,
  systems,
  animator,
  audio,
  audioStats,
  animatorController,
  animatorParameters,
}

class FramePlan {
  const FramePlan(
    this.steps,
    this.droppedSeconds,
    this.clamped,
    this.alpha, {
    this.realDelta = 0,
    this.gameplayDelta = 0,
    this.realElapsed = 0,
    this.gameplayElapsed = 0,
    this.clampedSeconds = 0,
    this.frame = 0,
  });
  final double realDelta, gameplayDelta, realElapsed, gameplayElapsed, clampedSeconds;
  final int frame;
  final int steps;
  final double droppedSeconds;
  final bool clamped;
  final double alpha;
  bool get missedDeadline => droppedSeconds > 0;
  @override
  String toString() => 'FramePlan(steps: $steps, dropped: $droppedSeconds, clamped: $clamped, alpha: $alpha)';
}

class StepResult {
  const StepResult(
    this.status,
    this.tick,
    this.applied,
    this.rejected,
    this.snapshotRecords,
    this.eventRecords,
    this.diagnosticRecords,
    this.liveEntities, {
    this.simulationElapsed = 0,
  });
  final double simulationElapsed;
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
    required this.stepCalls,
    required this.queryCalls,
    required this.bytesIn,
    required this.bytesOut,
    required this.liveEntities,
    required this.subscriptions,
    required this.poisoned,
    required this.ffiCalls,
    required this.dartCopies,
    required this.dartBytesCopied,
    this.snapshotBytes = 0,
    this.snapshotPeakBytes = 0,
    this.snapshotBudget = 0,
    this.nativeBytes = 0,
    this.residentBytes = 0,
    this.residencyBytes = 0,
  });
  final int tick, frame, stepCalls, queryCalls, bytesIn, bytesOut;
  final int liveEntities, subscriptions;
  final bool poisoned;

  /// The last tick's snapshot in bytes, the largest so far, and the
  /// host's ceiling (`World.snapshotBudget`; 0 = none) — blueprint C5.
  final int snapshotBytes, snapshotPeakBytes, snapshotBudget;

  /// The engine's Rust heap in use, the process's resident set (0 where
  /// the platform does not report it) and the CPU bytes of the resident
  /// assets (M3-16, §15.4).
  final int nativeBytes, residentBytes, residencyBytes;

  /// Every FFI call made through this bridge (all families).
  final int ffiCalls;

  /// Explicit bridge copies and their total size, including immutable owned
  /// output publications.
  final int dartCopies, dartBytesCopied;

  @override
  String toString() =>
      'BridgeStats(tick=$tick frame=$frame ffi=$ffiCalls step=$stepCalls query=$queryCalls in=${bytesIn}B out=${bytesOut}B copies=$dartCopies/${dartBytesCopied}B live=$liveEntities subs=$subscriptions)';
}

class NativeLibrary {
  NativeLibrary._(this._runtime);

  final Pointer<FynkelRuntime> _runtime;

  /// Process-lifetime authority token for native embedding hosts. Retain it
  /// outside replaceable engine modules; pass it to each replacement before
  /// that module creates any world, runtime or authoring session. Never persist
  /// its address or send it to another process. It has no dispose operation.
  static Pointer<Void> get identityAuthority => fynkel_identity_authority().cast<Void>();

  /// Verifies ABI version and schema fingerprint and creates the runtime.
  ///
  /// The native code is the `package:fynkel/fynkel_native` code asset that
  /// `hook/build.dart` provides (ADR-0015); the bindings are `@Native` and
  /// the Dart/Flutter toolchain resolves them, so there is no library path.
  /// An embedding host supplies its retained authority before first use of
  /// a replacement code asset; the usual single-module host omits it.
  static NativeLibrary open({Pointer<Void>? identityAuthority}) {
    final v = fynkel_abi_version();
    if (v != gen.abiVersion) {
      throw StateError('fynkel_native speaks ABI $v, this SDK speaks ${gen.abiVersion}');
    }
    final fp = fynkel_schema_fingerprint();
    if (fp != gen.schemaFingerprint) {
      throw StateError(
        'schema fingerprint mismatch: library ${_hex(fp)} vs SDK ${_hex(gen.schemaFingerprint)}; '
        'rerun `fynkel codegen` and rebuild',
      );
    }
    if (identityAuthority != null) {
      final status = fynkel_identity_attach(identityAuthority.cast<FynkelIdentityAuthority>());
      if (status != FynkelStatus.FYNKEL_STATUS_OK) {
        throw StateError('Identity authority attachment failed; initialize replacement modules before use');
      }
    }
    final rt = fynkel_runtime_create();
    if (rt == nullptr) throw StateError('fynkel_runtime_create failed');
    return NativeLibrary._(rt);
  }

  static String _hex(int v) =>
      '0x${(v >>> 32).toRadixString(16).padLeft(8, '0')}${v.toUnsigned(32).toRadixString(16).padLeft(8, '0')}';

  NativeWorld createWorld({double fixedDt = 0, int maxStepsPerFrame = 0, double maxFrameDt = 0}) {
    final desc = calloc<FynkelWorldDesc>();
    final status = calloc<UnsignedInt>();
    try {
      desc.ref.fixed_dt_seconds = fixedDt;
      desc.ref.max_steps_per_frame = maxStepsPerFrame;
      desc.ref.max_frame_dt_seconds = maxFrameDt;
      final w = fynkel_world_create(_runtime, desc, status);
      if (w == nullptr) {
        throw StateError('fynkel_world_create failed: status ${status.value}');
      }
      return NativeWorld._(w);
    } finally {
      calloc.free(desc);
      calloc.free(status);
    }
  }

  void dispose() => fynkel_runtime_destroy(_runtime);
}

class NativeWorld {
  NativeWorld._(this._ptr)
    : _plan = calloc<FynkelFramePlan>(),
      _step = calloc<FynkelStepResult>(),
      _query = calloc<FynkelQueryDesc>(),
      _view = calloc<FynkelBufferView>(),
      _stats = calloc<FynkelWorldStats>(),
      _projection = calloc<FynkelProjectionInfo>();

  Pointer<FynkelWorld> _ptr;

  /// The native world; after [dispose] every call throws here, before it
  /// reaches freed native memory or the freed scratch below (foundation
  /// 4.2).
  Pointer<FynkelWorld> get _w {
    _requireLive();
    return _ptr;
  }

  void _requireLive() {
    if (_ptr == nullptr) {
      throw StateError('the world was disposed (World.dispose); it can no longer be used');
    }
  }

  bool get isDisposed => _ptr == nullptr;

  final Pointer<FynkelFramePlan> _plan;
  final Pointer<FynkelStepResult> _step;
  final Pointer<FynkelQueryDesc> _query;
  final Pointer<FynkelBufferView> _view;
  final Pointer<FynkelWorldStats> _stats;
  final Pointer<FynkelProjectionInfo> _projection;

  // Reused native scratch for command bytes (grown on demand) so a submit is
  // one memcpy + one FFI call, no allocation in steady state.
  Pointer<Uint8> _scratch = nullptr;
  Uint8List _scratchView = Uint8List(0);
  int _scratchLen = 0;

  int ffiCalls = 0, dartCopies = 0, dartBytesCopied = 0;

  /// Successful identity snapshots decoded and identity response bytes read.
  /// Conditional unchanged responses count bytes but do not count a snapshot.
  int gameplayStateSnapshots = 0, gameplayStateBytes = 0;

  /// Raw `FynkelWorld*` for platform viewports that render this world's
  /// extract (e.g. `fynkel_apple_viewport_render`). Valid until [dispose].
  int get handleAddress => _w.address;

  bool get isPoisoned {
    ffiCalls++;
    return fynkel_world_is_poisoned(_w) != 0;
  }

  /// `fynkel_world_panic_message`: why the world is poisoned; null when
  /// it is not, or no panic said (foundation 4.1).
  String? get panicMessage {
    ffiCalls++;
    final v = fynkel_world_panic_message(_w);
    if (v.length == 0) return isPoisoned ? '(no message)' : null;
    return utf8.decode(v.data.asTypedList(v.length), allowMalformed: true);
  }

  /// `fynkel_world_debug_panic`: panics inside the world on purpose.
  void debugPanic() => _check(fynkel_world_debug_panic(_w), 'debug_panic');

  /// The error for a call that answered [s]: a [FynkelPanicError] with
  /// the panic's message when the world is poisoned, a
  /// [FynkelBridgeError] otherwise.
  FynkelBridgeError failure(String op, FynkelStatus s) =>
      s == FynkelStatus.FYNKEL_STATUS_POISONED || s == FynkelStatus.FYNKEL_STATUS_PANIC
      ? FynkelPanicError(op, panicMessage ?? '(no message)')
      : FynkelBridgeError('$op: $s');

  int get tick {
    ffiCalls++;
    return fynkel_world_tick(_w);
  }

  double get alpha {
    ffiCalls++;
    return fynkel_world_alpha(_w);
  }

  double get simulationElapsed {
    ffiCalls++;
    return fynkel_world_simulation_elapsed(_w);
  }

  FramePlan beginFrame(double dtSeconds, double timeScale) {
    ffiCalls++;
    final s = fynkel_world_begin_frame(_w, dtSeconds, timeScale, _plan);
    _check(s, 'begin_frame');
    final p = _plan.ref;
    return FramePlan(
      p.steps,
      p.dropped_seconds,
      p.clamped != 0,
      p.alpha,
      realDelta: p.real_delta,
      gameplayDelta: p.gameplay_delta,
      realElapsed: p.real_elapsed,
      gameplayElapsed: p.gameplay_elapsed,
      clampedSeconds: p.clamped_seconds,
      frame: p.frame,
    );
  }

  /// Copies the first [length] bytes of [bytes] into the reused native
  /// scratch (grown on demand). Checks the world first: after [dispose]
  /// the scratch is freed (foundation 4.2).
  void _toScratch(Uint8List bytes, int length) {
    _requireLive();
    if (length > _scratchLen) {
      if (_scratch != nullptr) calloc.free(_scratch);
      _scratchLen = length * 2;
      _scratch = calloc<Uint8>(_scratchLen);
      _scratchView = _scratch.asTypedList(_scratchLen);
    }
    _scratchView.setRange(0, length, bytes);
    dartCopies++;
    dartBytesCopied += length;
  }

  FynkelStatus prepareGameplayTick() {
    _requireLive();
    ffiCalls++;
    return fynkel_world_gameplay_tick_prepare(_w);
  }

  FynkelStatus cancelGameplayTick() {
    _requireLive();
    ffiCalls++;
    return fynkel_world_gameplay_tick_cancel(_w);
  }

  void registerComponent(String id, int version, List<NativeRegisteredField> fields) {
    _requireLive();
    final w = _GameplayWriter(0);
    w.bytes.clear();
    w.u32(1); w.string(id); w.u32(version); w.u32(fields.length);
    for (final f in fields) { w.string(f.name); w.scalar(f.defaultValue); }
    final bytes = w.bytes.takeBytes();
    _toScratch(bytes, bytes.length);
    ffiCalls++;
    final status = fynkel_world_component_register(_w, _scratch, bytes.length, _view);
    if (status != FynkelStatus.FYNKEL_STATUS_OK) { throw StateError(utf8.decode(_view.ref.data.asTypedList(_view.ref.length))); }
  }

  /// Read-only identity/name snapshot; never commits gameplay or drains events.
  Map<EntityRef, String> gameplayState() {
    _requireLive();
    final status = fynkel_world_gameplay_state(_w, _view);
    ffiCalls++;
    final bytes = _view.ref.data.asTypedList(_view.ref.length);
    if (status != FynkelStatus.FYNKEL_STATUS_OK) {
      throw StateError(utf8.decode(bytes));
    }
    gameplayStateBytes += bytes.length;
    final reader = _GameplayReader(bytes);
    gameplayStateSnapshots++;
    final live = <EntityRef, String>{};
    final size = reader.u32();
    for (var i = 0; i < size; i++) {
      final entity = reader.entity();
      live[entity] = reader.string();
    }
    return live;
  }

  /// A world-local consumer cursor. Null state means identities/names are
  /// unchanged; the caller must retain its existing state. Zero forces a read.
  ({int revision, Map<EntityRef, String>? live}) gameplayStateSince(int since) {
    _requireLive();
    final status = fynkel_world_gameplay_state_since(_w, since, _view);
    ffiCalls++;
    final bytes = _view.ref.data.asTypedList(_view.ref.length);
    if (status != FynkelStatus.FYNKEL_STATUS_OK) {
      throw StateError(utf8.decode(bytes));
    }
    gameplayStateBytes += bytes.length;
    final reader = _GameplayReader(bytes);
    final revision = reader.u64();
    if (reader.u32() == 0) return (revision: revision, live: null);
    gameplayStateSnapshots++;
    final live = <EntityRef, String>{};
    final size = reader.u32();
    for (var i = 0; i < size; i++) {
      final entity = reader.entity();
      live[entity] = reader.string();
    }
    return (revision: revision, live: live);
  }

  /// Sparse identity changes; reset replaces the consumer's membership.
  ({int revision, bool reset, Map<EntityRef, String?> changes}) gameplayStateDelta(int since) {
    _requireLive();
    final status = fynkel_world_gameplay_state_delta(_w, since, _view);
    ffiCalls++;
    final bytes = _view.ref.data.asTypedList(_view.ref.length);
    if (status != FynkelStatus.FYNKEL_STATUS_OK) throw StateError(utf8.decode(bytes));
    gameplayStateBytes += bytes.length;
    final reader = _GameplayReader(bytes);
    final revision = reader.u64();
    final mode = reader.u32();
    if (mode == 0) return (revision: revision, reset: false, changes: const {});
    if (mode == 1) gameplayStateSnapshots++;
    final changes = <EntityRef, String?>{};
    final count = reader.u32();
    for (var i = 0; i < count; i++) {
      final entity = reader.entity();
      changes[entity] = reader.u32() == 0 ? null : reader.string();
    }
    return (revision: revision, reset: mode == 1, changes: changes);
  }

  FynkelStatus playSoundNow(gen.PlaySoundCommandRecord sound) {
    _requireLive();
    if (sound.priority < -2147483648 || sound.priority > 2147483647) {
      throw ArgumentError.value(sound.priority, 'priority', 'must fit a signed 32-bit integer');
    }
    final body = ByteData(gen.PlaySoundCommandRecord.wireSize), strings = WireStrings();
    sound.encode(body, 0, strings);
    final bytes = Uint8List.fromList([...body.buffer.asUint8List(), ...strings.takeBytes()]);
    _toScratch(bytes, bytes.length);
    ffiCalls++;
    return fynkel_world_audio_play(_w, _scratch, bytes.length);
  }

  /// Internal F2 migration boundary. The callback is synchronous and atomic.
  /// Ordinary World/GameObject callback routing is migrated separately.
  NativeGameplayCommit<R> editGameplay<R>(R Function(NativeGameplayEdit edit) callback) {
    _requireLive();
    final sequence = calloc<Uint64>();
    final status = fynkel_world_gameplay_begin(_w, sequence);
    final token = sequence.value;
    calloc.free(sequence);
    ffiCalls++;
    if (status != FynkelStatus.FYNKEL_STATUS_OK) {
      throw failure('gameplay begin', status);
    }
    final edit = NativeGameplayEdit._(this, token);
    var ended = false;
    try {
      final value = callback(edit);
      if (value is Future) {
        // An invalid async callback may resume later; the closed edit rejects it.
        value.then<void>((_) {}, onError: (Object _, StackTrace _) {});
        throw StateError('gameplay edits must be synchronous');
      }
      if (edit._failed) throw StateError('gameplay edit previously failed');
      final result = fynkel_world_gameplay_end(_w, token, 1, _view);
      ffiCalls++;
      ended = true;
      final bytes = Uint8List.fromList(_view.ref.data.asTypedList(_view.ref.length));
      if (result != FynkelStatus.FYNKEL_STATUS_OK) {
        throw StateError(utf8.decode(bytes));
      }
      final reader = _GameplayReader(bytes);
      final resolved = <int, EntityRef>{};
      final count = reader.u32();
      for (var i = 0; i < count; i++) {
        final index = reader.u32();
        resolved[index] = reader.entity();
      }
      return NativeGameplayCommit._(value, edit, resolved);
    } finally {
      edit._closed = true;
      if (!ended && !isDisposed) {
        fynkel_world_gameplay_end(_w, token, 0, _view);
        ffiCalls++;
      }
    }
  }

  /// Hands the frame's raw control events (an input-events buffer, the
  /// first [length] bytes of [bytes]) to the world: one memcpy into the
  /// reused scratch, one FFI call.
  FynkelStatus captureInputBytes(Uint8List bytes, int length) {
    _toScratch(bytes, length);
    ffiCalls++;
    return fynkel_world_capture_input(_w, _scratch, length);
  }

  /// The bound actions as the next tick (and this frame) sees them: one
  /// Owned `InputActionRecord` publication in declaration order.
  WireBuffer get inputView {
    ffiCalls++;
    return _wrap(fynkel_world_input_view(_w));
  }

  FynkelStatus snapInterpolation() {
    ffiCalls++;
    return fynkel_world_snap_interpolation(_w);
  }

  StepResult step() {
    ffiCalls++;
    final s = fynkel_world_step(_w, _step);
    final r = _step.ref;
    return StepResult(
      s,
      r.tick,
      r.commands_applied,
      r.commands_rejected,
      r.snapshot_records,
      r.event_records,
      r.diagnostic_records,
      r.live_entities,
      simulationElapsed: r.simulation_elapsed,
    );
  }

  FynkelStatus subscribeTransforms(List<EntityRef> entities, {bool enable = true}) {
    if (entities.isEmpty) return FynkelStatus.FYNKEL_STATUS_OK;
    final p = calloc<FynkelEntity>(entities.length);
    try {
      for (var i = 0; i < entities.length; i++) {
        _writeNativeEntity(p[i], entities[i]);
      }
      ffiCalls++;
      return fynkel_world_subscribe(_w, gen.Transform.componentId, p, entities.length, enable ? 1 : 0);
    } finally {
      calloc.free(p);
    }
  }

  // Published buffers own immutable Dart memory. No native allocation or
  // mutable reusable wrapper can escape a call's lifetime.
  WireBuffer _wrap(FynkelBufferView v) {
    if (v.length == 0) return WireBuffer.empty;
    dartCopies++;
    dartBytesCopied += v.length;
    return WireBuffer(v.data.asTypedList(v.length));
  }

  /// Immutable owned subscribed transforms from the last completed step.
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

  bool contains(EntityRef entity) {
    _requireLive();
    _writeNativeEntity(_query.ref.entity, entity);
    ffiCalls++;
    return fynkel_world_contains(_w, _query.ref.entity) != 0;
  }

  int _requestId = 0;

  /// Explicit query returning an owned immutable snapshot, safe across later
  /// queries, mutation and disposal.
  WireBuffer query(QueryKind kind, {EntityRef entity = EntityRef.nil}) {
    _requireLive(); // before the write into the scratch
    _query.ref.kind = kind.index + 1;
    _query.ref.request_id = ++_requestId;
    _writeNativeEntity(_query.ref.entity, entity);
    ffiCalls++;
    final s = fynkel_world_query(_w, _query, _view);
    _check(s, 'query');
    final b = _wrap(_view.ref);
    assert(b.requestId == _requestId);
    return b;
  }

  /// Answers a `PhysicsQueries` buffer (the first [length] bytes of
  /// [bytes], M3-08): one memcpy into the reused scratch, one FFI call; the
  /// hits come back as `PhysicsHitRecord`s in an immutable owned buffer.
  WireBuffer physicsQuery(Uint8List bytes, int length) {
    _toScratch(bytes, length);
    ffiCalls++;
    final s = fynkel_world_physics_query(_w, _scratch, length, _view);
    _check(s, 'physics_query');
    return _wrap(_view.ref);
  }

  /// Projects a `ProjectionAnchors` buffer (the first [length] bytes of
  /// [bytes], M3-13) into a [width] × [height] viewport: one memcpy into
  /// the reused scratch, one FFI call; the points come back as
  /// `ProjectedPointRecord`s in an immutable owned buffer, with the camera
  /// entity (0 = none)
  /// and the projected/dropped counts.
  (WireBuffer, EntityRef camera, int projected, int dropped) project(
    Uint8List bytes,
    int length,
    int width,
    int height,
  ) {
    _copyToScratch(bytes, length);
    ffiCalls++;
    final s = fynkel_world_project(_w, width, height, _scratch, length, _projection, _view);
    _check(s, 'project');
    final info = _projection.ref;
    return (_wrap(_view.ref), _readNativeEntity(info.camera), info.projected, info.dropped);
  }

  void _copyToScratch(Uint8List bytes, int length) {
    _toScratch(bytes, length);
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

  /// `FYNKEL_LOAD_ADDITIVE`: the [loadSceneWith] flag that keeps the world.
  static const int loadAdditive = FYNKEL_LOAD_ADDITIVE;

  /// `fynkel_world_load_scene_with` (M3-12): [loadScene] with [flags] —
  /// 0 replaces what the world holds, [loadAdditive] keeps it.
  (FynkelStatus, Map<String, Object?>) loadSceneWith(String cacheDir, String sceneId, int flags) => _withUtf8(
    cacheDir,
    (c, cl) => _withUtf8(sceneId, (s, sl) => _load(() => fynkel_world_load_scene_with(_w, c, cl, s, sl, flags))),
  );

  /// `fynkel_world_load_scene_async` (ADR-0038): starts loading [sceneId]
  /// from [cacheDir] in the background; the ticket (0 when the status is
  /// not OK) and, on `LOAD_FAILED`, the report.
  (FynkelStatus, int, Map<String, Object?>) loadSceneAsync(String cacheDir, String sceneId) {
    final ticket = calloc<Uint32>();
    try {
      final (s, report) = _withUtf8(
        cacheDir,
        (c, cl) => _withUtf8(sceneId, (i, il) {
          ffiCalls++;
          final s = fynkel_world_load_scene_async(_w, c, cl, i, il, ticket);
          return s == FynkelStatus.FYNKEL_STATUS_LOAD_FAILED ? _load(() => s) : (s, const <String, Object?>{});
        }),
      );
      return (s, s == FynkelStatus.FYNKEL_STATUS_OK ? ticket.value : 0, report);
    } finally {
      calloc.free(ticket);
    }
  }

  /// `fynkel_world_load_progress`: the state (`FYNKEL_LOAD_*`) and the
  /// share loaded of the load [ticket]; null for a ticket the world does
  /// not hold.
  (int, double)? loadProgress(int ticket) {
    final state = calloc<Uint32>();
    final progress = calloc<Float>();
    try {
      ffiCalls++;
      if (fynkel_world_load_progress(_w, ticket, state, progress) != FynkelStatus.FYNKEL_STATUS_OK) {
        return null;
      }
      return (state.value, progress.value);
    } finally {
      calloc.free(state);
      calloc.free(progress);
    }
  }

  static const int loadLoading = FYNKEL_LOAD_LOADING;
  static const int loadReady = FYNKEL_LOAD_READY;
  static const int loadFailed = FYNKEL_LOAD_FAILED;

  /// `fynkel_world_load_activate`: spawns the ready load [ticket] with
  /// [flags] as [loadSceneWith] does; the same status and report.
  (FynkelStatus, Map<String, Object?>) loadActivate(int ticket, int flags) =>
      _load(() => fynkel_world_load_activate(_w, ticket, flags));

  /// `fynkel_world_load_cancel`: drops the load [ticket].
  FynkelStatus loadCancel(int ticket) {
    ffiCalls++;
    return fynkel_world_load_cancel(_w, ticket);
  }

  /// `fynkel_world_unload_scene` (M3-12): the loaded scene [sceneId] goes.
  (FynkelStatus, Map<String, Object?>) unloadScene(String sceneId) =>
      _withUtf8(sceneId, (s, sl) => _load(() => fynkel_world_unload_scene(_w, s, sl)));

  /// `fynkel_world_dont_destroy_on_load` (ADR-0037): keeps the root
  /// [entity] and its subtree across loads; `INVALID_ARGUMENT` for one not
  /// live or with a parent.
  FynkelStatus dontDestroyOnLoad(EntityRef entity) {
    final value = calloc<FynkelEntity>();
    try {
      _writeNativeEntity(value.ref, entity);
      return fynkel_world_dont_destroy_on_load(_w, value.ref);
    } finally {
      calloc.free(value);
    }
  }

  /// `fynkel_world_bind_actions` (M3-12): [rebindsJson] is the JSON object
  /// `{ action: [bindings] }` laid over the project's actions.
  (FynkelStatus, Map<String, Object?>) bindActions(String rebindsJson) =>
      _withUtf8(rebindsJson, (s, sl) => _load(() => fynkel_world_bind_actions(_w, s, sl)));

  /// `fynkel_world_instantiate`: the cooked prefab [prefabId] (UUID text)
  /// with its root named [name] (empty: the prefab's) at the given local
  /// transform, child of [parent] (0: a root); the status and
  /// the JSON report of that call.
  (FynkelStatus, Map<String, Object?>) instantiate(
    String prefabId,
    String name,
    EntityRef parent,
    List<double> position,
    List<double> rotation,
    List<double> scale,
  ) => _withUtf8(
    prefabId,
    (p, pl) => _withUtf8(name, (n, nl) {
      final d = calloc<FynkelInstantiateDesc>();
      try {
        d.ref.prefab_id_utf8 = p;
        d.ref.prefab_id_len = pl;
        d.ref.name_utf8 = n;
        d.ref.name_len = nl;
        _writeNativeEntity(d.ref.parent, parent);
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
    }),
  );

  /// `fynkel_world_clone` (ADR-0026 §4): a copy of the live subtree
  /// under [source]; the report's `clone` has the pairs.
  (FynkelStatus, Map<String, Object?>) clone(
    EntityRef source,
    String name,
    EntityRef parent,
    List<double> position,
    List<double> rotation,
    List<double> scale,
  ) => _withUtf8(name, (n, nl) {
    final d = calloc<FynkelCloneDesc>();
    try {
      _writeNativeEntity(d.ref.source, source);
      d.ref.name_utf8 = n;
      d.ref.name_len = nl;
      _writeNativeEntity(d.ref.parent, parent);
      for (var i = 0; i < 3; i++) {
        d.ref.position[i] = position[i];
        d.ref.scale[i] = scale[i];
      }
      for (var i = 0; i < 4; i++) {
        d.ref.rotation[i] = rotation[i];
      }
      return _load(() => fynkel_world_clone(_w, d));
    } finally {
      calloc.free(d);
    }
  });

  /// `fynkel_world_set_material` (ADR-0026 §4): [material] drawn as the
  /// runtime material [id] (16 UUID bytes); null removes it.
  FynkelStatus setMaterial(Uint8List id, gen.PbrMaterial? material) {
    ffiCalls++;
    final idp = calloc<Uint8>(16);
    try {
      idp.asTypedList(16).setAll(0, id);
      if (material == null) {
        return fynkel_world_set_material(_w, idp, nullptr, 0);
      }
      final payload = materialPayload(material);
      final p = calloc<Uint8>(payload.length);
      try {
        p.asTypedList(payload.length).setAll(0, payload);
        return fynkel_world_set_material(_w, idp, p, payload.length);
      } finally {
        calloc.free(p);
      }
    } finally {
      calloc.free(idp);
    }
  }

  /// `fynkel_world_set_shader_material` (ADR-0031): the material document
  /// [json] (its `shader` a `.shader` asset's id) checked against its
  /// shader and drawn as the runtime material [id] (16 UUID bytes); null
  /// removes it. The diagnostics come back with the status, warnings too.
  (FynkelStatus, List<Map<String, Object?>>) setShaderMaterial(Uint8List id, String? json) {
    ffiCalls++;
    final idp = calloc<Uint8>(16);
    final out = calloc<FynkelBufferView>();
    Pointer<Uint8> text = nullptr;
    try {
      idp.asTypedList(16).setAll(0, id);
      var length = 0;
      if (json != null) {
        final bytes = utf8.encode(json);
        length = bytes.length;
        text = calloc<Uint8>(length);
        text.asTypedList(length).setAll(0, bytes);
      }
      final s = fynkel_world_set_shader_material(_w, idp, text, length, out);
      final v = out.ref;
      if (v.length == 0) return (s, const []);
      dartCopies++;
      dartBytesCopied += v.length;
      final list = jsonDecode(utf8.decode(v.data.asTypedList(v.length))) as List<Object?>;
      return (s, [for (final d in list) (d as Map).cast<String, Object?>()]);
    } finally {
      calloc.free(idp);
      calloc.free(out);
      if (text != nullptr) calloc.free(text);
    }
  }

  /// `fynkel_world_set_mesh` (ADR-0026 §4): the static `MESH` [payload]
  /// drawn as the runtime mesh [id] (16 UUID bytes); null removes it.
  FynkelStatus setMesh(Uint8List id, Uint8List? payload) {
    ffiCalls++;
    final idp = calloc<Uint8>(16);
    try {
      idp.asTypedList(16).setAll(0, id);
      if (payload == null) return fynkel_world_set_mesh(_w, idp, nullptr, 0);
      final p = calloc<Uint8>(payload.length);
      try {
        p.asTypedList(payload.length).setAll(0, payload);
        return fynkel_world_set_mesh(_w, idp, p, payload.length);
      } finally {
        calloc.free(p);
      }
    } finally {
      calloc.free(idp);
    }
  }

  /// `fynkel_world_set_clip` (ADR-0033 § 2): the `ANIM` [payload]
  /// registered as the runtime clip [id] (16 UUID bytes); null removes it.
  FynkelStatus setClip(Uint8List id, Uint8List? payload) {
    ffiCalls++;
    final idp = calloc<Uint8>(16);
    try {
      idp.asTypedList(16).setAll(0, id);
      if (payload == null) return fynkel_world_set_clip(_w, idp, nullptr, 0);
      final p = calloc<Uint8>(payload.length);
      try {
        p.asTypedList(payload.length).setAll(0, payload);
        return fynkel_world_set_clip(_w, idp, p, payload.length);
      } finally {
        calloc.free(p);
      }
    } finally {
      calloc.free(idp);
    }
  }

  /// `fynkel_world_asset_json` (ADR-0027 §6): the cooked JSON of the
  /// document asset [id] (16 UUID bytes) — `(OK, the document)`, or
  /// `(LOAD_FAILED, the diagnostic)` when no project is loaded or the
  /// cache has no such document.
  (FynkelStatus, Map<String, Object?>) assetJson(Uint8List id) {
    ffiCalls++;
    final idp = calloc<Uint8>(16);
    final out = calloc<FynkelBufferView>();
    try {
      idp.asTypedList(16).setAll(0, id);
      final s = fynkel_world_asset_json(_w, idp, out);
      final v = out.ref;
      if (v.length == 0) return (s, const {});
      dartCopies++;
      dartBytesCopied += v.length;
      return (s, jsonDecode(utf8.decode(v.data.asTypedList(v.length))) as Map<String, Object?>);
    } finally {
      calloc.free(idp);
      calloc.free(out);
    }
  }

  /// A `MATL` chunk payload (`fynkel_wire::material`): u32 version 1,
  /// record size, tail length, reserved; the record; its string tail.
  static Uint8List materialPayload(gen.PbrMaterial m) {
    const header = 16;
    final strings = WireStrings();
    final record = ByteData(gen.PbrMaterial.wireSize);
    m.encode(record, 0, strings);
    final tail = strings.takeBytes();
    final out = Uint8List(header + record.lengthInBytes + tail.length);
    ByteData.sublistView(out)
      ..setUint32(0, 1, Endian.little)
      ..setUint32(4, record.lengthInBytes, Endian.little)
      ..setUint32(8, tail.length, Endian.little);
    out.setAll(header, record.buffer.asUint8List());
    out.setAll(header + record.lengthInBytes, tail);
    return out;
  }

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

  /// Opens the output device (M3-11, `fynkel_world_audio_open`): false
  /// when the platform has none or the build carries no device backend
  /// (`UNSUPPORTED`; the world keeps the null backend and reports
  /// `audio_device` in its diagnostics).
  bool audioOpen() {
    ffiCalls++;
    final s = fynkel_world_audio_open(_w);
    if (s == FynkelStatus.FYNKEL_STATUS_UNSUPPORTED) return false;
    _check(s, 'audio_open');
    return true;
  }

  /// Pauses or resumes the output device (M3-11); a no-op without one.
  void audioSuspend(bool suspended) {
    ffiCalls++;
    _check(fynkel_world_audio_suspend(_w, suspended ? 1 : 0), 'audio_suspend');
  }

  /// Closes the output device (M3-11); a no-op without one.
  void audioClose() {
    ffiCalls++;
    _check(fynkel_world_audio_close(_w), 'audio_close');
  }

  BridgeStats stats() {
    ffiCalls++;
    _check(fynkel_world_stats(_w, _stats), 'stats');
    final s = _stats.ref;
    return BridgeStats(
      tick: s.tick,
      frame: s.frame,
      stepCalls: s.step_calls,
      queryCalls: s.query_calls,
      bytesIn: s.bytes_in,
      bytesOut: s.bytes_out,
      liveEntities: s.live_entities,
      subscriptions: s.subscriptions,
      poisoned: s.poisoned != 0,
      ffiCalls: ffiCalls,
      dartCopies: dartCopies,
      dartBytesCopied: dartBytesCopied,
      snapshotBytes: s.snapshot_bytes,
      snapshotPeakBytes: s.snapshot_peak_bytes,
      snapshotBudget: s.snapshot_budget,
      nativeBytes: s.native_bytes,
      residentBytes: s.resident_bytes,
      residencyBytes: s.residency_bytes,
    );
  }

  /// The subscription budget (C5): a ceiling in bytes on a tick's
  /// snapshot; the world reports `snapshot_budget` once per crossing.
  /// 0 removes it.
  void setSnapshotBudget(int bytes) {
    ffiCalls++;
    _check(fynkel_world_set_snapshot_budget(_w, bytes), 'set_snapshot_budget');
  }

  void _check(FynkelStatus s, String op) {
    if (s != FynkelStatus.FYNKEL_STATUS_OK) {
      throw failure('fynkel_world_$op', s);
    }
  }

  void dispose() {
    if (_ptr == nullptr) return;
    fynkel_world_destroy(_ptr);
    _ptr = nullptr;
    calloc.free(_plan);
    calloc.free(_step);
    calloc.free(_query);
    calloc.free(_view);
    calloc.free(_stats);
    calloc.free(_projection);
    if (_scratch != nullptr) calloc.free(_scratch);
    _scratch = nullptr;
    _scratchLen = 0;
    _scratchView = Uint8List(0);
  }
}

void _writeNativeEntity(FynkelEntity native, EntityRef value) {
  native.world = value.world;
  native.slot = value.slot;
  native.generation = value.generation;
}

EntityRef _readNativeEntity(FynkelEntity value) => EntityRef(value.world, value.slot, value.generation);
