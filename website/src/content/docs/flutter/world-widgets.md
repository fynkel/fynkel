---
title: "Widgets in the world"
description: "Health bars, name tags and arrows pinned to objects."
---

A health bar over an enemy, a name tag, a
damage number or a "press E" hint is a Flutter widget on the object: a
`WorldWidget`, from `package:fynkel_flutter`. Every view of the world
shows it over the game, where its camera sees the object, each frame:

```dart
// lib/tagged.dart
import 'package:flutter/material.dart' show Alignment, Colors, Text, TextStyle;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

/// A cube with its name over it.
class Tagged extends GameObject {
  Tagged(String name) : super(name: name, components: [MeshRenderer(Mesh.cube(), Material())]) {
    addComponent(
      WorldWidget(
        (context) => Text(name, style: const TextStyle(color: Colors.white)),
        offset: Vec3(0, 0.8, 0), // above it, in the world's directions
        alignment: Alignment.bottomCenter, // the tag's bottom middle there
        shrinkBeyond: 8, // smaller farther than 8 from the camera
      ),
    );
  }
}
```

The widget is built once and then only moved, so a hundred cost little (100
health bars in the arena hold 120 fps on the Mac and the iPhone). To change
what it shows, it rebuilds itself as any widget does: a
`ValueListenableBuilder` of a `ValueNotifier` on the object, or a
`StreamBuilder` of a signal's `stream` (the arena's `HealthBar`). It's hidden
while its object is inactive, behind the camera or off the view, and the
nearer ones are drawn over the farther ones.

A tap on it is its own, a button in it works, and the game doesn't see that
tap. For a widget at a point, a damage number say, put it on an empty object
there, and move or destroy that.

## Walls and the screen's edge

It moves with the game's picture, not ahead of it: the engine's picture of a
frame reaches the screen a frame after Flutter's on a Mac or an Android phone,
and the widgets wait for it there. Two settings change where it shows.

**`hideBehind: true`** hides it while something is between the camera and its
point, a pillar or a wall (not its own object), fading over 0.15 s;
`hideLayers` says what counts (`LayerMask.except([glass])` sees through
glass). It looks with `world.raycast`, eight widgets a frame in turn, so a
hundred cost eight casts, not a hundred: among 3,000 objects, 0.3 ms a frame
on a Mac and 1.3 ms on a Note10+ (each cast still looks at every object; a
tree over them is to come).

**`keepOnScreen: true`** keeps all of it inside the view's edge, `edgeMargin`
in: a speech bubble over someone at the side doesn't hang off, and one whose
point leaves the view, or goes behind the camera, waits at the edge toward it.

`WorldWidget.of(context)` tells its builder when it's waiting there and which
way the point is, for an arrow:

```dart
// lib/goal_arrow.dart
import 'package:flutter/material.dart' show Colors, Icon, Icons;
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show GameObject, Vec3;
import 'package:fynkel_flutter/fynkel_flutter.dart';

/// The goal: "goal" over it, an arrow at the screen's edge toward it
/// while it's off the screen.
class Goal extends GameObject {
  Goal() : super(name: 'goal') {
    addComponent(WorldWidget((context) => const GoalMark(), offset: Vec3(0, 1, 0), keepOnScreen: true, hideBehind: true));
  }
}

class GoalMark extends StatelessWidget {
  const GoalMark({super.key});

  @override
  Widget build(BuildContext context) {
    final place = WorldWidget.of(context);
    return ValueListenableBuilder(
      valueListenable: place.offScreen,
      builder: (context, off, _) => off
          ? ValueListenableBuilder(
              valueListenable: place.angle, // 0 to the right, pi / 2 down
              builder: (context, angle, _) =>
                  Transform.rotate(angle: angle, child: const Icon(Icons.arrow_forward, color: Colors.yellow)),
            )
          : const Text('goal'),
    );
  }
}
```

(Off the screen, `hideBehind` doesn't hide it: the arrow shows whatever
is in the way.)
