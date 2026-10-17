# iPhone: drawing straight to the screen — the report

[The plan](ios-surface.md). Built in a day. All the runs were on the
iPhone 16 Pro Max (iOS 26.6.1) over Wi-Fi, unplugged, Sheepdog at high,
with `fynkel_measure rate --ios`; the logs are in `~/fynkel-runs/heat/`.

## The result

**Sheepdog at high, at 60, stays nominal for 10 minutes: the phone no
longer gets warm. Flutter's own frames still come at 120 a second, none
slow.**

| iPhone, Sheepdog at high, unplugged | fps by minute | thermal state | Flutter's frames | render thread |
|---|---|---|---|---|
| Before (#118, a texture), 10 min | 60 each (slowest second 50) | nominal, **fair from 313 s** | 72,101, none slow | busy 16 % |
| Spike (step 1), 10 min | 60 each (slowest 58) | nominal throughout | 72,117, none slow | busy 19 % |
| **Now (a layer of its own), 10 min** * | **60 each** (slowest 59) | **nominal throughout** | 71,999, 1 slow | busy 20 % |
| Now, paused behind a menu, 5 min | 10 each (as meant) | nominal throughout | 36,120 | busy 4 % |
| Now, Hello, 1 min | 60 | nominal | 7,200, none slow | busy 5 % |

\* It started 5 minutes after your hands-on check, so a little warm, as
#118's run started 5 minutes after a run that ended fair.

The goal was "nominal longer, or throughout; 60 each minute; Flutter at
120, smooth": all met. Your hands on the phone found the HUD, the
minimap, the pause menu, settings, rotation and going home and back all
working.

Why: as a texture, the game's frame was drawn again into Flutter's
picture each time Flutter drew, 120 times a second, 1320 × 2868 pixels
each, though the game draws only 60. On a layer of its own iOS puts it on
the screen beside Flutter's, and Flutter's picture holds the UI alone.
The render thread is a little busier (it waits for the layer's drawables
itself), and the phone is cooler all the same: the copies were the cost.

## What changed

- **iOS draws to a layer of the game's own**, a `CAMetalLayer` under
  Flutter's see-through view, where the `FynkelView` is, as Android's
  surface (#117). The plugin finds Flutter's view through its registrar
  and makes it see-through itself: a game's app needs no change. Where
  there's no Flutter view of its own (Flutter added to an app's own view
  controllers), the view is a texture, quietly.
- **One setting for both phones**: `FynkelView.androidSurface` is
  `onScreen`; `onScreen: false` keeps the texture. A breaking change, in
  the guide's §21.
- **The engine**: a layer target (`fynkel_present::Layer`) presents each
  frame to the layer's next drawable; a new size sets the same surface up
  again; a frame in progress holds the layer while the plugin swaps or
  takes it. ABI 30 (`fy_view_new_apple_layer`, `fy_apple_layer_attach`,
  `fy_apple_layer_detach`).
- **The guide**: §1 (nothing to do on iOS), §15 (the limits are both
  phones'), §18, §21.

## The Mac and the Note10+

Both keep what they had.

- The Mac (a texture still), Sheepdog at high, 60 s, run right after the
  same on `main`: 60 fps, the render thread busy 11 %, 7,195 Flutter
  frames, against 60, 11 % and 7,281 on `main`. (Two runs while the
  window was behind another gave 1,467 and 2,451 Flutter frames: the Mac
  draws a covered window less, the code had nothing to do with it.)
- The Note10+ (Android 12, just updated), Sheepdog at its own quality
  (low), 60 s: 59.9 fps (slowest second 54.9), against 58.9 (41) in #118.

## Tests

- New, the iOS way (a stand-in for the plugin, on the Mac): a view
  registers a layer with its size, place and the engine's calls, is
  placed and resized as it moves, and goes with the view; `onScreen:
  false` keeps the texture; no layer to give falls back to a texture with
  no error; a failed resize brings a new layer.
- `fynkel_flutter` 90, and every example's widget tests pass (Sheepdog
  32, the lab 88); the guide's code compiles; clippy for the Mac and iOS.
- On the phone, by you: the HUD and minimap over the field, the pause
  menu and settings, rotation, home and back (and another app between).

## Not done

- **Flutter's slow frames behind the menu**: 1,180 of 36,120 took more
  than a display frame to draw, against 1,430 in #118, with the texture.
  It's the dialog's own drawing (its dimmed background over the whole
  screen), not the game's: the game draws 10 a second there. Flutter's
  to look at, if it shows.
- **The swipe back during play** does nothing on the iPhone, as before:
  Sheepdog's play page turns a back into its pause menu (`PopScope`),
  and on iOS that turns the edge swipe off. A swipe that opens the pause
  menu is a small change of Sheepdog's, if wanted.
- **The Mac** keeps the texture (the plan's "not covered").
