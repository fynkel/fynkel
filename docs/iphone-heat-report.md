# iPhone: running cool — the report

[The plan](iphone-heat.md). Built in a day. All the runs were on the
iPhone 16 Pro Max (iOS 26.6.1) over Wi-Fi, Sheepdog at high, with
`fynkel_measure rate --ios`; the logs are in `~/fynkel-runs/heat/`.

## The result

**Sheepdog at high holds 60 fps for 10 minutes and the phone never gets
to serious. At 120 it got there within a minute, and iOS slowed it to
about 80.**

| iPhone, Sheepdog at high | fps by minute | frames skipped | thermal state |
|---|---|---|---|
| Before: 120, 10 min | 120, then 72 the other nine | 25,590 | fair from 72 s |
| Before: 120, unplugged, 5 min * | 120, 82, 82, 83, 83 | 8,579 | fair from 26 s, **serious from 51 s** |
| **Now: 60, unplugged, 10 min** | **60 each minute** (slowest second 50) | **0** | nominal; fair from 313 s, never serious |
| Now: 60, unplugged, 5 min | 60 each minute | 0 | nominal throughout |
| Now: 60, at 0.75 resolution, 5 min | 60 each minute | 0 | nominal throughout |
| Now: 60, charging, 10 min | 60 each minute | 0 | fair from 32 s, never serious |
| Now: paused behind a menu, 5 min * | 10 (as meant) | 0 | nominal throughout |
| Hello (one cube), 120, 10 min | 120 each minute | — | nominal throughout |

\* It started 5 minutes after a run that ended fair or serious, so a
little warm.

The goal was "never serious, the last minute within 5 % of the first":
both met. Your hand on the phone has the last say.

What heated the phone was the frame rate: twice the frames is twice the
GPU's work, and at high the iPhone can't keep that up for long. Sheepdog
at 120 is fine for a minute; the phone then slows itself down and the
game gets 70–80 fps, with uneven frames, where 60 would have been even.

The charger heats a phone too. It was in for the 60 fps, 10-minute run
marked "charging", and maybe for some of the night's runs; nobody knows
which. The unplugged runs say the same thing, so the result stands.

## What changed

- **`world.targetFrameRate`**, as Unity's `Application.targetFrameRate`:
  the most frames a second the world runs and its views draw, 60 unless
  the game sets it, on every device. 120 (or 0, every display frame) for
  a game that wants it, 30 for one that wants less; it can change at any
  time. On a 120 Hz screen, 60 is every other display frame: the world
  isn't updated and nothing is drawn in the others, and taps and keys wait
  for the next. A frame that comes up to half a display frame early still
  counts, so the frames don't drift.
- **`Quality.renderScale`**, as Unity's render scale: pixels drawn per
  pixel of the screen, 1 at every level, changed from the next frames, on
  a settings page for example. A view's own `resolutionScale` stays, for
  one view, and multiplies it. 0.75 made no difference to the heat at 60,
  so the iPhone keeps 1.
- **A game paused behind a menu draws 10 frames a second.** When a page
  or dialog covers the view and pauses the game (`pauseWhenCovered`),
  nothing in it moves, so its picture is the same. Paused, it cost as much
  as playing: fair after 35 s at 120 (before), after 116 s at 60. Behind
  a dialog now, for 5 minutes, started 5 minutes after the run that
  reached serious: 10 fps, the render thread busy 3 % of the time, and
  nominal throughout. Closing the menu goes back to 60 at once.
- **The measure tool** gives iOS's (and macOS's) thermal state and when
  it changed, each minute's frame rate on long runs, `--warmup`, and
  `--define FYNKEL_FRAME_RATE` (the game's own rate unless given).

## Not done

- **Flutter still ticks at the display's rate.** Flutter 3.47 has no way
  to lower an iPhone's display rate while the app runs:
  `CADisableMinimumFrameDurationOnPhone` in Info.plist is the only
  switch, and the examples keep it so that a game can ask for 120. So at
  60, Flutter's own frames are still 120 (7,322 in a minute on the Mac).
  They cost little: Hello at 120, with everything, stayed nominal for 10
  minutes.
- **Drawing only when something changed**, as Unity's
  `OnDemandRendering`, isn't built: the engine doesn't track what changed
  from one frame to the next. The paused menu was the case that needed it.
- **Xcode's Power Profiler** isn't used: over Wi-Fi it hung or saved
  nothing about half the time. The thermal state and the frame rate by
  minute were enough.
- **The look scene** wasn't run. Sheepdog at high is the heaviest scene we
  have on the phone.

## The Mac and the Note10+

- The Mac: Sheepdog at 60 by default (the render thread busy 4 % of the
  time), and 120 with `targetFrameRate = 0` (12 %), as before.
- The Note10+: [to run]. Its screen is 60 Hz, so nothing should change.

## Tests

- New: the frame rate (60 unless set, on a 120 Hz display every other
  frame; 0 and 30; each view of a world in the same frames), a tap between
  frames, the render scale (the size drawn changes without a new layout;
  out of range throws), and a covered, paused view at 10 a second.
- `fynkel_flutter` 86, and every example's widget tests pass (Sheepdog 32,
  the lab 88).
- Widget tests that pump 8 ms frames see every other one now: the
  engine's own set the world's `targetFrameRate = 0` or pump 17 ms.
