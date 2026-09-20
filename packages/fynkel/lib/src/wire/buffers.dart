// Buffer framing for ABI v1 (crates/fynkel_native/include/fynkel.h):
// 24-byte header, record area, string tail. Record bodies use the
// generated codecs (lib/generated/schema.g.dart).

import 'dart:typed_data';

import '../../generated/schema.g.dart';
import '../transform_writer.dart';
import 'support.dart';

const int bufferMagic = 0x4b4e5946; // "FYNK"
const int bufferHeaderSize = 24;

enum BufferKind {
  commands,
  snapshot,
  events,
  diagnostics,
  query,
  inputEvents,
  inputActions,
  physicsQueries,
  projectionAnchors,
}

int bufferKindValue(BufferKind k) => k.index + 1;

/// Read-only view over one native output buffer. Valid until the next
/// mutating call on the world; decode or copy before then.
class WireBuffer {
  WireBuffer(this.bytes) {
    if (bytes.length < bufferHeaderSize) {
      throw FormatException('buffer shorter than its header (${bytes.length} bytes)');
    }
    reparse();
  }

  /// Re-reads the header (and re-slices records/tail when the layout moved)
  /// after the native side rewrote the same memory. The bridge keeps one
  /// [WireBuffer] per output family and reparses it while the view's
  /// address and length are unchanged, so a steady-state frame allocates
  /// no views (M1 gate).
  void reparse() {
    final d = ByteData.sublistView(bytes);
    if (d.getUint32(0, Endian.little) != bufferMagic) throw const FormatException('bad buffer magic');
    final version = d.getUint16(4, Endian.little);
    if (version != abiVersion) throw FormatException('buffer ABI version $version, expected $abiVersion');
    kind = d.getUint16(6, Endian.little);
    recordCount = d.getUint32(8, Endian.little);
    final byteLength = d.getUint32(12, Endian.little);
    if (byteLength != bytes.length) {
      throw FormatException('buffer declares $byteLength bytes, view has ${bytes.length}');
    }
    final end = d.getUint32(16, Endian.little);
    if (end < bufferHeaderSize || end > bytes.length) {
      throw FormatException('bad records_end $end');
    }
    requestId = d.getUint32(20, Endian.little);
    if (end != recordsEnd) {
      recordsEnd = end;
      records = ByteData.sublistView(bytes, bufferHeaderSize, end);
      tail = Uint8List.sublistView(bytes, end);
    }
  }

  static final WireBuffer empty = WireBuffer(_emptyBytes());

  static Uint8List _emptyBytes() {
    final b = Uint8List(bufferHeaderSize);
    final d = ByteData.sublistView(b);
    d.setUint32(0, bufferMagic, Endian.little);
    d.setUint16(4, abiVersion, Endian.little);
    d.setUint32(12, bufferHeaderSize, Endian.little);
    d.setUint32(16, bufferHeaderSize, Endian.little);
    return b;
  }

  final Uint8List bytes;
  int kind = 0;
  int recordCount = 0;
  int recordsEnd = -1;
  int requestId = 0;
  late ByteData records;
  late Uint8List tail;

  int get byteLength => bytes.length;

  /// Fixed-size records decoded with [decode] at each offset.
  Iterable<T> fixed<T>(int size, T Function(ByteData d, int o, Uint8List tail) decode) sync* {
    for (var i = 0; i < recordCount; i++) {
      yield decode(records, i * size, tail);
    }
  }

  /// Raw `u64` records (handle lists from queries).
  List<int> u64s() => [for (var i = 0; i < recordCount; i++) records.getUint64(i * 8, Endian.little)];
}

/// Reusable command batch. Zero allocations in steady state: the backing
/// buffer grows to the largest batch and is reused; strings are appended
/// to a tail after the records.
class CommandBatch {
  CommandBatch([int initialCapacity = 4096])
      : _bytes = ByteData(initialCapacity),
        _view = Uint8List(0) {
    _view = _bytes.buffer.asUint8List();
  }

  ByteData _bytes;

  /// Whole backing store as bytes (rebuilt only when it grows).
  Uint8List _view;
  int _len = bufferHeaderSize;
  int _count = 0;
  final WireStrings _strings = WireStrings();

