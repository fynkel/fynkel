// Thin Dart layer over the generated ABI v0 bindings: command-buffer
// encoding, buffer-view decoding, status mapping. This is the seed of the
// SDK's bridge (blueprint §5.3–5.5); the record codecs here are hand-written
// for v0 and will be generated from the Rust schemas (ADR-0012).

import 'dart:ffi';
import 'dart:io';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'fynkel_bindings.g.dart';

export 'fynkel_bindings.g.dart' show FynkelStatus;

const int _abiVersion = 0;
const int _bufferMagic = 0x4b4e5946; // "FYNK"
const int _headerSize = 16;
const int _commandSize = 64;
const int _snapshotTransformSize = 48;
const int _eventSize = 32;
const int _diagnosticSize = 32;

/// Schema fingerprint the hand-written v0 codecs in this file implement.
const int schemaFingerprint = 0x0000000100000003;

enum CommandLane { simulation, presentation }

class Transform {
  const Transform({
    this.position = const [0, 0, 0],
    this.rotation = const [0, 0, 0, 1],
    this.scale = const [1, 1, 1],
  });
  final List<double> position, rotation, scale;

  @override
  String toString() => 'Transform(p=$position r=$rotation s=$scale)';
}

/// Growable, reusable batch of commands (one `Uint8List` per submit).
class CommandBuffer {
  CommandBuffer([int initialCapacity = 64]) : _bytes = ByteData(_headerSize + initialCapacity * _commandSize);

  ByteData _bytes;
  int _count = 0;

  int get length => _count;

  void clear() => _count = 0;

  int _reserve() {
    final needed = _headerSize + (_count + 1) * _commandSize;
    if (needed > _bytes.lengthInBytes) {
      final grown = ByteData(_bytes.lengthInBytes * 2);
      grown.buffer.asUint8List().setRange(0, _bytes.lengthInBytes, _bytes.buffer.asUint8List());
      _bytes = grown;
    }
    final off = _headerSize + _count * _commandSize;
    _count++;
    // Zero the record so reserved/padding bytes are deterministic.
    _bytes.buffer.asUint8List(off, _commandSize).fillRange(0, _commandSize, 0);
    return off;
  }

  void _header(int off, int kind, int target) {
    _bytes.setUint32(off, kind, Endian.little);
    _bytes.setUint32(off + 4, 0, Endian.little);
    _bytes.setUint64(off + 8, target, Endian.little);
  }

  void _transform(int off, Transform t) {
    var o = off;
    for (final v in [...t.position, ...t.rotation, ...t.scale]) {
      _bytes.setFloat32(o, v, Endian.little);
      o += 4;
    }
  }

  /// Spawns an entity; the step's events map [tempId] to its handle.
  void spawn(int tempId, Transform initial) {
    final off = _reserve();
    _header(off, 1, tempId);
    _transform(off + 16, initial);
  }

  void destroy(int entity) => _header(_reserve(), 2, entity);

  void setTransform(int entity, Transform t) {
    final off = _reserve();
    _header(off, 3, entity);
    _transform(off + 16, t);
  }

  /// Finalizes the header and returns a view of the encoded bytes.
  Uint8List encoded() {
    final len = _headerSize + _count * _commandSize;
    _bytes.setUint32(0, _bufferMagic, Endian.little);
    _bytes.setUint16(4, _abiVersion, Endian.little);
    _bytes.setUint16(6, 1, Endian.little); // FYNKEL_BUFFER_COMMANDS
    _bytes.setUint32(8, _count, Endian.little);
    _bytes.setUint32(12, len, Endian.little);
    return _bytes.buffer.asUint8List(0, len);
  }
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

class SnapshotTransform {
  const SnapshotTransform(this.entity, this.transform);
  final int entity;
  final Transform transform;
}

class FynkelEvent {
  const FynkelEvent(this.kind, this.tickLo, this.a, this.b, this.c);
  final int kind, tickLo, a, b, c;
  bool get isSpawned => kind == 1;
  bool get isDestroyed => kind == 2;
}

class Diagnostic {
  const Diagnostic(this.commandIndex, this.code, this.entity, this.component, this.lane, this.message);
  final int commandIndex, code, entity, component, lane;
  final String message;

