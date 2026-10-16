# Android: drawing straight to the screen — the report

[The plan](android-surface.md), step 5 of
[Android frame pacing](android-pacing-report.md). Built in a day. All
the runs were on the Galaxy Note10+ (Android 12, Mali-G76, 60 Hz), 60 s
each, under `caffeinate -du`; the logs are in `~/fynkel-runs/pacing/`.

## The result

**Sheepdog holds 60 at low, and the look scene holds 59 at medium. Sheepdog
at medium doesn't reach the goal.**

| Note10+ | before #116 | after #116 (texture) | now (on screen) | goal |
|---|---|---|---|---|
| Sheepdog, low | 34–36 (min 27–28) | 51 (min 35–39) | **59.9 (min 50)** | 60 (min ≥ 50) |
| Sheepdog, medium | 13–26 | 40 | 42.9 (min 40) | ≥ 55 |
| The look scene, medium, by day | 34.4 | 43–50 | **59.2 (min 54)** | ≥ 55 |
| The look scene, medium, by night | 27.2 | 42 | **59.0 (min 52)** | — |
| The look scene, low, by day | 59.5 | 58.3 | 59.9 | — |
| Dart's ticks a second (Sheepdog, low) | — | 51–53 | 59.9 | 60 |

Sheepdog at medium is now bound by the GPU itself: it's 81 % busy at
572 MHz, and the render thread is busy 93 % of the time waiting for it.
Nothing between us and the screen is left to take out there. So, as the
plan says, **`Quality.device` stays low on Android**. A game can still
offer medium; the look scene shows that a lighter scene holds 60 at medium.

## What changed

**Each `FynkelView` on Android is a `SurfaceView` of its own, behind
Flutter's.** A game's activity extends `FynkelActivity` (the plugin's),
which makes Flutter's view see-through. The plugin puts a `SurfaceView`
under it where the `FynkelView` is laid out, and the engine presents to
its surface. Android's compositor shows it, as a hardware overlay on the
Note10+, and Flutter never touches our frames: Dart no longer waits for
them on Android's main thread, so it ticks 60 times a second again.

- **Flutter's side** (`fynkel_view.dart`): the view paints a hole
  (`BlendMode.clear`) where it is, so the game shows through whatever is
  painted beneath it. Each tick, it tells the plugin where it is now, if
  that changed (`place`), and whether it lies within another on-screen
  view, so it goes over it (`over`: Sheepdog's minimap over the field).
- **The plugin** (`FynkelFlutterPlugin.kt`): a `SurfaceView` per view;
  the surface's size follows the view's (`resize`); `over` is
  `setZOrderMediaOverlay`, which Android takes only before a view is in
  the window, so the `SurfaceView` is put in again. The surfaces come and
  go with the activity (background and back) and hot restart
  (`releaseOrphans`).
- **The engine** (`fynkel_present`, `fynkel_ffi`): `attach` takes
  `on_screen`. A surface on screen is presented without waiting for the
  GPU first: Android's compositor waits on the frame's fence itself, and
  the GPU, never left idle, is clocked up (at low, 377 MHz rather than
  260). The texture still waits, as #116 found it must.
- **The texture stays**: an activity left a `FlutterActivity`, or
  `FynkelView(androidSurface: false)`, works as before. The plugin falls
  back to it, and logs once why, when the activity isn't a
  `FynkelActivity`.

What it costs: one more image per view, for the reason below (about
10 MB for a full-screen view on the Note10+).

## A third image, when one view is over another

The first spike gave Sheepdog 59 at low; built properly, it gave 36–41.
The difference was the minimap. In the spike it was under the field,
hidden, though still drawn. Once it was properly over the field, both
views fell to about 38.

A trace showed both presenter threads spending about 80 % of their time
in `vkQueuePresentKHR`, waiting on a fence. Android holds the image on
screen, and in presenting one frame it waits for the one before. With two
images, a visible view's present waits for the display, and while it
waits it holds the GPU queue, which every view shares, so the field waits
for the minimap. With three images on an on-screen surface, Sheepdog at
low is back to 59.9, and its slowest second went from 43 to 50.

Ruled out on the way, each by a run: the hole painted by Flutter (40 fps
without it), Flutter's own surface (`RenderMode.texture` instead: 41),
the compositor (every layer an overlay, none composited by the GPU), and
the phone's heat.

## The limits (in the guide, §15)

Android composites the view, not Flutter, so:

- What's painted under a view doesn't show on it. Widgets over it draw
  over it as before.
- A transform, clip or fade Flutter applies to the view (a page
  transition) isn't followed. The view keeps its place and box.
- Views stack two deep: one on screen over another, not three.

`androidSurface: false` keeps the texture where those matter.

## Checked by hand on the phone

Sheepdog: the pause menu over the game, going to the background and back,
quitting to the menu and playing again, and a hot restart all work. The
minimap stays over the field, and the HUD stays over both.

## The Mac and the iPhone

Nothing changed there. The new arguments go only to Android's plugin,
and the engine change is in Android's presenter. Sheepdog at high, 60 s:

- The Mac: 120, as before.
- The iPhone: 113, 83 and 99 in three runs, against 120 at the end of
  #116. The runs skipped frames while the render thread was busy only
  18–23 % of the time, so the GPU was behind. Main without this work,
  built and run right after, gave 98 (slowest second 22). So the phone
  is slower today, not because of this work. It's connected over Wi-Fi,
  which wasn't so for #116's run.

## Tests

- New: the engine presents to an on-screen surface without waiting
  (`fynkel_present`, on the phone). The plugin's `place`, `over`,
  `unregister` and the texture fallback (`fynkel_flutter`'s widget tests,
  4 new).
- The phone tests (`tools/test-mobile.sh android`) pass, except the same
  8 textured goldens that fail on main (see
  [the pacing report](android-pacing-report.md#tests)).
- `fynkel_flutter` 80, Sheepdog 32 and the lab 88 widget tests pass, as do
  `cargo clippy` for the Mac and Android, and `fynkel_present`'s and
  `fynkel_ffi`'s Mac tests.

## Not covered

- Other Android phones. Only the Note10+ is here.
- Android 14's own way of doing this for platform views (HCPP) isn't
  needed: this path works from Android 11 (the plugin's minimum).
- Flutter's raster thread still has slow frames of its own: 573 of 3,585
  in Sheepdog at low. They no longer hold up the game's frames, only the
  HUD's, and they're for another day.
