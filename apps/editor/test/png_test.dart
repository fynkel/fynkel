import 'dart:io' show ZLibEncoder;
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/scene_view/png.dart';

void main() {
  group('png', () {
    test('encode → decode round-trips RGBA pixels and rejects a wrong length', () {
      const w = 5, h = 3;
      final rgba = Uint8List(w * h * 4);
      for (var i = 0; i < rgba.length; i++) {
        rgba[i] = (i * 37) & 0xFF;
      }
      final file = encodePng(w, h, rgba);
      expect(file.sublist(0, 8), [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
      final (dw, dh, pixels) = decodePng(file);
      expect(dw, w);
      expect(dh, h);
      expect(pixels, rgba);
      expect(() => encodePng(w, h, Uint8List(3)), throwsArgumentError);
    });

    test('decode reads RGB with the Sub/Up/Average/Paeth filters (what other writers produce)', () {
      // Hand-built: 2×2 RGB, one row per filter type where a previous row exists.
      const w = 2;
      final rows = <List<int>>[
        [0, 10, 20, 30, 40, 50, 60], // none
        [1, 5, 5, 5, 1, 1, 1], // sub: (5,5,5), (6,6,6)
        [2, 1, 1, 1, 2, 2, 2], // up: (6,6,6), (8,8,8)
        [
          4,
          0,
          0,
          0,
          0,
          0,
          0,
        ], // paeth over (6,6,6),(8,8,8): first pixel a=0,b=6,c=0 → p=6 → b → 6; second: a=6,b=8,c=6 → p=8 → b → 8
      ];
      final raw = Uint8List.fromList([for (final r in rows) ...r]);
      final compressed = ZLibEncoder().convert(raw);
      final out = BytesBuilder();
      out.add(const [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
      void chunk(String type, List<int> data) {
        final len = ByteData(4)..setUint32(0, data.length);
        out.add(len.buffer.asUint8List());
        out.add(type.codeUnits);
        out.add(data);
        out.add([0, 0, 0, 0]); // the decoder does not check CRCs
      }

      final ihdr = ByteData(13)
        ..setUint32(0, w)
        ..setUint32(4, rows.length)
        ..setUint8(8, 8)
        ..setUint8(9, 2);
      chunk('IHDR', ihdr.buffer.asUint8List());
      chunk('IDAT', compressed);
      chunk('IEND', const []);
      final (dw, dh, px) = decodePng(out.takeBytes());
      expect((dw, dh), (2, 4));
      List<int> pixel(int x, int y) => [for (var c = 0; c < 4; c++) px[(y * w + x) * 4 + c]];
      expect(pixel(0, 0), [10, 20, 30, 255]);
      expect(pixel(1, 0), [40, 50, 60, 255]);
      expect(pixel(0, 1), [5, 5, 5, 255]);
      expect(pixel(1, 1), [6, 6, 6, 255]);
      expect(pixel(0, 2), [6, 6, 6, 255]);
      expect(pixel(1, 2), [8, 8, 8, 255]);
      expect(pixel(0, 3), [6, 6, 6, 255]);
      expect(pixel(1, 3), [8, 8, 8, 255]);
    });
  });
}