  @override
  String toString() => 'Diagnostic(#$commandIndex code=$code entity=$entity lane=$lane "$message")';
}

/// A read-only view over one of the world's output buffers. Valid until the
/// next mutating call on the world (see fynkel.h); decode or copy before then.
class BufferView {
  BufferView._(this.bytes);
  final Uint8List bytes;

  int get recordCount => bytes.length < _headerSize ? 0 : ByteData.sublistView(bytes).getUint32(8, Endian.little);
  int get byteLength => bytes.length;

  ByteData get _d => ByteData.sublistView(bytes);

  Transform _transformAt(int o) {
    final d = _d;
    double f(int i) => d.getFloat32(o + i * 4, Endian.little);
    return Transform(
      position: [f(0), f(1), f(2)],
      rotation: [f(3), f(4), f(5), f(6)],
      scale: [f(7), f(8), f(9)],
    );
  }

  Iterable<SnapshotTransform> transforms() sync* {
    final d = _d;
    for (var i = 0; i < recordCount; i++) {
      final o = _headerSize + i * _snapshotTransformSize;
      yield SnapshotTransform(d.getUint64(o, Endian.little), _transformAt(o + 8));
    }
  }

  /// Zero-decode access: entity handle of snapshot record [i].
  int snapshotEntity(int i) => _d.getUint64(_headerSize + i * _snapshotTransformSize, Endian.little);

  /// Zero-decode access: position.x of snapshot record [i].
  double snapshotPositionX(int i) => _d.getFloat32(_headerSize + i * _snapshotTransformSize + 8, Endian.little);

  Iterable<FynkelEvent> events() sync* {
    final d = _d;
    for (var i = 0; i < recordCount; i++) {
      final o = _headerSize + i * _eventSize;
      yield FynkelEvent(
        d.getUint32(o, Endian.little),
        d.getUint32(o + 4, Endian.little),
        d.getUint64(o + 8, Endian.little),
        d.getUint64(o + 16, Endian.little),
        d.getUint64(o + 24, Endian.little),
      );
    }
  }

  Iterable<Diagnostic> diagnostics() sync* {
    final d = _d;
    for (var i = 0; i < recordCount; i++) {
      final o = _headerSize + i * _diagnosticSize;
      final off = d.getUint32(o + 20, Endian.little);
      final len = d.getUint32(o + 24, Endian.little);
      yield Diagnostic(
        d.getUint32(o, Endian.little),
        d.getUint32(o + 4, Endian.little),
        d.getUint64(o + 8, Endian.little),
        d.getUint32(o + 16, Endian.little),
        d.getUint32(o + 28, Endian.little),
        String.fromCharCodes(bytes, off, off + len),
      );
    }
  }
}

class FynkelNative {
  FynkelNative._(this._b, this._runtime);

  final FynkelNativeBindings _b;
  final Pointer<FynkelRuntime> _runtime;

  static FynkelNative open(String libraryPath) {
    final b = FynkelNativeBindings(DynamicLibrary.open(libraryPath));
    final v = b.fynkel_abi_version();
    if (v != _abiVersion) throw StateError('fynkel_native ABI version $v, this SDK speaks $_abiVersion');
    final fp = b.fynkel_schema_fingerprint();
    if (fp != schemaFingerprint) {
      throw StateError('schema fingerprint mismatch: library ${fp.toRadixString(16)} vs SDK ${schemaFingerprint.toRadixString(16)}');
    }
    final rt = b.fynkel_runtime_create();
    if (rt == nullptr) throw StateError('fynkel_runtime_create failed');
    return FynkelNative._(b, rt);
  }

