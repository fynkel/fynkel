// Thin typed layer over the ffigen bindings (ABI v1). One FFI call per
// operation family (blueprint §5.3); buffers in and out, no per-property
// calls. The gameplay API in lib/src/world.dart builds on this.

import 'dart:convert';
import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import '../../generated/schema.g.dart' as gen;
import '../wire/buffers.dart';
import '../wire/support.dart' show WireStrings;
import 'fynkel_bindings.g.dart';

export 'fynkel_bindings.g.dart' show FynkelStatus;

enum CommandLane { simulation, presentation }

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
  animatorParameters
}

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
    this.snapshotBytes = 0,
    this.snapshotPeakBytes = 0,
    this.snapshotBudget = 0,
    this.nativeBytes = 0,
    this.residentBytes = 0,
    this.residencyBytes = 0,
  });
  final int tick, frame, submitCalls, stepCalls, queryCalls, bytesIn, bytesOut;
  final int liveEntities, subscriptions, stagedCommands;
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
        _stats = calloc<FynkelWorldStats>(),
        _projection = calloc<FynkelProjectionInfo>();

  Pointer<FynkelWorld> _w;
  final Pointer<FynkelFramePlan> _plan;
  final Pointer<FynkelSubmitResult> _submit;
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
  // Queries: one cache per kind, so an `entities` query and the
  // `components` queries a tick makes do not evict each other's view.
  final _queryViews = [for (final _ in QueryKind.values) _ViewCache()];
  final _inputView = _ViewCache();
  final _physicsView = _ViewCache();
  final _projectionView = _ViewCache();

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
    final b = _wrap(_view.ref, _queryViews[kind.index]);
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

  /// Projects a `ProjectionAnchors` buffer (the first [length] bytes of
  /// [bytes], M3-13) into a [width] × [height] viewport: one memcpy into
  /// the reused scratch, one FFI call; the points come back as
  /// `ProjectedPointRecord`s in a query buffer read in place, valid until
  /// the next query or mutating call, with the camera entity (0 = none)
  /// and the projected/dropped counts.
  (WireBuffer, int camera, int projected, int dropped) project(Uint8List bytes, int length, int width, int height) {
    _copyToScratch(bytes, length);
    ffiCalls++;
    final s = fynkel_world_project(_w, width, height, _scratch, length, _projection, _view);
    _check(s, 'project');
    final info = _projection.ref;
    return (_wrap(_view.ref, _projectionView), info.camera, info.projected, info.dropped);
  }

  void _copyToScratch(Uint8List bytes, int length) {
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
      if (fynkel_world_load_progress(_w, ticket, state, progress) != FynkelStatus.FYNKEL_STATUS_OK) return null;
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
  FynkelStatus dontDestroyOnLoad(int entity) => fynkel_world_dont_destroy_on_load(_w, entity);

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

  /// `fynkel_world_clone` (ADR-0026 §4): a copy of the live subtree
  /// under [source]; the report's `clone` has the pairs.
  (FynkelStatus, Map<String, Object?>) clone(
    int source,
    String name,
    int parent,
    List<double> position,
    List<double> rotation,
    List<double> scale,
  ) =>
      _withUtf8(name, (n, nl) {
        final d = calloc<FynkelCloneDesc>();
        try {
          d.ref.source = source;
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
      if (material == null) return fynkel_world_set_material(_w, idp, nullptr, 0);
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
    calloc.free(_projection);
    if (_scratch != nullptr) calloc.free(_scratch);
  }
}
