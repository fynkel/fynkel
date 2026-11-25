---
title: "On-screen controls"
description: "A thumb stick and buttons, as Flutter widgets that feed the same actions."
---

**Flutter controls** feed the same axes and actions as the keys, as Unity's
on-screen controls do, so the game reads one path. The on-screen stick is
`Stick(world.input)` (from `package:fynkel_flutter`, Unity's `OnScreenStick`):
placed where it goes, its knob sets the 'move' axis (`axis:` names another),
up the screen being +y, and letting go zeroes it.

`Stick(input, floating: true)` isn't drawn till a thumb comes down in its
area, and appears under it: give it the area (the screen's bottom half), and a
tap there with no drag still reaches the game. A widget of your own calls
`input.setAxis2('move', Vec2(x, y))` (it adds to the keys, and stays until set
again), and a button calls `input.action('jump') .press()` when it goes down
and `.release()` when it comes up (or both at once for a tap).

With no widget, `input.stick()` reads a finger dragging alone as a stick:
`input.stick() ?? input.axis2('move')`. A mouse held down drags it too, so a
phone game plays on a Mac; a game whose mouse aims and fires leaves it out
(`input.fingers.any((f) => f.kind == FingerKind.mouse) ? null :
input.stick()`).

```dart
// lib/walk_page.dart
import 'package:flutter/widgets.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'game.dart';

class WalkPage extends StatelessWidget {
  const WalkPage({super.key});

  @override
  Widget build(BuildContext context) => FynkelGame(
    Score.new,
    builder: (context, game) => Stack(
      fit: StackFit.expand,
      children: [
        FynkelView(root: game),
        // The bottom half: a thumb anywhere there walks.
        Positioned(left: 0, right: 0, bottom: 0, height: 300, child: Stick(game.world.input, floating: true)),
      ],
    ),
  );
}
```
