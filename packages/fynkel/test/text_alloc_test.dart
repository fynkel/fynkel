// A frame that changes 200 numbers in the scene, as a game's hit numbers
// and scores do, allocates nothing in Dart (stage 15 phase 2's bar): the
// text goes to the engine as UTF-8 through scratch memory. The strings are
// made before the frames are counted, as a game's own formatting is its
// own. Counted by the VM service's allocation profile (see
// allocations.dart).

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';

final _roboto = File.fromUri(Directory.current.uri.resolve('../../tests/assets/fonts/Roboto-Regular.ttf'));

void main() {
  final madeEachFrame = allocations();

  test('a frame that changes 200 numbers allocates nothing', () async {
    final world = World();
    addTearDown(world.dispose);
    world.add(Numbers(Font(_roboto.readAsBytesSync())));
    final made = await madeEachFrame(world, 2000);
    expect(made, isEmpty);
  });
}

/// 200 numbers, each changed every frame.
class Numbers extends GameObject {
  Numbers(this.font);

  final Font font;
  final texts = <TextRenderer>[];
  static final _numbers = [for (var i = 0; i < 1000; i++) i.isEven ? '+$i' : '-${i * 7} x2'];
  var _frame = 0;

  @override
  void onLoad() {
    super.onLoad();
    for (var i = 0; i < 200; i++) {
      final text = TextRenderer('0', font: font, size: 0.3, facing: Facing.camera, onTop: true);
      texts.add(text);
      add(
        GameObject(
          transform: Transform(position: Vec3(i % 20 - 10, i ~/ 20 - 5, 0)),
          components: [text],
        ),
      );
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _frame++;
    for (var i = 0; i < texts.length; i++) {
      texts[i].text = _numbers[(_frame * 13 + i) % _numbers.length];
    }
  }
}
