// Hand-written support for the generated wire codecs
// (lib/generated/schema.g.dart). Layout rules: fynkel_contracts::schema.

import 'dart:convert';
import 'dart:typed_data';
import '../entity_ref.dart';
export '../entity_ref.dart' show writeEntity, readEntity;

import '../../math.dart';
import '../asset_ref.dart';

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

void writeVec4(ByteData d, int o, Vec4 v) {
  d.setFloat32(o, v.x, Endian.little);
  d.setFloat32(o + 4, v.y, Endian.little);
  d.setFloat32(o + 8, v.z, Endian.little);
  d.setFloat32(o + 12, v.w, Endian.little);
}

Vec4 readVec4(ByteData d, int o) => Vec4(
      d.getFloat32(o, Endian.little),
      d.getFloat32(o + 4, Endian.little),
      d.getFloat32(o + 8, Endian.little),
      d.getFloat32(o + 12, Endian.little),
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
EntityRef? optionalEntity(EntityRef entity) => entity.isNull ? null : entity;

/// `AssetRef` on the wire: 16 id bytes, then `sub` as a wire string (empty =
/// none). Throws [FormatException] when the id text is not canonical.
void writeAssetRef(ByteData d, int o, AssetRef v, WireStrings strings) {
  final id = v.bytes;
  if (id == null) throw FormatException('AssetRef.asset is not a canonical UUID: "${v.asset}"');
  for (var i = 0; i < 16; i++) {
    d.setUint8(o + i, id[i]);
  }
  writeWireString(d, o + 16, v.sub ?? '', strings);
}

AssetRef readAssetRef(ByteData d, int o, Uint8List tail) {
  final id = Uint8List(16);
  for (var i = 0; i < 16; i++) {
    id[i] = d.getUint8(o + i);
  }
  return AssetRef.fromBytes(id, readWireString(d, o + 16, tail));
}

/// `AssetRef?` on the wire: a nil id (all zero bytes) with an empty `sub`
/// means none.
void writeOptionalAssetRef(ByteData d, int o, AssetRef? v, WireStrings strings) {
  if (v == null) {
    for (var i = 0; i < 16; i++) {
      d.setUint8(o + i, 0);
    }
    writeWireString(d, o + 16, '', strings);
    return;
  }
  writeAssetRef(d, o, v, strings);
}

AssetRef? readOptionalAssetRef(ByteData d, int o, Uint8List tail) {
  final r = readAssetRef(d, o, tail);
  return r.isNone ? null : r;
}

/// Enum fields carry the declaration index; an index this SDK does not know
/// (a newer native library) is a [FormatException], never a silent default.
T enumValue<T extends Enum>(List<T> values, int index, String name) {
  if (index < 0 || index >= values.length) {
    throw FormatException('$name: unknown variant index $index (this SDK knows ${values.length})');
  }
  return values[index];
}

// ---- document JSON (the generated `toJson`, `fynkel_document`'s codec)

final _f32 = Float32List(1);

/// [v] as the `f32` the engine keeps, in the shortest decimal that reads
/// back as that `f32` — the lexeme the Rust codec writes (`0.05`, not
/// `0.05000000074505806`).
double docFloat(double v) {
  if (!v.isFinite) return v;
  _f32[0] = v;
  final f = _f32[0];
  for (var p = 1; p < 10; p++) {
    final d = double.parse(f.toStringAsPrecision(p));
    _f32[0] = d;
    if (_f32[0] == f) return d;
  }
  return f;
}

List<double> docVec3(Vec3 v) => [docFloat(v.x), docFloat(v.y), docFloat(v.z)];
List<double> docVec4(Vec4 v) => [docFloat(v.x), docFloat(v.y), docFloat(v.z), docFloat(v.w)];
List<double> docQuat(Quat q) => [docFloat(q.x), docFloat(q.y), docFloat(q.z), docFloat(q.w)];

/// `{ "asset": id, "sub"? }`.
Map<String, Object?> docAssetRef(AssetRef r) => {'asset': r.asset, if ((r.sub ?? '').isNotEmpty) 'sub': r.sub};
