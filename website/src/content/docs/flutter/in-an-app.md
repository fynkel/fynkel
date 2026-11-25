---
title: "A game inside an app"
description: "A title screen, a game page, tabs, and turning the phone."
---

A game inside a bigger app (a title screen, a menu) uses the pieces
`FynkelApp` is made of: a **`FynkelGame`**, which makes the root and can
start it over, and a **`FynkelView`**, which shows it:

```dart
// lib/play_page.dart
import 'package:flutter/material.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'game.dart';
import 'hud.dart';

class PlayPage extends StatelessWidget {
  const PlayPage({super.key});

  @override
  Widget build(BuildContext context) => FynkelGame(
    Score.new,
    // Text needs a Material above it, as anywhere outside a Scaffold:
    // without one, it's red with a yellow underline.
    builder: (context, game) => Material(
      type: MaterialType.transparency,
      child: Stack(
        fit: StackFit.expand,
        children: [
          FynkelView(root: game),
          ScoreHud(game),
          Align(
            alignment: Alignment.bottomCenter,
            child: GestureDetector(
              onTap: () => FynkelGame.maybeOf(context)?.restart(),
              child: const Text('again'),
            ),
          ),
        ],
      ),
    ),
  );
}
```

`FynkelGame` makes the game's world and adds the root to it before the
builder runs, so `game.world` is there from the start: a widget beside
the view can take `game.world.input` while it's built, as an on-screen
stick does (`Stick(game.world.input)`). The view shows the game's world;
a game whose assets don't come from the app's bundle gives
`FynkelGame(source: ...)`.

A page pushed over this one with `Navigator` (or a dialog) isn't below
its `FynkelGame`, so `FynkelGame.of` there doesn't find it: give the page
the game (`SettingsPage(game)`). A `FynkelGame` keeps the game it
started with when it's built again with another `make`; give it a `Key`
to start another (`key: ValueKey(level)`). Nothing warns you: a closure
made again in a build (`() => Level(number: level)`) is a new function
every time, whether or not it makes something else.

Leaving the page, or starting over, destroys the game as a scene unloads
in Unity: every object gets `onDisable` and `onDestroy`, so clean-up and
saving there run, and then the engine's memory is freed.

## Turning the phone

The view takes its new size and the game goes on:
`input.width` and `height` change, and a drag held through the turn goes on. A
camera's `fovY` is the angle top to bottom whatever the shape, so turned on
its side a view shows as much height and more width; to frame the same in
both, set `fovY` from `input.width / input.height` when it changes.

On Android, `flutter create`'s styles keep the app out of the camera's cutout
in landscape, which leaves a black band down that side: add `<item
name="android:windowLayoutInDisplayCutoutMode">shortEdges</item>` to
`LaunchTheme` and `NormalTheme` in
`android/app/src/main/res/values*/styles.xml` so the view fills the screen.

Put the HUD in a `SafeArea` so its buttons stay clear of the notch, the cutout
and the home bar; the view itself can go under them.

## Tabs

A view in a tab that isn't showing, kept alive by an `IndexedStack`
(or under a `Visibility.maintain` that hides it), is paused the same way, and
on a phone its surface is hidden too, so it doesn't show through the shown
tab's view. So is a view on a page another page covers whole, once that page
has slid in (not under a dialog, which shows it): paused, it runs 10 frames a
second as under a dialog, its unscaled time going on (an idle game earns in
its shop), and draws nothing.

With `pauseWhenCovered: false` it runs on at its rate there, unseen. A world
shown in more than one view is paused only when none of them shows it.

## If the view can't draw

A lost GPU device or a refused surface gets
a new view by itself, up to three times a minute; after that the view
gives up, and its place shows an `ErrorWidget` while the world runs on.
`FynkelView(onError: (error, stack) => ...)` hears that once, for a crash
reporter, and `errorBuilder: (context, error, retry) => ...` shows your
own message in its place, with a button calling `retry` to try a new
view.
