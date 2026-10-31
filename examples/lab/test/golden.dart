// The lab's golden check, as packages/fynkel/test/golden.dart: the images
// are in tests/golden/, and FYNKEL_BLESS=1 writes them.

import 'dart:io';
import 'dart:typed_data';

import 'package:fynkel/testing.dart';

final _root = Directory.current.uri.resolve('../../');

void expectGolden(String name, Uint8List rgba, int width, int height) => expectImage(
  _root.resolve('tests/golden/$name.png').toFilePath(),
  rgba,
  width: width,
  height: height,
  failures: _root.resolve('target/golden-failures/lab').toFilePath(),
);
