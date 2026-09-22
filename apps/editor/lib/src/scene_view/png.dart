// A PNG writer for the Scene view's "Save Frame": RGBA8 pixels in, the
// file's bytes out. Eight-bit RGBA, one IDAT, zlib through `dart:io`
// (deflate is what `fynkel render` writes too, so the same pixels give
// a file any image tool opens). No image package: the editor writes one
// kind of file.
import 'dart:io';
import 'dart:typed_data';

/// The PNG of [rgba] (`width × height × 4` bytes, top row first).
Uint8List encodePng(int width, int height, Uint8List rgba) {
  if (rgba.length != width * height * 4) {
    throw ArgumentError('rgba has ${rgba.length} bytes, not ${width * height * 4} for $width×$height');
  }
  // Filter byte 0 (none) before every row.
  final raw = Uint8List(height * (width * 4 + 1));
  for (var y = 0; y < height; y++) {
    final src = y * width * 4;
    final dst = y * (width * 4 + 1);
    raw[dst] = 0;
    raw.setRange(dst + 1, dst + 1 + width * 4, rgba, src);
  }
  final compressed = ZLibEncoder(level: 6).convert(raw);
  final out = BytesBuilder(copy: false);
  out.add(const [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
  final ihdr = ByteData(13)
    ..setUint32(0, width)
    ..setUint32(4, height)
    ..setUint8(8, 8) // bit depth
    ..setUint8(9, 6) // colour type: RGBA
    ..setUint8(10, 0) // compression
    ..setUint8(11, 0) // filter
    ..setUint8(12, 0); // interlace
  _chunk(out, 'IHDR', ihdr.buffer.asUint8List());
  _chunk(out, 'IDAT', Uint8List.fromList(compressed));
  _chunk(out, 'IEND', Uint8List(0));
  return out.takeBytes();
}

void _chunk(BytesBuilder out, String type, Uint8List data) {
  final len = ByteData(4)..setUint32(0, data.length);
  out.add(len.buffer.asUint8List());
  final typeBytes = Uint8List.fromList(type.codeUnits);
  out.add(typeBytes);
  out.add(data);
  var crc = _crc32(typeBytes, 0xFFFFFFFF);
  crc = _crc32(data, crc);
  final crcBytes = ByteData(4)..setUint32(0, crc ^ 0xFFFFFFFF);
  out.add(crcBytes.buffer.asUint8List());
}

final _crcTable = () {
  final t = Uint32List(256);
  for (var n = 0; n < 256; n++) {
    var c = n;
    for (var k = 0; k < 8; k++) {
      c = (c & 1) != 0 ? 0xEDB88320 ^ (c >> 1) : c >> 1;
    }
    t[n] = c;
  }
  return t;
}();

int _crc32(Uint8List bytes, int crc) {
  var c = crc;
  for (final b in bytes) {
    c = _crcTable[(c ^ b) & 0xFF] ^ (c >> 8);
  }
  return c;
}

/// The reverse, for the tests and the pixel gate: `(width, height, rgba)`
/// of an 8-bit RGB or RGBA PNG without interlace (what [encodePng] and
/// `fynkel render` write).
(int, int, Uint8List) decodePng(Uint8List file) {
  final data = ByteData.sublistView(file);
  var pos = 8;
  int width = 0, height = 0, colorType = 0;
  final idat = BytesBuilder(copy: false);
  while (pos + 8 <= file.length) {
    final len = data.getUint32(pos);
    final type = String.fromCharCodes(file, pos + 4, pos + 8);
    final body = Uint8List.sublistView(file, pos + 8, pos + 8 + len);
    if (type == 'IHDR') {
      final h = ByteData.sublistView(body);
      width = h.getUint32(0);
      height = h.getUint32(4);
      if (h.getUint8(8) != 8 || h.getUint8(12) != 0) throw const FormatException('not an 8-bit non-interlaced PNG');
      colorType = h.getUint8(9);
      if (colorType != 2 && colorType != 6) throw const FormatException('not an RGB/RGBA PNG');
    } else if (type == 'IDAT') {
      idat.add(body);
    } else if (type == 'IEND') {
      break;
    }
    pos += 12 + len;
  }
  final channels = colorType == 6 ? 4 : 3;
  final raw = Uint8List.fromList(ZLibDecoder().convert(idat.takeBytes()));
  final stride = width * channels;
  final out = Uint8List(width * height * 4);
  final prev = Uint8List(stride);
  final cur = Uint8List(stride);
  for (var y = 0; y < height; y++) {
    final filter = raw[y * (stride + 1)];
    cur.setRange(0, stride, raw, y * (stride + 1) + 1);
    for (var i = 0; i < stride; i++) {
      final a = i >= channels ? cur[i - channels] : 0;
      final b = prev[i];
      final c = i >= channels ? prev[i - channels] : 0;
      final add = switch (filter) {
        0 => 0,
        1 => a,
        2 => b,
        3 => (a + b) ~/ 2,
        4 => _paeth(a, b, c),
        _ => throw FormatException('unknown PNG filter $filter'),
      };
      cur[i] = (cur[i] + add) & 0xFF;
    }
    for (var x = 0; x < width; x++) {
      final s = x * channels, d = (y * width + x) * 4;
      out[d] = cur[s];
      out[d + 1] = cur[s + 1];
      out[d + 2] = cur[s + 2];
      out[d + 3] = channels == 4 ? cur[s + 3] : 255;
    }
    prev.setAll(0, cur);
  }
  return (width, height, out);
}

int _paeth(int a, int b, int c) {
  final p = a + b - c;
  final pa = (p - a).abs(), pb = (p - b).abs(), pc = (p - c).abs();
  if (pa <= pb && pa <= pc) return a;
  return pb <= pc ? b : c;
}
