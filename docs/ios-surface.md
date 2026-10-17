# iPhone: drawing straight to the screen

Status: proposed 2026-10-17 (your call: "let's merge and then do the new
plan"). Follows [iPhone heat](iphone-heat-report.md), as Android's
[drawing straight to the screen](android-surface.md) followed its frame
pacing. Scope and rules: [ADR-0001](decisions/0001-fynkel-2-code-first.md).

## Why

At 60 fps, Sheepdog at high keeps the iPhone from serious for 10 minutes,
but it still gets warm: fair after about 5 minutes. Android doesn't, and
one thing Android has that the iPhone hasn't is its own place on the
screen (#117).

On the iPhone our frames are a Flutter texture. Flutter draws its picture
120 times a second, as it should for its UI, and each time it draws the
game's whole frame into it: 1320 × 2868 pixels, 120 times a second,
though the game draws only 60. If the game has a layer of its own, iOS
puts it on the screen beside Flutter's, and Flutter's picture holds the
UI alone.

Your rule (2026-10-17): Flutter and its UI stay as smooth as Flutter
makes them, at the display's rate. Only the engine draws less.

## How

As on Android: **a layer of the game's own behind a see-through Flutter
view.**

- **The plugin** puts a `UIView` with a `CAMetalLayer` under Flutter's
  view in the window, for each `FynkelView`, where Dart says it is
  (`place`, as on Android), and one over another where Dart says so
  (`over`: Sheepdog's minimap). It makes Flutter's view see-through
  itself (`FlutterViewController.isViewOpaque = false`), so a game's
  app needs no change, unlike Android's activity.
- **Flutter's side** is Android's already: the view paints a hole where
  it is, and sends its place each frame it changes.
- **The engine** presents to the layer (wgpu draws to a `CAMetalLayer`)
  instead of the IOSurfaces, at the game's own rate: 60 frames a second
  while Flutter draws 120.
- **The texture stays**, as Android's does: `FynkelView(androidSurface:
  false)` becomes a setting for both phones (its name to be settled: one
  for both), and the Mac keeps the texture (below).

The other way, a platform view (`UiKitView`), makes Flutter split its
picture around the view and merge its threads, which costs Flutter's own
frames; it's tried only if this one fails.

The limits are Android's (the guide, §15): Flutter can't fade, clip or
move the game's layer in a page transition; what's painted under the view
doesn't show; two views deep, not three.

## The goal

On the iPhone here, unplugged, Sheepdog at high, at 60, 10 minutes, from
cool:

| | now (texture, #118) | goal |
|---|---|---|
| thermal state | nominal, fair from 313 s | nominal longer, or throughout |
| fps by minute | 60 each | 60 each |
| Flutter's frames | 120 a second | 120 a second, smooth |

And your hand on the phone.

## Steps

### 1. Spike (½–1 day)

The least that measures: the plugin puts a layer under Flutter's view for
Sheepdog's field, the engine presents to it, Flutter's view see-through.
The same 10-minute run as #118's. If the phone is cooler, on to step 2;
if not, it's written down, and the texture stays.

### 2. Build it properly

- The plugin: place and size follow the layout (scrolling, rotation, a
  minimap over the field); the layers come and go with the app
  (background and back) and hot restart (`releaseOrphans`).
- The engine: a resize, frame pacing with the game's frame rate, a lost
  device; the frames a covered, paused game draws (10 a second) still
  hold.
- The setting for both phones, the guide (§15, §18, §21), tests (the
  widget tests' stand-in for the plugin, and on the phone).

### 3. Measure and report

The goal's table, Sheepdog paused behind a menu, Hello, the Note10+ and
the Mac once (untouched). The report: `docs/ios-surface-report.md`. One
PR.

## Not covered

- **The Mac** keeps the texture: it doesn't get hot, and its windows move
  and resize in ways a phone's don't. It can follow later the same way.
- **The quality.** The iPhone draws at high, at 1.6 × the Note10+'s
  pixels; if it's still warm after this, that's the next lever, your
  call.
- **Other iPhones and iPads**: one iPhone is here (an iPad on iOS 17
  too, for a check that it works).
