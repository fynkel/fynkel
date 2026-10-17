# iPhone: running cool

Status: done 2026-10-17 (your call: "let's go"). The report:
`docs/iphone-heat-report.md`. Scope and rules:
[ADR-0001](decisions/0001-fynkel-2-code-first.md).

## Why

The iPhone runs every scene at 120 fps, but you feel it warm up even
in a simple one. The numbers agree: on 2026-10-16 Sheepdog at high gave
120, then 113, 83 and 99 in back-to-back runs. Main gave 98 the same
day. The render thread was idle most of the time while frames were
skipped, so the GPU had slowed down, which is what a warm phone does.

What we run today, on every iPhone, for every scene:

- **120 frames a second.** Every example opts into 120 Hz
  (`CADisableMinimumFrameDurationOnPhone`), and the view draws a new
  frame on every tick, even when nothing on screen has changed.
- **High quality.** `Quality.device` picks high on any Apple GPU, so
  shadows, bloom and ambient occlusion are on.
- **The full resolution**: 1320 × 2868 on the iPhone here.

Unity, our reference, runs phones at 30 fps unless a game asks for more
(`Application.targetFrameRate`). It can also draw less often than it
updates (`OnDemandRendering.renderFrameInterval`).

All our iPhone numbers so far are fps over 60 s. Heat builds over
minutes, and nothing measures it or power use yet. So this measures
first, and changes only what the numbers say matters.

## The goal

On the iPhone here, held as a player would hold it, after 10 minutes:

| Scene | Thermal state | fps |
|---|---|---|
| Hello (one cube) | stays nominal | holds its rate |
| Sheepdog, at the default settings | never serious | last minute within 5 % of the first |

The thermal state is iOS's own (`ProcessInfo.thermalState`: nominal,
fair, serious, critical). iOS slows the phone down at serious. And your
hand on the phone, before and after: you have the final say.

## Steps

### 1. Measure (1 day)

- **The thermal state**, in the measure tool: the plugin reports iOS's
  thermal state and each change to it. `fynkel_measure rate --ios` prints
  when each change happened, and `--seconds 600` runs 10 minutes.
- **Runs**, 10 minutes each, over Wi-Fi (your call: the phone off the
  charger), at the same brightness, starting cool:
  - Hello: one cube, nothing moving.
  - Sheepdog at high (today's default), playing.
  - Sheepdog paused, with the pause menu open.

The day ends with a table: what heats the phone.

Changed on the way (2026-10-17): Xcode's Power Profiler is dropped. Over
Wi-Fi it hung or saved nothing about half the time. The thermal state and
the fps minute by minute say what we need. The costs of each option one by
one (60 fps, medium, 0.75) are dropped too, your call ("can't we do this
in shorter time?"): the baseline already showed what to change, so the
changes are built and measured instead.

### 2. Change what the numbers say (1–2 days)

These are the likely changes. Each one is built only if step 1 shows it
matters, and measured again after:

- **A frame rate a game chooses**, like Unity's
  `Application.targetFrameRate`: `world.targetFrameRate`, 60 by default
  everywhere, and 120 (or 0, every display frame) when a game asks for it
  (your call). Flutter 3.47 has no way to lower the display's rate while
  running on iOS: `CADisableMinimumFrameDurationOnPhone` in Info.plist
  is the only switch. So the examples keep it, which lets a game ask for
  120, and the engine runs and draws only every other display frame at 60.
- **The resolution a game chooses**: `Quality.renderScale`, like Unity's
  render scale, 1 at every level, which a game can change at any time
  from its settings page. A view's own `resolutionScale` stays, for one
  view (a minimap).
- **Nothing drawn when nothing changed**: a paused game, a still scene
  behind a menu. The frame is drawn again when something moves, the
  camera changes or the view resizes. Unity's equivalent is
  `OnDemandRendering`.
- **The iPhone's defaults**: if high costs much more than medium for
  little difference to the eye, `Quality.device` picks medium on iPhones
  (iPads and Macs stay high), or the resolution drops to 0.75.
- **The app in the background or the view off screen draws nothing.**
  Checked; it should already be so.

### 3. Report (½ day)

The goal's table before and after, the costs from step 1, what changed
and what didn't, and the guide (§5 quality, §6 the frame, §18 speed, §20
from Unity). The Mac and the Note10+ once each, to show they're
unchanged. One PR, and then your 10 minutes with Sheepdog in your hand.

## Not covered

- Other iPhones: only one is here. The defaults are by kind of device,
  phone or not, so an older iPhone gets the same, and may need less.
- Battery life as a number of its own: power is measured, so it follows
  from that, but it isn't a separate goal.
