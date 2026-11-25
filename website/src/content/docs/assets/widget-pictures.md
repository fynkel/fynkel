---
title: "A widget as a picture"
description: "Draw a Flutter widget into a texture: posters and scoreboards."
---

**`widget.toTexture(size:, pixelRatio:)`**, from `package:fynkel_flutter`,
draws a widget once into a texture: a poster, a scoreboard, a shop's
price list with the game's own fonts and look, shown by a sprite or a
material (`Material(baseColorTexture: …)`):

```dart
// lib/poster.dart
import 'package:flutter/material.dart' show BorderRadius, BoxDecoration, Center, Colors, Container, Size, Text, TextStyle;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

/// A poster for [level], drawn by Flutter, on a sprite 2.4 by 1.5 metres.
Future<GameObject> poster(int level) async {
  final picture = await Container(
    decoration: BoxDecoration(color: Colors.indigo, borderRadius: BorderRadius.circular(12)),
    child: Center(
      child: Text('LEVEL $level', style: const TextStyle(color: Colors.white, fontSize: 28)),
    ),
  ).toTexture(size: const Size(240, 150), pixelRatio: 2);
  return GameObject(name: 'poster', components: [SpriteRenderer(picture, size: Vec2(2.4, 1.5))]);
}
```

It's a picture, not a live widget: it doesn't animate and takes no taps. To
change it, draw it again and give the sprite the new texture, then dispose the
old one: a scoreboard once a second, not every frame.

The widget is drawn round only a `Directionality` and a `MediaQuery` of its
size, so a `Theme` the game wants is put round it by the game; and images in
it must be loaded first (`precacheImage`), as nothing waits for them.

On Android it waits for the app's first frame before drawing. For a widget
that stays live, a button or a bar that changes, use a `WorldWidget` ([Widgets
in the world](/flutter/world-widgets/)), which stays over the scene rather
than in it.
