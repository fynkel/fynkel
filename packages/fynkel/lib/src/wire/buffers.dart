// Buffer framing for ABI v1 (crates/fynkel_native/include/fynkel.h):
// 24-byte header, record area, string tail. Record bodies use the
// generated codecs (lib/generated/schema.g.dart).

import 'dart:typed_data';

import '../../generated/schema.g.dart';
import 'support.dart';

const int bufferMagic = 0x4b4e5946; // "FYNK"
const int bufferHeaderSize = 24;

enum BufferKind { commands, snapshot, events, diagnostics, query }

int bufferKindValue(BufferKind k) => k.index + 1;

/// Read-only view over one native output buffer. Valid until the next
/// mutating call on the world; decode or copy before then.
class WireBuffer {
  WireBuffer(this.bytes) {
    if (bytes.length < bufferHeaderSize) {
      throw FormatException('buffer shorter than its header (${bytes.length} bytes)');
    }
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
    recordsEnd = d.getUint32(16, Endian.little);
    if (recordsEnd < bufferHeaderSize || recordsEnd > bytes.length) {
      throw FormatException('bad records_end $recordsEnd');
    }
    requestId = d.getUint32(20, Endian.little);
    records = ByteData.sublistView(bytes, bufferHeaderSize, recordsEnd);
    tail = Uint8List.sublistView(bytes, recordsEnd);
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
  late final int kind;
  late final int recordCount;
  late final int recordsEnd;
  late final int requestId;
  late final ByteData records;
  late final Uint8List tail;

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
  CommandBatch([int initialCapacity = 4096]) : _bytes = ByteData(initialCapacity);

  ByteData _bytes;
  int _len = bufferHeaderSize;
  int _count = 0;
  final WireStrings _strings = WireStrings();

  int get length => _count;
  bool get isEmpty => _count == 0;

  void clear() {
    _len = bufferHeaderSize;
    _count = 0;
    _strings.takeBytes();
  }

  void _reserve(int extra) {
    if (_len + extra <= _bytes.lengthInBytes) return;
    var cap = _bytes.lengthInBytes * 2;
    while (cap < _len + extra) {
      cap *= 2;
    }
    final grown = ByteData(cap);
    grown.buffer.asUint8List().setRange(0, _len, _bytes.buffer.asUint8List());
    _bytes = grown;
  }

  /// Appends one command: header + [bodySize] bytes written by [encode].
  void command(int kind, int bodySize, void Function(ByteData d, int o, WireStrings s) encode) {
    _reserve(CommandHeader.wireSize + bodySize);
    CommandHeader(kind: kind, size: bodySize).encode(_bytes, _len, _strings);
    _len += CommandHeader.wireSize;
    _bytes.buffer.asUint8List(_len, bodySize).fillRange(0, bodySize, 0);
    encode(_bytes, _len, _strings);
    _len += bodySize;
    _count++;
  }

  void spawn(SpawnCommand c) => command(SpawnCommand.commandKind, SpawnCommand.wireSize, c.encode);
  void destroy(DestroyCommand c) => command(DestroyCommand.commandKind, DestroyCommand.wireSize, c.encode);
  void setTransform(SetTransformCommand c) =>
      command(SetTransformCommand.commandKind, SetTransformCommand.wireSize, c.encode);
  void setParent(SetParentCommand c) => command(SetParentCommand.commandKind, SetParentCommand.wireSize, c.encode);
  void setName(SetNameCommand c) => command(SetNameCommand.commandKind, SetNameCommand.wireSize, c.encode);

  /// Finalizes header and tail; returns a view (valid until [clear]).
  Uint8List encoded() {
    final tail = _strings.takeBytes();
    _reserve(tail.length);
    final recordsEnd = _len;
    _bytes.buffer.asUint8List(_len, tail.length).setAll(0, tail);
    final total = _len + tail.length;
    _bytes.setUint32(0, bufferMagic, Endian.little);
    _bytes.setUint16(4, abiVersion, Endian.little);
    _bytes.setUint16(6, bufferKindValue(BufferKind.commands), Endian.little);
    _bytes.setUint32(8, _count, Endian.little);
    _bytes.setUint32(12, total, Endian.little);
    _bytes.setUint32(16, recordsEnd, Endian.little);
    _bytes.setUint32(20, 0, Endian.little);
    return _bytes.buffer.asUint8List(0, total);
  }
}
