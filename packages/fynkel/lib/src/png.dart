import 'dart:typed_data';

/// [rgba] (RGBA8, rows top to bottom) of [width] × [height] as a PNG file:
/// uncompressed, which every reader takes, so it needs no compressor.
Uint8List encodePng(int width, int height, Uint8List rgba) {
  final row = width * 4;
  // Each row starts with its filter: 0, none.
  final raw = Uint8List(height * (row + 1));
  for (var y = 0; y < height; y++) {
    raw.setRange(y * (row + 1) + 1, (y + 1) * (row + 1), rgba, y * row);
  }
  final out = BytesBuilder(copy: false)..add(const [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
  final header = ByteData(13)
    ..setUint32(0, width)
    ..setUint32(4, height)
    ..setUint8(8, 8) // bits per channel
    ..setUint8(9, 6); // RGBA
  _chunk(out, 'IHDR', header.buffer.asUint8List());
  _chunk(out, 'IDAT', _zlibStored(raw));
  _chunk(out, 'IEND', Uint8List(0));
  return out.takeBytes();
}

void _chunk(BytesBuilder out, String type, Uint8List data) {
  final typed = Uint8List.fromList([...type.codeUnits, ...data]);
  out
    ..add((ByteData(4)..setUint32(0, data.length)).buffer.asUint8List())
    ..add(typed)
    ..add((ByteData(4)..setUint32(0, _crc32(typed))).buffer.asUint8List());
}

/// [data] as a zlib stream of stored (uncompressed) deflate blocks.
Uint8List _zlibStored(Uint8List data) {
  final out = BytesBuilder(copy: false)..add(const [0x78, 0x01]);
  var at = 0;
  do {
    final length = data.length - at < 0xFFFF ? data.length - at : 0xFFFF;
    final last = at + length == data.length;
    out
      ..add([last ? 1 : 0, length & 0xFF, length >> 8, ~length & 0xFF, (~length >> 8) & 0xFF])
      ..add(Uint8List.sublistView(data, at, at + length));
    at += length;
  } while (at < data.length);
  var a = 1, b = 0;
  for (final byte in data) {
    a = (a + byte) % 65521;
    b = (b + a) % 65521;
  }
  out.add((ByteData(4)..setUint32(0, (b << 16) | a)).buffer.asUint8List());
  return out.takeBytes();
}

final _crcTable = List<int>.generate(256, (n) {
  var c = n;
  for (var k = 0; k < 8; k++) {
    c = c & 1 != 0 ? 0xEDB88320 ^ (c >> 1) : c >> 1;
  }
  return c;
});

int _crc32(Uint8List bytes) {
  var c = 0xFFFFFFFF;
  for (final byte in bytes) {
    c = _crcTable[(c ^ byte) & 0xFF] ^ (c >> 8);
  }
  return c ^ 0xFFFFFFFF;
}