  /// `FYNKEL_NATIVE_LIB` env var, else the workspace's release build.
  static String defaultLibraryPath() {
    final env = Platform.environment['FYNKEL_NATIVE_LIB'];
    if (env != null && env.isNotEmpty) return env;
    var dir = Directory.current;
    while (true) {
      final candidate = '${dir.path}/target/release/libfynkel_native.dylib';
      if (File(candidate).existsSync()) return candidate;
      final parent = dir.parent;
      if (parent.path == dir.path) break;
      dir = parent;
    }
    throw StateError('libfynkel_native.dylib not found; run `cargo build --release -p fynkel_native` or set FYNKEL_NATIVE_LIB');
  }

  NativeWorld createWorld({int initialCapacity = 0}) {
    final desc = calloc<FynkelWorldDesc>();
    try {
      desc.ref.initial_entity_capacity = initialCapacity;
      final w = _b.fynkel_world_create(_runtime, desc);
      if (w == nullptr) throw StateError('fynkel_world_create failed');
      return NativeWorld._(_b, w);
    } finally {
      calloc.free(desc);
    }
  }

  void dispose() => _b.fynkel_runtime_destroy(_runtime);
}

class NativeWorld {
  NativeWorld._(this._b, this._w)
      : _stepDesc = calloc<FynkelStepDesc>(),
        _stepResult = calloc<FynkelStepResult>();

  final FynkelNativeBindings _b;
  Pointer<FynkelWorld> _w;
  final Pointer<FynkelStepDesc> _stepDesc;
  final Pointer<FynkelStepResult> _stepResult;

  // Reused native scratch for command bytes (grown on demand) so a submit is
  // one memcpy + one FFI call, no allocation in steady state.
  Pointer<Uint8> _scratch = nullptr;
  int _scratchLen = 0;

  bool get isPoisoned => _b.fynkel_world_is_poisoned(_w) != 0;

  FynkelStatus submit(CommandLane lane, CommandBuffer commands) => submitRaw(lane, commands.encoded());

  /// Submits an already-encoded command buffer (generated codecs, tests).
  FynkelStatus submitRaw(CommandLane lane, Uint8List bytes) {
    if (bytes.length > _scratchLen) {
      if (_scratch != nullptr) calloc.free(_scratch);
      _scratchLen = bytes.length * 2;
      _scratch = calloc<Uint8>(_scratchLen);
    }
    _scratch.asTypedList(_scratchLen).setRange(0, bytes.length, bytes);
    return _b.fynkel_world_submit_commands(_w, lane.index, _scratch, bytes.length);
  }

  StepResult step({double fixedDt = 1 / 60}) {
    _stepDesc.ref.fixed_dt_seconds = fixedDt;
    final s = _b.fynkel_world_step(_w, _stepDesc, _stepResult);
    final r = _stepResult.ref;
    return StepResult(s, r.tick, r.commands_applied, r.commands_rejected, r.snapshot_records, r.event_records,
        r.diagnostic_records, r.live_entities);
  }

  FynkelStatus subscribeTransforms(List<int> entities, {bool enable = true}) {
    final p = calloc<Uint64>(entities.length);
    try {
      p.asTypedList(entities.length).setAll(0, entities);
      return _b.fynkel_world_subscribe(_w, 1, p, entities.length, enable ? 1 : 0);
    } finally {
      calloc.free(p);
    }
  }

  BufferView _view(FynkelBufferView v) =>
      BufferView._(v.length == 0 ? Uint8List(0) : v.data.asTypedList(v.length));

  BufferView get snapshot => _view(_b.fynkel_world_snapshot(_w));
  BufferView get events => _view(_b.fynkel_world_events(_w));
  BufferView get diagnostics => _view(_b.fynkel_world_diagnostics(_w));

  void dispose() {
    if (_w == nullptr) return;
    _b.fynkel_world_destroy(_w);
    _w = nullptr;
    calloc.free(_stepDesc);
    calloc.free(_stepResult);
    if (_scratch != nullptr) calloc.free(_scratch);
  }
}
