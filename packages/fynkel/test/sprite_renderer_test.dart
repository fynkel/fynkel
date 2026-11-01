// Pictures in the scene (stage 15 phase 3): a SpriteRenderer draws a
// texture on a quad, as Unity's Sprite Renderer: its colour multiplied in,
// by its texture's size unless given one, flipped, 9-sliced, drawn by its
// sorting layer and order before by distance, cut out by alpha.

import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

/// [width] × [height] pixels, each [rgba].
Texture2D plain(int width, int height, List<int> rgba) =>
    Texture2D.rgba(width, height, Uint8List.fromList([for (var i = 0; i < width * height; i++) ...rgba]));

void main() {
  late World world;
  late HeadlessView view;
  setUp(() {
    world = World();
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 5)),
        components: [Camera(clearColor: const Color.rgb(0, 0, 0))],
      ),
    );
    view = HeadlessView(world, width: 64, height: 64);
  });
  tearDown(() {
    view.dispose();
    world.dispose();
  });

  /// The middle pixel's red, green and blue.
  (int, int, int) middle() {
    final image = view.read(view.frame());
    final i = (32 * 64 + 32) * 4;
    return (image.rgba[i], image.rgba[i + 1], image.rgba[i + 2]);
  }

  test('a sprite is its texture times its colour, its size its pixels at 100 a metre unless given', () {
    final white = plain(200, 100, [255, 255, 255, 255]);
    final sprite = SpriteRenderer(white, color: const Color.rgb(1, 0, 0));
    expect(sprite.size, Vec2(2, 1));
    world.add(GameObject(components: [sprite]));
    final (r, g, b) = middle();
    expect(r, greaterThan(200));
    expect(g + b, lessThan(r ~/ 2), reason: 'red, graded as the camera does');
    sprite.color = const Color.rgb(0, 0, 1);
    expect(middle().$3, greaterThan(200));
    sprite.size = Vec2(0.1, 0.1);
    sprite.pivot = Vec2(0, 0);
    expect(middle(), (0, 0, 0), reason: 'small, beside the middle now');
    sprite.enabled = false;
    sprite
      ..size = Vec2(3, 3)
      ..pivot = Vec2(0.5, 0.5);
    expect(middle(), (0, 0, 0), reason: 'off');
    sprite.enabled = true;
    expect(middle().$3, greaterThan(200), reason: 'on again, as it was set while off');
  });

  test('the higher sorting layer, then order, is drawn over, however far', () {
    final white = plain(2, 2, [255, 255, 255, 255]);
    final near = SpriteRenderer(white, size: Vec2(1, 1), color: const Color.rgb(1, 0, 0));
    final far = SpriteRenderer(white, size: Vec2(1, 1), color: const Color.rgb(0, 0, 1));
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 1)),
        components: [near],
      ),
    );
    world.add(GameObject(components: [far]));
    expect(middle().$1, greaterThan(200), reason: 'the nearer');
    far.sortingLayer = 1;
    expect(middle().$3, greaterThan(200), reason: 'a higher layer');
    far.sortingLayer = 0;
    far.order = 3;
    expect(middle().$3, greaterThan(200), reason: 'a higher order');
    near.order = 4;
    expect(middle().$1, greaterThan(200));
  });

  test('a cutoff cuts out what is below it', () {
    final faint = plain(2, 2, [255, 255, 255, 77]);
    final sprite = SpriteRenderer(faint, size: Vec2(3, 3));
    world.add(GameObject(components: [sprite]));
    expect(middle().$1, inInclusiveRange(40, 200), reason: 'see-through');
    sprite.alphaCutoff = 0.5;
    expect(middle().$1, lessThan(5), reason: 'cut out');
  });

  test('a sliced sprite, flipped and facing, and settings that can\'t be are refused', () {
    final white = plain(30, 30, [255, 255, 255, 255]);
    final sprite = SpriteRenderer(
      white,
      size: Vec2(3, 1),
      slice: const Insets.all(10),
      flipX: true,
      flipY: true,
      facing: Facing.camera,
    );
    world.add(GameObject(components: [sprite]));
    expect(middle().$1, greaterThan(200));
    sprite.lit = true;
    expect(middle().$1, lessThan(20), reason: 'lit, in a world with no light');
    expect(sprite.slice, const Insets.all(10));
    expect(() => sprite.alphaCutoff = 2, throwsArgumentError);
    expect(sprite.alphaCutoff, 0);
    expect(() => sprite.size = Vec2(-1, 1), throwsArgumentError);
    expect(() => sprite.slice = const Insets(-1, 0, 0, 0), throwsArgumentError);
    expect(() => SpriteRenderer(white, pixelsPerMetre: 0), throwsArgumentError);
    // The texture can't go while the sprite shows it.
    expect(white.dispose, throwsStateError);
    sprite.gameObject.removeComponent(sprite);
    white.dispose();
  });
}
