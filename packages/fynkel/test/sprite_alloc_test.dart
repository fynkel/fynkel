// A frame that fades 200 sprites, as a game fades markers and hit flashes,
// allocates nothing in Dart: the settings go through scratch memory. The
// colours are made before the frames are counted, as a game's own are its
// own. Counted by the VM service's allocation profile (see
// allocations.dart).

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';

void main() {
  final madeEachFrame = allocations();

  test('a frame that fades 200 sprites allocates nothing', () async {
    final world = World();
    addTearDown(world.dispose);
    world.add(Markers(Texture2D.rgba(8, 8, Uint8List(8 * 8 * 4)..fillRange(0, 256, 255))));
    final made = await madeEachFrame(world, 2000);
    expect(made, isEmpty);
  });
}

/// 200 sprites, each faded every frame; half sized by their texture.
class Markers extends GameObject {
  Markers(this.texture);

  final Texture2D texture;
  final sprites = <SpriteRenderer>[];
  static final _fades = [for (var i = 0; i < 64; i++) Color.white.withAlpha(i / 63)];
  var _frame = 0;

  @override
  void onLoad() {
    super.onLoad();
    for (var i = 0; i < 200; i++) {
      final sprite = SpriteRenderer(
        texture,
        size: i.isEven ? Vec2(0.3, 0.3) : null,
        slice: i % 4 == 1 ? const Insets.all(2) : Insets.zero,
        facing: Facing.camera,
      );
      sprites.add(sprite);
      add(
        GameObject(
          transform: Transform(position: Vec3(i % 20 - 10, i ~/ 20 - 5, 0)),
          components: [sprite],
        ),
      );
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _frame++;
    for (var i = 0; i < sprites.length; i++) {
      sprites[i].color = _fades[(_frame + i) % _fades.length];
    }
  }
}
