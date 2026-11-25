---
title: "Time and frame rate"
description: "time.delta, pausing, slow motion, fixed steps and the frame rate."
---

**`time`** is on every object and behaviour:

- `time.delta`: this frame's seconds, scaled. Multiply every speed by it.
- `time.scale`: 0 stops everything that uses `delta` (moves, waits, tweens);
  0.5 is slow motion. To pause the game, use `world.paused` ([Keys and
  pausing](/input/pause/)), which stops the input too.
- `time.unscaledDelta`, `time.sinceStart`, `time.unscaledSinceStart`,
  `time.frame`.
- `time.maxDelta` (1/3 s): a long hitch counts as at most this much game time,
  whatever the scale, so nothing jumps through walls.
- `time.maxFixedSteps` (5): the most fixed steps a frame runs. Raise it for a
  game that runs `time.scale` fast at a low frame rate.
- `time.fixedDelta`, `time.fixedAlpha`, `time.inFixedStep`: the fixed steps.

## The frame rate

is at most `world.targetFrameRate` frames a second, as
Unity's `Application.targetFrameRate`: 60 unless the game sets it, on every
device. A game that wants more sets 120 (or 0, for every frame the display
shows); one that wants less, 30.

On a 120 Hz screen, 60 is every other display frame: the world isn't updated
and nothing is drawn in the others, and taps and keys wait for the next frame
of the world's. Change it at any time, from a settings page for example:

```dart
/// The settings page's choice: smoother (the most a ProMotion screen
/// shows), or the default, which keeps the phone cool.
void chooseFrameRate(World world, {required bool smoother}) {
  world.targetFrameRate = smoother ? 120 : 60;
}
```

60 is the default because more costs the phone heat: Sheepdog at 120 on an
iPhone 16 Pro Max made it hot enough within a minute that iOS slowed it to
about 80; at 60 it holds 60 for 10 minutes and more ([the
report](https://github.com/fynkel/fynkel/blob/main/docs/iphone-heat-report.md)).

For 120 on an iPhone, the app's `ios/Runner/Info.plist` needs
`CADisableMinimumFrameDurationOnPhone` (`true`), as every example has (a new
`flutter create` app has it; an older app may not): without it, iOS keeps the
app's screen at 60, whatever the game asks. Behind a menu, a paused game draws
less still (`pauseWhenCovered`, [Menus and pages](/flutter/menus/)).

## Fixed steps

cut the game's time into equal steps of `time.fixedDelta`
(1/50 s), so what runs in them goes the same at 30 fps and at 120. A frame
runs as many as its time holds: at 120 fps most frames have none, at 30 some
have two.

A frame runs at most `time.maxFixedSteps` (5), and drops the time past that,
so a step slower than its frame can't snowball. Inside a step `time.delta` is
`time.fixedDelta`. To draw something a step moves without it stepping, draw it
between where the last two steps left it, `time.fixedAlpha` of the way (0 to
1).
