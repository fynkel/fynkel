// Buffer framing for ABI v3 (crates/fynkel_native/include/fynkel.h):
// 24-byte header, record area, string tail. Record bodies use the
// generated codecs (lib/generated/schema.g.dart).

import 'dart:typed_data';

import '../entity_ref.dart';

import '../../generated/schema.g.dart';
import '../../math.dart' show Vec3;
import '../asset_ref.dart';
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
    final d = data;
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

  /// The whole buffer, header included, as one [ByteData] made once.
  late final ByteData data = ByteData.sublistView(bytes);

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

  /// Raw 16-byte entity records (handle lists from queries).
  List<EntityRef> entities() => [for (var i = 0; i < recordCount; i++) readEntity(records, i * 16)];
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
  void setTransformOf(EntityRef entity, Transform t) {
    final o = _begin(SetTransformCommand.commandKind, SetTransformCommand.wireSize);
    writeEntity(_bytes, o, entity);
    t.encode(_bytes, o + 16, _strings);
  }

  /// Same command from a reusable writer: no value objects at all.
  void setTransformFrom(EntityRef entity, TransformWriter w) {
    final o = _begin(SetTransformCommand.commandKind, SetTransformCommand.wireSize);
    writeEntity(_bytes, o, entity);
    w.encode(_bytes, o + 16);
  }

  void setParent(SetParentCommand c) => command(SetParentCommand.commandKind, SetParentCommand.wireSize, c);
  void setName(SetNameCommand c) => command(SetNameCommand.commandKind, SetNameCommand.wireSize, c);
  void setActive(SetActiveCommand c) => command(SetActiveCommand.commandKind, SetActiveCommand.wireSize, c);

  /// Inserts or replaces a native component (M3-07a): the
  /// `SetComponentCommand` header then the component's own bytes, one
  /// command; [codec] is the component's entry in `componentCodecs`.
  void setComponent(EntityRef entity, ComponentCodec codec, WireRecord value) {
    final o = _begin(SetComponentCommand.commandKind, SetComponentCommand.wireSize + codec.wireSize);
    writeEntity(_bytes, o, entity);
    _bytes.setUint32(o + 16, codec.id, Endian.little);
    value.encode(_bytes, o + SetComponentCommand.wireSize, _strings);
  }

  /// A body command (M3-08, kind 8): [op] with its vector and, for
  /// `setKinematicTarget`/`teleport`, the rotation. Allocation-free.
  void body(EntityRef entity, BodyOp op, double x, double y, double z,
      [double qx = 0, double qy = 0, double qz = 0, double qw = 1]) {
    final o = _begin(BodyCommandRecord.commandKind, BodyCommandRecord.wireSize);
    writeEntity(_bytes, o, entity);
    _bytes.setUint32(o + 16, op.index, Endian.little);
    _bytes.setFloat32(o + 20, x, Endian.little);
    _bytes.setFloat32(o + 24, y, Endian.little);
    _bytes.setFloat32(o + 28, z, Endian.little);
    _bytes.setFloat32(o + 32, qx, Endian.little);
    _bytes.setFloat32(o + 36, qy, Endian.little);
    _bytes.setFloat32(o + 40, qz, Endian.little);
    _bytes.setFloat32(o + 44, qw, Endian.little);
  }

  /// An animator command (M3-19, kind 9): [op] with the fields it reads —
  /// `play`/`crossfade` the clip, `looping` and `speed` (and the fade
  /// [seconds] for a crossfade); `setSpeed` [speed]; `setTime` [seconds].
  void animate(EntityRef entity, AnimateOp op,
      {AssetRef clip = AssetRef.none,
      double seconds = 0,
      double speed = 1,
      bool looping = true,
      double value = 0,
      String name = '',
      int integer = 0,
      int layer = -1,
      Vec3 point = Vec3.zero}) {
    final o = _begin(AnimateCommandRecord.commandKind, AnimateCommandRecord.wireSize);
    AnimateCommandRecord(
            entity: entity,
            op: op,
            seconds: seconds,
            speed: speed,
            looping: looping,
            clip: clip,
            value: value,
            name: name,
            integer: integer,
            layer: layer,
            point: point)
        .encode(_bytes, o, _strings);
  }

  /// An audio source command (M3-11, kind 10): [op] on [entity]'s
  /// `fynkel.AudioSource`; `setVolume`/`setPitch` read [value].
  void audio(EntityRef entity, AudioOp op, {double value = 0}) {
    final o = _begin(AudioCommandRecord.commandKind, AudioCommandRecord.wireSize);
    AudioCommandRecord(entity: entity, op: op, value: value).encode(_bytes, o, _strings);
  }

  /// A fire-and-forget sound (M3-11, kind 11; either lane).
  void playSound(AssetRef clip,
      {AudioBus bus = AudioBus.sfx, double volume = 1, double pitch = 1, int priority = 0}) {
    final o = _begin(PlaySoundCommandRecord.commandKind, PlaySoundCommandRecord.wireSize);
    PlaySoundCommandRecord(clip: clip, bus: bus, volume: volume, pitch: pitch, priority: priority)
        .encode(_bytes, o, _strings);
  }

  /// A bus gain and the mute switch (M3-11, kind 12; either lane).
  void audioBus(AudioBus bus, double volume, {bool muted = false}) {
    final o = _begin(AudioBusCommandRecord.commandKind, AudioBusCommandRecord.wireSize);
    AudioBusCommandRecord(bus: bus, volume: volume, muted: muted).encode(_bytes, o, _strings);
  }

  void removeComponent(EntityRef entity, int componentId) {
    final o = _begin(RemoveComponentCommand.commandKind, RemoveComponentCommand.wireSize);
    writeEntity(_bytes, o, entity);
    _bytes.setUint32(o + 16, componentId, Endian.little);
  }

  /// Finalizes header and tail; returns a view (valid until [clear]).
  Uint8List encoded() {
    // finalize() first: it may grow the buffer, replacing _view.
    final length = finalize();
    return Uint8List.sublistView(_view, 0, length);
  }

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

  /// The buffer the encoded bytes are in. Read it after [finalize]: the
  /// string tail may grow it, and a view read before is the old one.
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
    _view.fillRange(_len, _len + recordSize, 0);
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

  Uint8List encoded() {
    // finalize() first: it may grow the buffer, replacing _view.
    final length = finalize();
    return Uint8List.sublistView(_view, 0, length);
  }

  /// The buffer the encoded bytes are in. Read it after [finalize]: the
  /// string tail may grow it, and a view read before is the old one.
  Uint8List get backingStore => _view;
}
