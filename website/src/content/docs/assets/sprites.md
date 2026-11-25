---
title: "Sprites"
description: "Pictures in the scene, 9-slices and sorting."
---

A **`SpriteRenderer`** draws a picture on a quad: a ring on the ground where
to go, a leaf, a cloud, a speech bubble, a poster. Its size is its texture's
pixels at `pixelsPerMetre` (100, as Unity's) unless it's given one, and its
`pivot` is the point of it that sits on its object's origin ((0.5, 0.5) its
middle, (0.5, 0) its feet).

`color` is multiplied into the texture, so its alpha fades it; `flipX` and
`flipY` mirror it about the pivot; `facing` turns it as a text's does:

```dart
/// Where to go: a ring on the ground, pulsing.
class TargetRing extends GameObject {
  TargetRing() : super(name: 'target ring');

  SpriteRenderer? _ring;

  @override
  Future<void> onLoad() async {
    super.onLoad();
    final ring = SpriteRenderer(
      await world.assets.texture('ring.png'),
      size: Vec2(1.5, 1.5),
      color: const Color(0xFFFFD040),
      facing: Facing.up,
    );
    addComponent(ring);
    _ring = ring;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _ring?.color = Color.rgb(1, 0.82, 0.25, 0.4 + 0.6 * ((time.sinceStart * 1.5) % 1));
  }
}

/// A speech bubble standing over its object: one small rounded picture,
/// 9-sliced to fit the words, the words in front of it.
GameObject speechBubble(String words, Font font, Texture2D rounded) {
  const size = 0.22;
  final fit = font.measure(words, size: size) + Vec2(0.4, 0.3);
  return GameObject(
    name: 'bubble',
    transform: Transform(position: Vec3(0, 2, 0)),
    components: [
      SpriteRenderer(rounded, size: fit, slice: const Insets.all(16), pivot: Vec2(0.5, 0), facing: Facing.camera),
    ],
    children: [
      // One line's letters stand on y = 0; what hangs below is about a
      // quarter of the size.
      GameObject(
        transform: Transform(position: Vec3(0, 0.15 + size * 0.25, 0.01)),
        components: [TextRenderer(words, font: font, size: size, color: Color.black, facing: Facing.camera)],
      ),
    ],
  );
}
```

- **9-slice**: `slice: Insets(left, top, right, bottom)` says how far in from
  each edge the corners are, in the texture's pixels (Unity's sprite borders).
  The corners keep their size, `pixelsPerMetre` to the metre, however big it's
  made, and the edges and the middle stretch: a bubble, a panel, a bar. A
  sprite smaller than its corners shrinks them to fit.
- **What's drawn over what**: see-through sprites are drawn by their
  `sortingLayer`, then their `order`, higher over lower, and only then by
  distance, farther first. Cards in a hand, a shadow under a character, a 2D
  game's background behind its middle: give them their layers and orders, and
  the distance doesn't matter. Text that's `onTop` stays over them all.
- **Cut out**: `alphaCutoff` above 0 cuts the sprite out where its alpha is
  below it, for leaves and fences that hide what's behind them as solid things
  do. At 0 it's see-through by its alpha.
- **Many**: sprites of one texture, sorting layer and order are drawn
  together, so a thousand cost one draw: 1,000 moving at once hold 60 fps on a
  Note10+ at low and on a 2018 iPad. Put a game's small pictures in one
  texture (an atlas) to keep them together. A texture can't be disposed while
  a sprite shows it.

The lab's "Text in the world" and "Pictures in the world" show all of
this ([examples/lab/lib/scenes/flat_text.dart](https://github.com/fynkel/fynkel/blob/main/examples/lab/lib/scenes/flat_text.dart) and sprites.dart).
