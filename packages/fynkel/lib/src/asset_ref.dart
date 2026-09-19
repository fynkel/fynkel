/// Reference to an asset or one of its sub-assets (ADR-0006, blueprint
/// §19.2c). `asset` is the canonical lower-case hyphenated UUID from the
/// asset's `.meta.json`; `sub` names a sub-asset (`mesh/0`) or is null for
/// the asset itself. Never a path. Immutable value (ADR-0013).
library;

import 'dart:typed_data';

class AssetRef {
  const AssetRef(this.asset, {this.sub});

  /// The unset reference: all-zero id, no sub-asset.
  static const AssetRef none = AssetRef('00000000-0000-0000-0000-000000000000');

  /// Canonical UUID text; [isValid] tells whether it is well-formed.
  final String asset;
  final String? sub;

  bool get isNone => asset == none.asset;

  /// True when [asset] is 8-4-4-4-12 lower-case hex.
  bool get isValid => _parse(asset) != null;

  /// Big-endian bytes of [asset], or null when it is malformed.
  Uint8List? get bytes => _parse(asset);

  /// [AssetRef] from 16 id bytes; [sub] empty means none.
  factory AssetRef.fromBytes(Uint8List id, String sub) {
    if (id.length != 16) throw ArgumentError.value(id, 'id', 'expected 16 bytes');
    final b = StringBuffer();
    for (var i = 0; i < 16; i++) {
      if (i == 4 || i == 6 || i == 8 || i == 10) b.write('-');
      b.write(id[i].toRadixString(16).padLeft(2, '0'));
    }
    return AssetRef(b.toString(), sub: sub.isEmpty ? null : sub);
  }

  static Uint8List? _parse(String s) {
    if (s.length != 36) return null;
    final out = Uint8List(16);
    var i = 0;
    var pos = 0;
    while (pos < 36) {
      if (pos == 8 || pos == 13 || pos == 18 || pos == 23) {
        if (s.codeUnitAt(pos) != 0x2d) return null;
        pos++;
        continue;
      }
      final hi = _nibble(s.codeUnitAt(pos));
      final lo = _nibble(s.codeUnitAt(pos + 1));
      if (hi < 0 || lo < 0) return null;
      out[i++] = (hi << 4) | lo;
      pos += 2;
    }
    return out;
  }

  static int _nibble(int c) {
    if (c >= 0x30 && c <= 0x39) return c - 0x30;
    if (c >= 0x61 && c <= 0x66) return c - 0x61 + 10;
    return -1;
  }

  @override
  bool operator ==(Object other) => other is AssetRef && other.asset == asset && other.sub == sub;
  @override
  int get hashCode => Object.hash(asset, sub);
  @override
  String toString() => sub == null ? 'AssetRef($asset)' : 'AssetRef($asset#$sub)';
}
