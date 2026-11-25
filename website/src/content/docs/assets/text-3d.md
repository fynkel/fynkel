---
title: "3D text"
description: "Solid letters, lit and shadowed like any mesh."
---

`Mesh.text` makes text solid from a font: letters with depth that are
lit, cast shadows and have a collider like any mesh. Use it for titles,
signs, a score standing in the world, or letters that fall and bounce. A
font is a `.ttf` or `.otf`: load it as an asset, or with `Font(bytes)`.

```dart
/// A sign: its words in solid letters on a board that fits them.
Future<GameObject> sign(World world, String words, Material paint, Material wood) async {
  final font = await world.assets.font('fonts/Roboto-Regular.ttf');
  const size = 0.4;
  final board = font.measure(words, size: size) + Vec2(0.3, 0.2);
  // The first line stands on y = 0 and the rest go down: the board's
  // middle is a line's height (less the space under the letters) below.
  final middle = size * 0.75 - board.y / 2;
  return GameObject(
    transform: Transform(position: Vec3(0, 1.5, 0)),
    components: [MeshRenderer(Mesh.text(words, font: font, size: size, depth: 0.05), paint)],
    children: [
      GameObject(
        transform: Transform(position: Vec3(0, middle, -0.05)),
        components: [MeshRenderer(Mesh.box(size: Vec3(board.x, board.y, 0.05)), wood)],
      ),
    ],
  );
}

/// A score in the world: text that changes is a new mesh each time.
void showScore(MeshRenderer score, Font font, int points) {
  score.mesh = Mesh.text('$points', font: font, size: 0.6, depth: 0.15);
}
```

### Layout

Layout: `size` is the font's em in metres; capitals are about 0.7 of it.
The first line's letters stand on the X axis and each `\n` starts a line
below, `lineSpacing` times the font's line height. `align`
(`TextAlignment.left`, `center`, `right`) puts each line's start, middle or
end on the origin.

Letters are kerned as the font says. `font.measure(text)` gives the width of
the widest line and the height of the lines, for a board or a button to fit.

### Letters

Letters: each one has the holes its font gives it (the middle of an o,
the two in a B), its curves smooth at any size. A letter the font hasn't got
is drawn as the font's missing-letter box. Text is laid out left to right,
one letter after another: no Arabic joining, no right-to-left, no emoji.
Fonts that need those take a font shaper, which isn't in the engine.

### The texture

The texture lies flat across the front and back, `uvPerMetre` repeats a
metre, and wraps along the sides.

### What it costs

What it costs: two lines of 26 letters are about 4,000 triangles and
take 0.4 ms on a Mac. A score changing a few times a second is fine; text
that changes every frame is better as a Flutter widget over the view, pinned
to a place in the world ([Widgets in the world](/flutter/world-widgets/)),
or flat text in the scene ([Flat text](/assets/flat-text/)).