  int get length => _count;
  bool get isEmpty => _count == 0;

  void clear() {
    _len = bufferHeaderSize;
    _count = 0;
    _strings.clear();
  }

  void _reserve(int extra) {
    if (_len + extra <= _bytes.lengthInBytes) return;
    var cap = _bytes.lengthInBytes * 2;
    while (cap < _len + extra) {
      cap *= 2;
    }
    final grown = ByteData(cap);
    grown.buffer.asUint8List().setRange(0, _len, _view);
    _bytes = grown;
    _view = grown.buffer.asUint8List();
  }

  /// Appends one command: header + [bodySize] bytes written by [record].
  /// Allocation-free (M1 gate): header fields are written in place and the
  /// record encodes through its interface, not a tear-off.
  void command(int kind, int bodySize, WireRecord record) {
    final o = _begin(kind, bodySize);
    record.encode(_bytes, o, _strings);
  }

  int _begin(int kind, int bodySize) {
    _reserve(CommandHeader.wireSize + bodySize);
    _bytes.setUint32(_len, kind, Endian.little);
    _bytes.setUint32(_len + 4, bodySize, Endian.little);
    _len += CommandHeader.wireSize;
    _bytes.buffer.asUint8List(_len, bodySize).fillRange(0, bodySize, 0);
    final o = _len;
    _len += bodySize;
    _count++;
    return o;
  }

  void spawn(SpawnCommand c) => command(SpawnCommand.commandKind, SpawnCommand.wireSize, c);
  void destroy(DestroyCommand c) => command(DestroyCommand.commandKind, DestroyCommand.wireSize, c);
  void setTransform(SetTransformCommand c) => command(SetTransformCommand.commandKind, SetTransformCommand.wireSize, c);

  /// The hot path (one per moving object per tick) without the command
  /// record: entity, then the transform, as `SetTransformCommand.encode`
  /// lays them out.
  void setTransformOf(int entity, Transform t) {
    final o = _begin(SetTransformCommand.commandKind, SetTransformCommand.wireSize);
    _bytes.setUint64(o, entity, Endian.little);
    t.encode(_bytes, o + 8, _strings);
  }

  /// Same command from a reusable writer: no value objects at all.
  void setTransformFrom(int entity, TransformWriter w) {
    final o = _begin(SetTransformCommand.commandKind, SetTransformCommand.wireSize);
    _bytes.setUint64(o, entity, Endian.little);
    w.encode(_bytes, o + 8);
  }

  void setParent(SetParentCommand c) => command(SetParentCommand.commandKind, SetParentCommand.wireSize, c);
  void setName(SetNameCommand c) => command(SetNameCommand.commandKind, SetNameCommand.wireSize, c);

  /// Inserts or replaces a native component (M3-07a): the
  /// `SetComponentCommand` header then the component's own bytes, one
  /// command; [codec] is the component's entry in `componentCodecs`.
  void setComponent(int entity, ComponentCodec codec, WireRecord value) {
    final o = _begin(SetComponentCommand.commandKind, SetComponentCommand.wireSize + codec.wireSize);
    _bytes.setUint64(o, entity, Endian.little);
    _bytes.setUint32(o + 8, codec.id, Endian.little);
    value.encode(_bytes, o + SetComponentCommand.wireSize, _strings);
  }

  /// A body command (M3-08, kind 8): [op] with its vector and, for
  /// `setKinematicTarget`/`teleport`, the rotation. Allocation-free.
  void body(int entity, BodyOp op, double x, double y, double z,
      [double qx = 0, double qy = 0, double qz = 0, double qw = 1]) {
    final o = _begin(BodyCommandRecord.commandKind, BodyCommandRecord.wireSize);
    _bytes.setUint64(o, entity, Endian.little);
    _bytes.setUint32(o + 8, op.index, Endian.little);
    _bytes.setFloat32(o + 12, x, Endian.little);
    _bytes.setFloat32(o + 16, y, Endian.little);
    _bytes.setFloat32(o + 20, z, Endian.little);
    _bytes.setFloat32(o + 24, qx, Endian.little);
    _bytes.setFloat32(o + 28, qy, Endian.little);
    _bytes.setFloat32(o + 32, qz, Endian.little);
    _bytes.setFloat32(o + 36, qw, Endian.little);
  }

