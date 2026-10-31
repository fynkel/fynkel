// The repository's golden images, tests/golden/*.png, shared with the Rust
// tests (crates/fynkel_golden, the same rule): package:fynkel/testing.dart's
// expectImage. FYNKEL_BLESS=1 writes them instead.

import 'dart:io';
import 'dart:typed_data';

import 'package:fynkel/testing.dart';

final _root = Directory.current.uri.resolve('../../');

void expectGolden(String name, Uint8List rgba, int width, int height) => expectImage(
  _root.resolve('tests/golden/$name.png').toFilePath(),
  rgba,
  width: width,
  height: height,
  failures: _root.resolve('target/golden-failures/dart').toFilePath(),
);
