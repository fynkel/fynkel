// Golden-image comparison, the same rule as crates/fynkel_golden: a channel
// may differ by at most 2/255, and at most 0.1 % of pixels may be outside
// that. The references are the repository's tests/golden/*.png, shared
// with the Rust tests.

import 'dart:io';
import 'dart:typed_data';

import 'package:image/image.dart' as img;
import 'package:test/test.dart';

final _root = Directory.current.uri.resolve('../../');

void expectGolden(String name, Uint8List rgba, int width, int height) {
  final file = File.fromUri(_root.resolve('tests/golden/$name.png'));
  final golden = img.decodePng(file.readAsBytesSync());
  expect(golden, isNotNull, reason: '${file.path} is not a PNG');
  expect((golden!.width, golden.height), (width, height), reason: '$name: size differs from the golden');
  final expected = golden.convert(format: img.Format.uint8, numChannels: 4).getBytes(order: img.ChannelOrder.rgba);

  var off = 0;
  var worst = 0;
  final diff = Uint8List(rgba.length);
  for (var i = 0; i < rgba.length; i += 4) {
    var d = 0;
    for (var c = 0; c < 4; c++) {
      final e = (rgba[i + c] - expected[i + c]).abs();
      if (e > d) d = e;
    }
    if (d > worst) worst = d;
    if (d > 2) off++;
    diff
      ..[i] = (d * 16).clamp(0, 255)
      ..[i + 3] = 255;
  }
  if (off * 1000 > width * height) {
    final failures = Directory.fromUri(_root.resolve('target/golden-failures/'))..createSync(recursive: true);
    _write('${failures.path}dart_$name.actual.png', rgba, width, height);
    _write('${failures.path}dart_$name.diff.png', diff, width, height);
    fail('$name: $off of ${width * height} pixels differ by more than 2/255 (worst $worst); see ${failures.path}');
  }
}

void _write(String path, Uint8List rgba, int width, int height) {
  final image = img.Image.fromBytes(width: width, height: height, bytes: rgba.buffer, numChannels: 4);
  File(path).writeAsBytesSync(img.encodePng(image));
}
