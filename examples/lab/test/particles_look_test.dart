// 'Particles: the look': each of its five effects alone, close up, seeded,
// after two seconds, matches its golden (tests/golden/particles_<name>.png,
// shared with the Rust tests' rule: FYNKEL_BLESS=1 writes them).

import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:image/image.dart' as img;
import 'package:lab/scenes/particles_look.dart';

final _root = Directory.current.uri.resolve('../../');

/// As packages/fynkel/test/golden.dart: a channel may differ by 2/255, and
/// 0.1 % of the pixels by more.
void expectGolden(String name, Uint8List rgba, int width, int height) {
  final file = File.fromUri(_root.resolve('tests/golden/$name.png'));
  if (Platform.environment['FYNKEL_BLESS'] == '1') {
    _write(file.path, rgba, width, height);
    return;
  }
  final golden = img.decodePng(file.readAsBytesSync())!;
  expect((golden.width, golden.height), (width, height), reason: '$name: size differs from the golden');
  final expected = golden.convert(format: img.Format.uint8, numChannels: 4).getBytes(order: img.ChannelOrder.rgba);
  var off = 0, worst = 0;
  for (var i = 0; i < rgba.length; i += 4) {
    var d = 0;
    for (var c = 0; c < 4; c++) {
      final e = (rgba[i + c] - expected[i + c]).abs();
      if (e > d) d = e;
    }
    if (d > worst) worst = d;
    if (d > 2) off++;
  }
  if (off * 1000 > width * height) {
    final failures = Directory.fromUri(_root.resolve('target/golden-failures/'))..createSync(recursive: true);
    _write('${failures.path}lab_$name.actual.png', rgba, width, height);
    fail('$name: $off of ${width * height} pixels differ by more than 2/255 (worst $worst); see ${failures.path}');
  }
}

void _write(String path, Uint8List rgba, int width, int height) {
  final image = img.Image.fromBytes(width: width, height: height, bytes: rgba.buffer, numChannels: 4);
  File(path).writeAsBytesSync(img.encodePng(image));
}

void main() {
  for (final name in particleEffects) {
    test('Particles: the look: the $name matches its golden', () {
      final world = World()..quality = Quality.high;
      addTearDown(world.dispose);
      final scene = world.add(ParticlesLookScene(only: name));
      for (var i = 0; i < 120; i++) {
        world.update(1 / 60);
      }
      expect(scene.systems.keys, [name]);
      expect(scene.systems[name]!.every((s) => s.particleCount > 0), isTrue);
      final view = HeadlessView(world, width: 200, height: 200);
      addTearDown(view.dispose);
      expectGolden('particles_$name', view.read(view.frame()).rgba, 200, 200);
    });
  }
}
