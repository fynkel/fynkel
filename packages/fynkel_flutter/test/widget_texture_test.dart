// A widget drawn once into a texture (stage 15 phase 3): a sign or a
// poster with the game's own look, shown by a SpriteRenderer or a
// material. Checked as a game sees it: on a sprite, drawn.

import 'package:flutter/material.dart' hide Color, Transform;
import 'package:flutter/material.dart' as flutter show Color;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() {
  /// [texture] on a sprite filling a 64 × 32 view, drawn: its pixels.
  List<int> drawn(Texture2D texture) {
    final world = World();
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 5)),
        components: [Camera(clearColor: const Color.rgb(0, 0, 0))],
      ),
    );
    // 60° high at 5 m: 5.77 m tall; twice as wide.
    world.add(GameObject(components: [SpriteRenderer(texture, size: Vec2(11.6, 5.8))]));
    final view = HeadlessView(world, width: 64, height: 32);
    try {
      return view.read(view.frame()).rgba;
    } finally {
      view.dispose();
      world.dispose();
    }
  }

  testWidgets('a widget is drawn into a texture its size times the pixel ratio, alpha unmultiplied', (tester) async {
    final texture = (await tester.runAsync(
      () => Row(
        children: [
          Expanded(child: Container(color: const flutter.Color(0xFFFF0000))),
          Expanded(child: Container(color: const flutter.Color(0x800000FF))),
        ],
      ).toTexture(size: const Size(40, 20), pixelRatio: 2),
    ))!;
    expect((texture.width, texture.height), (80, 40));
    final pixels = drawn(texture);
    List<int> at(int x, int y) => pixels.sublist((y * 64 + x) * 4, (y * 64 + x) * 4 + 3);
    final red = at(16, 16);
    expect(red[0], greaterThan(200), reason: 'the red half: $red');
    final blue = at(48, 16);
    // Half see-through full blue over black: about half bright, not a
    // quarter (premultiplied twice).
    expect(blue[2], inInclusiveRange(110, 210), reason: '$blue');
    expect(blue[0] + blue[1], lessThan(blue[2] ~/ 3));
  });

  testWidgets('text in it, with no app round it, is drawn', (tester) async {
    final texture = (await tester.runAsync(
      () => const ColoredBox(
        color: flutter.Color(0xFF000000),
        child: Center(
          child: Text('HELLO', style: TextStyle(fontSize: 14, color: flutter.Color(0xFFFFFFFF))),
        ),
      ).toTexture(size: const Size(64, 32)),
    ))!;
    final pixels = drawn(texture);
    var white = 0;
    for (var i = 0; i < pixels.length; i += 4) {
      if (pixels[i] > 150) white++;
    }
    expect(white, greaterThan(30));
  });
}
