# Stage 8 report: the look

The last part of phase 7 of [the stage 8 plan](stage-8.md). Phases 1 to 6
are merged (#101–#103, #112–#114), with the hardening plan's six phases
between phases 3 and 4 (#104–#111). Phase 7 is this PR.

## The result

Stage 8's goal was a game that looks like a game. That means suns, lamps
and spotlights casting soft shadows; a sky that lights what's under it
and is reflected by it; see-through materials; fog; bloom on what glows;
and a light ambient occlusion. Each takes a few lines of Dart, is in the
guide, and is tested. A game sets its own quality, or starts from one of
three levels, or lets the device pick.

**That is done, with one exception: the Note10+.** The Mac and the
iPhone hold 120 fps at every level, day and night, in the lab scene that
has everything and in Sheepdog. The Note10+ holds about 60 at low in the
lab scene, but at medium (the level it picks itself) it runs at 27–34
fps, and Sheepdog, with two views and eight lamps, reaches only about 40
even at low. The cause was
found in phase 6 and accepted (your call, 2026-10-15). It is below,
under *What goes on*.

What a game writes for the look, all of it in Dart:

```dart
world.sky = Sky.gradient(zenith: blue, horizon: pale, ground: grass);
world.fog = const Fog(start: 18, end: 60);
addWith(Light.sun(direction: Vec3(-0.4, -1, -0.6), shadows: true));
GameObject(components: [Light.point(color: warm, range: 10)]);
GameObject(components: [Light.spot(range: 10, angle: 60.deg, shadows: true)]);
Material(color: glass, opacity: 0.35);
Material(color: Color.black, emissive: amber, emissiveIntensity: 4); // glows
Camera(bloom: const Bloom(), ambientOcclusion: const AmbientOcclusion()); // off unless given
world.quality = Quality.medium.copyWith(bloom: false); // or Quality.low, or null: the device picks
```

## What stage 8 did

| Phase | What | PR |
|---|---|---|
| 1 | Groundwork: passes in order, a half-float scene image above low, a light list, quality levels (`world.quality`, `Quality.device`) | #101 |
| 2 | Lamps and spotlights: `Light.point`, `Light.spot` | #102 |
| 3 | Shadows: the sun's in two cascades, spotlights' in an atlas, soft at medium and high; `castShadows`, `receiveShadows` | #103 |
| — | Hardening, six phases: the view and bridge, physics' sync, the Dart frame, the renderer, lights by place (clustered, no cap), what a game writes, the build hook and the guide | #104–#111 |
| 4 | The sky and what reflects it: `Sky.gradient`, `.color`, `.image` (`.hdr`), image-based light, fog | #112 |
| 5 | See-through: `Material(opacity:)`, sorted back to front, after the opaque | #113 |
| 6 | Bloom (medium and high) and a light AO (high), per camera | #114 |
| 7 | The look scene with everything; Sheepdog's look and quality setting; quality as settings; measured; the fresh agent; this report | this PR |

The C interface went from ABI 20 to 28. Each phase has its lab page, its
section of the guide's §5, its renderer tests with golden images, and the
same through C and Dart, with a lab page driven as a player would.

## Phase 7

- **The lab's look scene** ("Look: everything"): a courtyard with
  everything at once. By day: the sky image, the sun's shadows and fog.
  By night: four lamps round a pool, their bulbs glowing, and a
  spotlight casting shadows over crates against a wall. At any time:
  metal and plastic balls, see-through water and glass panes, and a ring
  of glowing blocks. It has buttons for each level and the device's pick,
  and switches for night, fog and the turning camera. `LAB_QUALITY` and
  `LAB_NIGHT` start it at a level, or at night, for measuring. Its test
  checks every part is there and that night swaps the sun for the lamps.
  With `LAB_SHOT`, it writes a picture at each level, day and night.
- **Sheepdog's look.** Phases 3 and 4 had already given it the sun's
  shadows under the sheep and fences, a gradient sky from its theme, and
  fog at the field's edges. Phase 2 had given it lamps, and phase 6 the
  glow. Phase 7 adds:
  - eight lamp posts along the fences, lit at night (it had four);
  - a test of the look: the sun's shadows, lamps along the fences that
    are off by day and on at night, and wolves' eyes bright enough to
    glow;
  - **Quality** on the settings page: Low, Medium, High or Auto, in the
    game at once (`Settings.quality`, `SheepdogGame(quality:)`), with a
    test that drives it as a player would;
  - `SHEEPDOG_QUALITY` for package:fynkel_measure's tool.
- **Quality is settings, not three fixed levels** (your call,
  2026-10-16). Before, each level decided for the game: no bloom at low,
  no ambient occlusion below high, whatever the camera asked. Now a
  `Quality` holds the settings, as Unity's quality levels do: `hdr`,
  `bloom`, `ambientOcclusion`, `lampHighlights`, `shadows`, and the
  shadows' cascades, resolutions, softness, distance and spot lights.
  `Quality.low`, `medium` and `high` are where a game starts from
  (`copyWith`); a game can keep one quality or have its own levels.
  - Cameras have no bloom or ambient occlusion unless given them, as in
    Unity; the lab's look scene, Sheepdog and the small games with
    glowing materials ask for them.
  - A debug build says once when a camera asks for an effect the
    quality drawn leaves out.
  - Settings out of range throw, and the world keeps its quality.
  - ABI 28: `fy_world_quality_set` takes an `FyQuality`;
    `fy_quality_preset` gives a level's. A test checks the Dart levels
    against the engine's.
  - The levels are the same settings as before, so the look scene's
    numbers below hold.

## Measured

The look scene, 60 s at each level, by day and by night, alone, under
`caffeinate`. The numbers are the mean, with the slowest second in
brackets:

| | low | medium | high |
|---|---|---|---|
| Mac (840×1088), day | 117.6 (59.5 in one second) | 119.8 (112) | 119.6 (110) |
| Mac, night | 119.9 (116) | 119.7 (114) | 117.7 (100) |
| iPhone (1320×1309), day | 120 (119) | 120 (120) | 119.9 (119) |
| iPhone, night | 120 (120) | 120 (120) | 120 (120) |
| Note10+ (1080×1068), day | 59.5 (53.9) | 34.4 (26.0) | 27.9 (20.0) |
| Note10+, night | 56.6 (47.9) | 27.2 (22.0) | 29.0 (22.0) |

Sheepdog, level 3 by day, with its HUD and minimap, 60 s at each level
(the Mac's window 1600×1200, the iPhone's screen 1320×2868, the
Note10+'s 1080×2241):

| | low | medium | high |
|---|---|---|---|
| Mac | 120 (120) | 120 (95.2 in one second) | 120 (115) |
| iPhone | 120 (118) | 120 (119) | 120 (76 in one second) |
| Note10+ | 42.9 (28.0) | 22.0 (15.0) | 19.0 (16.0) |

**Sheepdog on the Note10+ is well short, at every level.** In stage 7,
before the look, it held 60. Its runs also swing a lot: two more runs at
low gave 19 and 34 (the second on the code from before the quality
change, so that change didn't cause it). Probes at medium, each between
runs as it is (21 and 22): without shadows 25, without bloom 31, without
ambient occlusion 22 (medium doesn't draw it, so that one is the noise).
Bloom costs most, as in phase 6, but even low, with neither bloom nor the
scene image, doesn't reach 60. Sheepdog draws the scene twice, the field
and the minimap, each with its own shadow maps, and has eight lamps over
the field. It's the same cause as below: on this phone the GPU stays at
its lowest clock. The probe run that crashed last night (every light's
shadows turned off) ran without a crash this time, and a test now does
the same on the Mac.

On the Mac, two of the runs at low printed nothing: the app ran, but its
window never drew a frame. Run directly, and twice more under the tool,
it held 120. A covered window gets no frames on macOS, which fits; it
wasn't seen at the other levels.

The Mac and the iPhone pick high and hold 120, in the look scene and in
Sheepdog. The Note10+ picks medium, and medium doesn't hold 60 there
(below). In the look scene, low holds 60 by day and is a little short by
night.

## The Note10+

Phase 6 found the cause, with GPU timestamps and the GPU's clock read
during the runs:

- Its Mali GPU stays at its lowest clock, 156 of 754 MHz, 72–84 % busy.
  The governor raises the clock only when the GPU is busier than that.
- It isn't busier, because our frames and Flutter's run in series on the
  same GPU. Flutter's small job waits behind ours, so Flutter's frames,
  and Dart's ticks with them, come about 37 times a second. Our render
  thread idles about 20 ms of each 27, waiting for the next scene.
- At that clock each pass has a large fixed cost. Bloom adds about 9 ms
  a frame, and medium's last pass about 3–4 ms. Low fits.

So it isn't a feature that's too slow. It's how frames are paced on
Android. That is its own piece of work: keep the GPU fed, so the
governor raises the clock. Until then, a game for that class of phone
uses `Quality.low`, or turns bloom off. Sheepdog's settings page lets
the player choose.

**Since then** ([the Android frame pacing report](android-pacing-report.md),
2026-10-16): frames are presented by a thread of their own, so the GPU
stays fed and its clock rises. Sheepdog went from about 35 to 51 fps at
low and from 13–26 to 40 at medium, and the look scene at medium from 34
to 43–50. That is still short of 60, so `Quality.device` now picks low on
Android phones.

## The fresh agent

A fresh agent, given only the guide, wrote "Night Market": a square at
dusk with a gradient sky and fog, a sun with shadows by day, four lamps
with glowing bulbs and a spotlight casting shadows by night, a glass
case, a player on the arrow keys, its own quality (medium without
ambient occlusion, one cascade), a cheaper one for old phones, and keys
to switch quality and day or night. Five headless tests, all passing,
analyze clean, in about six minutes. No blockers; every guessed name
compiled the first time. It gave the guide 8 out of 10, and found the
quality design clear: cameras without effects unless given them, and a
game's own quality from `copyWith`.

What it found, all fixed in the guide (and one message):

| Found | Fixed |
|---|---|
| Checking a picture's pixels in a test wasn't shown (§17) | `view.read(view.frame()).rgba`, with a `pixel` helper, and disposing |
| What `Quality(...)` starts from; whether qualities compare | Medium's settings; they compare by value, `name` in `copyWith` |
| The cheap quality's debug message read as an error | It says a cheaper quality chosen on purpose is fine |
| A spotlight casting shadows only in prose | An example, pointing down |
| Letter keys: `KeyCode.q`, not `keyQ` (§20) | Said |
| §1's Material icons note was wrong for this Flutter | Says to check |

## Stage 7's carry-overs

| From stage 7 | Now |
|---|---|
| `uses-material-design` in the guide's "a new game" steps | Done (phase 7, §1) |
| The "is it fair" test pattern | Done (phase 7, §17) |

## What goes on

- **Android frame pacing** (the Note10+ at every level in Sheepdog, and
  at medium and high in the lab). Time-boxed, before stage 9 or as its
  first item. It decides whether the Note10+ class can use medium, and so
  whether `Quality.device` keeps picking medium for Mali and Adreno. A
  game for that class of phone today uses a quality without bloom, and
  maybe without the minimap's second view.
- **Volumes** (global and local, blended as the camera moves, as
  Unity's): bloom and ambient occlusion would move into them, with
  exposure, grading and vignette added. Their own piece of work, after
  animation or wherever you put it. A camera's `bloom:` and
  `ambientOcclusion:` would become shorthand for a global volume.
- **Not in stage 8, as planned:** point lights' shadows (decision 7),
  reflections of the scene itself (only the sky is reflected), and
  timing the GPU on devices (the Mac's timestamps came back stale).
- **Stage 9 is animation:** skinning, clips and an animator.

## Read

Phase 7 is done when you have read this.

- [x] Read (2026-10-16)

Then **stage 8 is done**.
