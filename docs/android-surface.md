# Android: drawing straight to the screen

Status: done 2026-10-16: [the report](android-surface-report.md).
Accepted 2026-10-16 (your call: "do step 5"). Follows
[the Android frame pacing report](android-pacing-report.md), step 5.
Scope and rules: [ADR-0001](decisions/0001-fynkel-2-code-first.md).

## Why

Frame pacing got the Note10+ from about 35 to 51 fps in Sheepdog at low,
and from 13–26 to 40 at medium, but not to 60. What's left is Flutter:
Android shows our frames as a Flutter texture, so Flutter waits for each
of them on Android's main thread, where Dart also runs. Dart then ticks
only as often as frames are shown.

If our frames go to a surface of their own, Android's compositor shows
them and Flutter never waits for them.

## How

Two ways a Flutter app shows an Android surface:

- **A. A platform view** (`AndroidView`) holding a `SurfaceView`.
  Flutter 3.47's best mode for it (HCPP) needs Android 14; the Note10+
  has 12, so it gets classic hybrid composition, which renders Flutter
  over the view in extra layers and runs Flutter's raster work on the
  main thread.
- **B. A `SurfaceView` behind a transparent Flutter view.** The plugin
  puts it in the activity, under Flutter's view, where the `FynkelView`
  is laid out; Flutter draws nothing there, so the game shows through,
  with the HUD and any widget over it drawn by Flutter as now. Flutter's
  own frames don't change at all. A game's activity has to make Flutter's
  view transparent (one line, or a `FynkelActivity` the plugin gives).

B keeps Flutter wholly out of our frames, so it is tried first.

## Steps

### 1. Spike (1 day)

B, the least that measures: the plugin adds a `SurfaceView` behind
Flutter's for each view, where Dart says it is, and attaches the engine
view to its surface (the engine already draws to any Android surface).
Sheepdog's activity transparent. Measured as the pacing runs were:
Sheepdog low and medium, the look scene medium.

- If Sheepdog holds 60 at low and the look scene ≥ 55 at medium: on to
  step 2.
- If not: A is spiked the same way; if neither helps, it's written down,
  and the texture stays.

### 2. Build it properly

- The plugin: a view's place and size follow its layout every frame
  (scrolling, transitions, a minimap over the field); its order follows
  the widget tree's; the surface comes and goes with the activity
  (background and back, rotation); hot restart.
- Widgets: what's over a view draws over it; what's under it doesn't
  show (said in the guide). A view clipped or faded by Flutter (a page
  transition) can't be, on this path: the guide says so, or the texture
  stays for those.
- The default: the surface on Android, the texture kept as an option
  (`FynkelView(androidSurface: false)`), if it's still wanted.
- A new game: the guide's §1 steps (the activity).
- Tests: the plugin's lifecycle as now (the widget tests' stand-in), and
  on the phone.

### 3. Measure and report

The goal's table again (docs/android-pacing.md), the Mac and the iPhone
once (untouched), and `Quality.device` back to medium on Android if
medium holds 55. The report: `docs/android-surface-report.md`. One PR.
