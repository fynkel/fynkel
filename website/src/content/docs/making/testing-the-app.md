---
title: "Testing the whole app"
description: "Widget tests with the real game in them."
---

**The whole app works in a widget test too**: the real `FynkelApp`, its
view and its overlay, real taps. With no GPU plugin in a test, the game
runs unseen, frame by frame as `tester.pump` moves the clock, and the view
is the test window's size. `package:fynkel_flutter/testing.dart` moves it
on, `tester.frames(2)` two frames and `tester.run(0.5)` half a second, and
finds it, `tester.game<MyGame>()`:

```dart
// test/app_test.dart
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_flutter/testing.dart';
import 'package:my_game/game.dart';
import 'package:my_game/hud.dart';

/// The Score game with a cube to tap, in front of a camera.
class TapToScore extends Score {
  @override
  void onLoad() {
    super.onLoad();
    add(GameObject(transform: Transform(position: Vec3(0, 0, 4)), components: [Camera()]));
    add(Target());
  }
}

class Target extends GameObject {
  Target() : super(components: [MeshRenderer(Mesh.cube(), Material())]);

  @override
  bool onClick(Finger finger) {
    (parent! as Score).points.value++;
    return true;
  }
}

void main() {
  testWidgets('a tap on the cube scores, and the HUD shows it', (tester) async {
    await tester.pumpWidget(FynkelApp(TapToScore.new, overlay: (context, game) => ScoreHud(game)));
    await tester.frames(); // a frame: the game loads and starts
    final game = tester.game<TapToScore>();
    final input = game.world.input;
    final cube = game.world.camera!.worldToScreenPoint(Vec3.zero, width: input.width, height: input.height)!;
    await tester.tapAt(Offset(cube.x, cube.y));
    await tester.frames(); // the frame that hears it
    expect(game.points.value, 1);
    await tester.pump();
    expect(find.text('★ 1'), findsOneWidget);
  });
}
```

## What differs from other widget tests

Two things differ from other widget tests. **`pumpAndSettle` never
settles** while a view is on screen, since the game asks for every frame:
`tester.frames()` or `tester.run(seconds)` instead. And input waits for
the next frame, as on a device: a tap reaches the game in the frame after
it. A page that slides in or out (a settings page, a dialog) takes about
a third of a second, so `tester.run(0.5)` after opening or closing one,
not a few frames.

## A game that loads files

(`assets.instantiate`, a texture, a sound) reads
them from the app's own bundle in a widget test too, as listed in
pubspec.yaml, with nothing to set up. It reads them in real time, which a
widget test's frames don't move: `await tester.loaded()` waits for the game on
screen to load, pumping frames meanwhile, and throws if its load failed
(`tester.loaded(level)` waits for another object).

What the test can't reach, as a level a button makes inside the game, is
waited for by what it changes: `await tester.until(() => game.level.value ==
2)` runs the game the same way until that holds. `await tester.runAsync(() =>
game.loaded)` would wait for ever: the game's `onLoad` goes on after each
`await` only when a frame is pumped.

## The app going away

**The app going away and back** is the binding's:
`tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused)`
gives the game `onApplicationPause(true)` (save there), and
`AppLifecycleState.resumed` brings it back. In a plain `World` test,
call the game's own saving code.

**flutter_test has a `find` of its own**, and in a test file it hides
`GameObject.find`: in a game object written in a test, `find('cube')`
is flutter_test's (`The method 'call' isn't defined for the type
'CommonFinders'`). Say `this.find('cube')` there.
