// Hand-written support for the generated wire codecs
// (lib/generated/schema.g.dart). Layout rules: fynkel_contracts::schema.

import 'dart:convert';
import 'dart:typed_data';

import '../../math.dart';

/// Collects strings while encoding; the bytes form the buffer's string tail.
/// Every generated record: fixed-size body written at [encode]'s offset,
/// strings in the tail. Lets [CommandBatch] encode without a per-command
/// closure (tear-off) allocation.
abstract interface class WireRecord {
  void encode(ByteData d, int offset, WireStrings strings);
}

class WireStrings {
  final BytesBuilder _b = BytesBuilder(copy: false);

  int get length => _b.length;

  /// Appends [s] as UTF-8 and returns its offset in the tail.
  int put(String s, List<int> out) {
    final bytes = utf8.encode(s);
    final off = _b.length;
    _b.add(bytes);
    out[0] = off;
    out[1] = bytes.length;
    return off;
  }

  Uint8List takeBytes() => _b.takeBytes();

  void clear() => _b.clear();
}

final List<int> _pair = List.filled(2, 0);

void writeWireString(ByteData d, int o, String s, WireStrings strings) {
  strings.put(s, _pair);
  d.setUint32(o, _pair[0], Endian.little);
  d.setUint32(o + 4, _pair[1], Endian.little);
}

String readWireString(ByteData d, int o, Uint8List tail) {
  final off = d.getUint32(o, Endian.little);
  final len = d.getUint32(o + 4, Endian.little);
  if (off + len > tail.length) {
    throw FormatException('wire string ($off, $len) outside tail of ${tail.length} bytes');
  }
  return utf8.decode(Uint8List.sublistView(tail, off, off + len));
}

void writeVec3(ByteData d, int o, Vec3 v) {
  d.setFloat32(o, v.x, Endian.little);
  d.setFloat32(o + 4, v.y, Endian.little);
  d.setFloat32(o + 8, v.z, Endian.little);
}

Vec3 readVec3(ByteData d, int o) => Vec3(
      d.getFloat32(o, Endian.little),
      d.getFloat32(o + 4, Endian.little),
      d.getFloat32(o + 8, Endian.little),
    );

void writeQuat(ByteData d, int o, Quat q) {
  d.setFloat32(o, q.x, Endian.little);
  d.setFloat32(o + 4, q.y, Endian.little);
  d.setFloat32(o + 8, q.z, Endian.little);
  d.setFloat32(o + 12, q.w, Endian.little);
}

Quat readQuat(ByteData d, int o) => Quat(
      d.getFloat32(o, Endian.little),
      d.getFloat32(o + 4, Endian.little),
      d.getFloat32(o + 8, Endian.little),
      d.getFloat32(o + 12, Endian.little),
    );

/// `0` on the wire means "no entity".
int? optionalEntity(int bits) => bits == 0 ? null : bits;
