import 'dart:io' show zlib;
import 'dart:typed_data';

/// [rgba] (RGBA8, rows top to bottom) of [width] × [height] as a PNG file:
/// uncompressed, which every reader takes, unless [compress] (for a file
/// kept, as a test's saved picture).
Uint8List encodePng(int width, int height, Uint8List rgba, {bool compress = false}) {
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
  _chunk(out, 'IDAT', compress ? Uint8List.fromList(zlib.encode(raw)) : _zlibStored(raw));
  _chunk(out, 'IEND', Uint8List(0));
  return out.takeBytes();
}

/// A PNG file's picture as RGBA8, rows top to bottom: 8-bit RGBA or RGB,
/// not interlaced, as Fynkel and most tools save. Throws a
/// [FormatException] for anything else.
({int width, int height, Uint8List rgba}) decodePng(Uint8List file) {
  const signature = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
  for (var i = 0; i < 8; i++) {
    if (i >= file.length || file[i] != signature[i]) throw const FormatException('not a PNG file');
  }
  final data = ByteData.sublistView(file);
  final idat = BytesBuilder(copy: false);
  var width = 0, height = 0, channels = 0;
  for (var at = 8; at + 8 <= file.length;) {
    final length = data.getUint32(at);
    final type = String.fromCharCodes(file, at + 4, at + 8);
    final body = Uint8List.sublistView(file, at + 8, at + 8 + length);
    at += 12 + length;
    if (type == 'IHDR') {
      width = data.getUint32(16);
      height = data.getUint32(20);
      final (depth, colour, interlace) = (body[8], body[9], body[12]);
      channels = switch (colour) {
        6 => 4,
        2 => 3,
        _ => 0,
      };
      if (depth != 8 || channels == 0 || interlace != 0) {
        throw FormatException(
          'a PNG of $depth-bit colour type $colour${interlace != 0 ? ', interlaced' : ''}: '
          'only 8-bit RGBA or RGB, not interlaced, is read',
        );
      }
    } else if (type == 'IDAT') {
      idat.add(body);
    } else if (type == 'IEND') {
      break;
    }
  }
  if (channels == 0) throw const FormatException('a PNG with no header');
  final raw = zlib.decode(idat.takeBytes());
  final row = width * channels;
  if (raw.length < height * (row + 1)) throw const FormatException('a PNG cut short');
  // Undo each row's filter (PNG's §9) in place, against the row above.
  final pixels = Uint8List(height * row);
  for (var y = 0; y < height; y++) {
    final filter = raw[y * (row + 1)];
    final from = y * (row + 1) + 1, to = y * row;
    for (var x = 0; x < row; x++) {
      final a = x >= channels ? pixels[to + x - channels] : 0;
      final b = y > 0 ? pixels[to - row + x] : 0;
      final c = x >= channels && y > 0 ? pixels[to - row + x - channels] : 0;
      final value = raw[from + x];
      pixels[to + x] =
          switch (filter) {
            0 => value,
            1 => value + a,
            2 => value + b,
            3 => value + ((a + b) >> 1),
            4 => value + _paeth(a, b, c),
            _ => throw FormatException('a PNG row with filter $filter'),
          } &
          0xFF;
    }
  }
  if (channels == 4) return (width: width, height: height, rgba: pixels);
  final rgba = Uint8List(width * height * 4);
  for (var i = 0, o = 0; i < pixels.length; i += 3, o += 4) {
    rgba
      ..[o] = pixels[i]
      ..[o + 1] = pixels[i + 1]
      ..[o + 2] = pixels[i + 2]
      ..[o + 3] = 255;
  }
  return (width: width, height: height, rgba: rgba);
}

int _paeth(int a, int b, int c) {
  final p = a + b - c;
  final pa = (p - a).abs(), pb = (p - b).abs(), pc = (p - c).abs();
  return pa <= pb && pa <= pc ? a : (pb <= pc ? b : c);
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