  void removeComponent(int entity, int componentId) {
    final o = _begin(RemoveComponentCommand.commandKind, RemoveComponentCommand.wireSize);
    _bytes.setUint64(o, entity, Endian.little);
    _bytes.setUint32(o + 8, componentId, Endian.little);
  }

  /// Finalizes header and tail; returns a view (valid until [clear]).
  Uint8List encoded() => Uint8List.sublistView(_view, 0, finalize());

  /// Finalizes header and tail in place and returns the encoded length; the
  /// bytes are the first [finalize] bytes of [backingStore]. This is the
  /// allocation-free form [encoded] wraps.
  int finalize() {
    final recordsEnd = _len;
    var total = _len;
    if (_strings.length != 0) {
      final tail = _strings.takeBytes();
      _reserve(tail.length);
      _view.setRange(_len, _len + tail.length, tail);
      total += tail.length;
    }
    _bytes.setUint32(0, bufferMagic, Endian.little);
    _bytes.setUint16(4, abiVersion, Endian.little);
    _bytes.setUint16(6, bufferKindValue(BufferKind.commands), Endian.little);
    _bytes.setUint32(8, _count, Endian.little);
    _bytes.setUint32(12, total, Endian.little);
    _bytes.setUint32(16, recordsEnd, Endian.little);
    _bytes.setUint32(20, 0, Endian.little);
    return total;
  }

  Uint8List get backingStore => _view;
}

/// Reusable batch of fixed-size records (no per-record header): the input
/// event buffer a host captures once per frame. Same reuse discipline as
/// [CommandBatch].
class RecordBatch {
  RecordBatch(this.kind, this.recordSize, [int initialCapacity = 1024])
      : _bytes = ByteData(initialCapacity),
        _view = Uint8List(0) {
    _view = _bytes.buffer.asUint8List();
  }

  final BufferKind kind;
  final int recordSize;
  ByteData _bytes;
  Uint8List _view;
  int _len = bufferHeaderSize;
  int _count = 0;
  final WireStrings _strings = WireStrings();

  int get length => _count;
  bool get isEmpty => _count == 0;

  void clear() {
    _len = bufferHeaderSize;
    _count = 0;
    _strings.clear();
  }

  void _reserve(int extra) {
    if (_len + extra <= _bytes.lengthInBytes) return;
    var cap = _bytes.lengthInBytes * 2;
    while (cap < _len + extra) {
      cap *= 2;
    }
    final grown = ByteData(cap);
    grown.buffer.asUint8List().setRange(0, _len, _view);
    _bytes = grown;
    _view = grown.buffer.asUint8List();
  }

  void add(WireRecord record) {
    _reserve(recordSize);
    _bytes.buffer.asUint8List(_len, recordSize).fillRange(0, recordSize, 0);
    record.encode(_bytes, _len, _strings);
    _len += recordSize;
    _count++;
  }

  /// Finalizes header and tail in place; the bytes are the first
  /// [finalize] bytes of [backingStore].
  int finalize() {
    final recordsEnd = _len;
    var total = _len;
    if (_strings.length != 0) {
      final tail = _strings.takeBytes();
      _reserve(tail.length);
      _view.setRange(_len, _len + tail.length, tail);
      total += tail.length;
    }
    _bytes.setUint32(0, bufferMagic, Endian.little);
    _bytes.setUint16(4, abiVersion, Endian.little);
    _bytes.setUint16(6, bufferKindValue(kind), Endian.little);
    _bytes.setUint32(8, _count, Endian.little);
    _bytes.setUint32(12, total, Endian.little);
    _bytes.setUint32(16, recordsEnd, Endian.little);
    _bytes.setUint32(20, 0, Endian.little);
    return total;
  }

  Uint8List encoded() => Uint8List.sublistView(_view, 0, finalize());

  Uint8List get backingStore => _view;
}
