// Buffer framing for ABI v5 (crates/fynkel_native/include/fynkel.h):
// 24-byte header, record area, string tail. Record bodies use the
// generated codecs (lib/generated/schema.g.dart).

import 'dart:typed_data';

import '../entity_ref.dart';

import '../../generated/schema.g.dart';
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

/// Reusable batch of fixed-size records for input and query transport.
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
