# Stage 8 plan: the look

Status: accepted 2026-10-14, with the recommended answers to §4. Scope and rules:
[ADR-0001](decisions/0001-fynkel-2-code-first.md). Stage 7 is done
([report](stage-7-report.md)).

A Fynkel game works, runs at the display's rate on every device, and
feels like an app. It doesn't yet look like a game:

- nothing casts a shadow;
- there is one sun, and no lamp or torch;
- glass and water are drawn solid;
- the sky is a flat colour, and shiny things reflect nothing;
- nothing glows.

Stage 8 makes it look good, within the frame budget, on all three
devices.

**The goal.** A scene can have suns, lamps and spotlights that cast soft
shadows, a sky that lights and is reflected by what's under it,
see-through materials, fog, a light ambient occlusion, and bloom on what
glows. Each comes from a few lines of Dart, in the guide, and is tested.
A game picks one of three quality levels (or lets the device pick), and
each level holds its device's frame rate.

**Not in this stage.** Animated characters and animation (skinning,
clips, an animator) are the next stage, on their own. Particles come
with them or after; this stage only makes sure a blended pass exists for
them.

**The road after it.** Stage 9 is animation. Stage 10 is shipping: a
build outside this repository, a licence, versions, and CI that isn't
this Mac. After stage 10, a small real game can be written and shipped
with Fynkel: that is version 1.

## 1. What there is today

- One forward pass, 4× MSAA, reversed-Z, drawn straight into the view's
  sRGB texture. Tone mapping (Khronos PBR Neutral) happens in the same
  shader, so there is no HDR image to post-process.
- glTF's metallic-roughness materials: textures, normal maps, occlusion,
  emission, cut-outs. A glTF "blend" material is drawn solid.
- `Light.sun()` and `Light.ambient()`. Only the first sun counts; the
  others are dropped, with a warning. The ambient light is one flat
  colour, and its specular part uses an analytic approximation, with no
  environment behind it.
- Static glTF models, textures with mipmaps (uncompressed), and
  primitives.
- `FynkelView(resolutionScale:)` already trades sharpness for speed.
- The stage 6 architecture review (R12) said what's missing underneath:
  the renderer is one pass with one sun. More lights, shadows,
  transparency and post-processing all need an ordered list of passes
  over shared frame data, and a list of lights.
- Fynkel 1 had cascaded shadows, AO, bloom, grading, image-based
  lighting and contact shadows (tag `fynkel-1-final`). Following
  ADR-0001's quality rule, nothing is copied as it was. Fynkel 1 is a
  reference to learn from: each piece is written again, small enough to
  read, tested, and measured.

## 2. How each phase is done

As in stage 7: one PR or a few per phase. Each PR adds:

- its lab scene ("the basics" first);
- its guide section;
- its tests, with golden images (`fynkel_golden`) wherever a picture is
  the answer;
- an explanation in plain words.

Two rules hold in every phase:

- **The frame budget.** Each phase is measured on the Mac, the iPhone
  and the Note10+, in the lab's look scene at each quality level. The
  Mac and the iPhone must hold 120 fps and the Note10+ 60 at the level
  its device picks. A feature that can't fit at a level is off at that
  level, and the guide says so.
- **No hidden cost.** A scene that doesn't use a feature doesn't pay for
  it. Phase 1 measures the base cost, and every phase after compares
  against it.

## 3. Phases

### Phase 1: groundwork

The things the rest stands on, changing nothing on screen.

- **Passes.** An ordered list of passes over shared frame data, in
  place of the one pass: the opaque scene; later shadows, the
  see-through scene, and post-processing.
- **An HDR image.** The scene is drawn into a half-float image, and tone
  mapping becomes its own last pass. Bloom and fog need the light values
  before they're squashed to the screen's range.
  - This costs memory bandwidth, the Note10+'s weak point. It is
    measured first, against today's single pass.
  - If it costs too much at the low level, the low level keeps today's
    path: tone mapping in the scene's shader, and no bloom.
- **A light list.** The frame carries every light, not one sun.
- **Quality levels.**
  - `Quality.low`, `medium` and `high`, set on the world
    (`world.quality`) and read by every later phase.
  - Picked by the device when the game doesn't choose: Apple GPUs high,
    Mali and Adreno medium, software or unknown GPUs low.
  - A game can change the level at any time, from its settings page for
    example.

*Done when* every golden image is unchanged, and the cost of the HDR
image and the extra pass is measured on all three devices.

**Done** (2026-10-14):

- **Passes.** The renderer records its passes in order (`renderer.rs`):
  the scene, then the last pass when there is an image to finish.
- **The scene image.** Above low, the scene's 4× MSAA colour is resolved
  into a half-float image (`Rgba16Float`), and a last pass
  (`finish.wgsl`) copies it onto the view. Tone mapping stays in the
  scene's shader, on each sample: then the resolve averages what the
  screen shows, and edges stay exactly as they were. Resolving the light
  values and tone mapping after would change the edges between bright
  and dark surfaces, and the goldens with them. The tone mapping
  (Khronos PBR Neutral) can be undone exactly, so bloom (phase 6) gets
  the light values back in the last pass. Very bright values come back
  less precisely (about 4 % at ten times white, in half floats); phase 6
  says whether that matters for bloom.
- **Low keeps one pass**, straight to the view, as decision 5
  recommended. The measures below don't need it, but low is for GPUs
  nothing is known about; phase 6 decides whether low gets bloom.
- **A light list.** `RenderScene.lights` carries every sun, in id order;
  the renderer still lights with the first. Dart still warns about a
  second sun.
- **Quality levels.** `Quality.low`, `medium`, `high` in the renderer and
  in Dart: `world.quality` (null for the device's pick), `Quality.device`.
  The device picks from its GPU: Apple's and desktops' high, Mali and
  Adreno medium, a software GPU or one not known low (a test covers each).
  ABI 21: `fy_world_quality_set`, `fy_quality_auto`.
- **Every golden is unchanged at every level.** The cube, the debug
  lines, the textured materials (a cut-out, a normal map, a glow) and the
  glTF cut-outs are each checked at all three levels, and a renderer
  switching level between frames.
- **The lab's look scene** ("Look: quality levels"): metal and plastic
  spheres from smooth to rough, a ring of pillars, a glowing cube, a
  turning camera, and buttons for each level and the device's pick.
  `--dart-define=LAB_QUALITY=low` starts it at a level. The guide's §5
  has quality levels, and §18 a line on low for older phones.
- **Measured**, the look scene, 60 s each, alone, under `caffeinate`:

  | | low | the device's level |
  |---|---|---|
  | Mac (840×1088 view) | 120 fps (min 116) | high: 120 fps (min 119) |
  | iPhone (1320×1309 view) | 120 fps (min 108 in one second) | high: 120 fps (min 119) |
  | Note10+ (1080×1068 view) | 60 fps (min 59.9) | medium: 60 fps (min 58.9) |

  No device loses frames to the image and the extra pass. What they cost
  is seen on the Mac's GPU alone (the renderer's bench, a whole frame,
  best of 50): **+0.13 to +0.16 ms a frame** at an iPhone Pro Max's
  2796×1290, the same with 1 cube or 10,000, of the 8.3 ms a 120 Hz
  frame has. At 1280×720 it's lost in the noise.
- **Not done: timing the GPU on devices.** Timestamps around the passes
  would give each phase a GPU time on the phones. Tried: on the Mac
  (Metal, wgpu 30) the end-of-pass timestamp came back stale, before the
  begin, so it was taken out rather than kept giving wrong numbers. The
  phones are measured by their frame rates, as before.

### Phase 2: lights

- `Light.point(range:)` and `Light.spot(range:, angle:, softness:)`,
  and more than one sun, all in the same physical units as today's sun.
- Many lights in a frame, a few per object: each object gets the ones
  nearest to it, up to a cap set by the quality level (for example 4 / 8
  / 8). The frame's own cap is a fixed number with a warning past it
  (Fynkel 1 used 64).
- The lab: a dark room with coloured lamps and a torch that follows the
  pointer. Sheepdog gets lamps at night.

*Done when* 32 lights hold the budget at every level, and an object lit
by more than its cap is lit by the nearest ones, with a test.

**Done** (2026-10-14), except the Note10+ at medium (below):

- **Lamps and spotlights.** `Light.point(range:)` and `Light.spot(range:,
  angle:, softness:)` in Dart, `LightKind.point` and `.spot`; `range`,
  `angle` and `softness` can be read and set, and bad values are refused
  (a negative range, an angle of 180° or more, a softness outside 0–1).
  ABI 22: `FyLight` has `range`, `spot_angle` (degrees, as the
  Inspector-style fields elsewhere) and `softness`.
- **The sun's units.** A lamp of intensity 1, 1 m from a surface facing
  it, lights it as a sun of 1 does; it falls off with the square of the
  distance and fades smoothly to nothing at its range. A spot's cone
  fades over its outer `softness`. Tests check each against the sun.
- **Every sun lights**, up to 4 in a frame, in id order. The Dart warning
  about a second sun is gone; a fifth is warned about, once.
- **Many lights in a frame.** The frame takes the lights in view (each
  lamp's reach tested against the view), nearest the camera first, up to
  64; Dart warns once past 64 lamps in the world, and `FrameStats` counts
  the lights drawn and the ones left out. Lights that are off (colour or
  intensity 0) take no place.
- **A few per object**: 4 at low and medium, 8 at high. *Changed from
  the plan:* an object gets the lights **brightest at it**, not simply
  the nearest. Among lights alike that is the nearest ones (the test the
  plan asks for: of equal lamps, the nearer win), but a bright torch
  from across the room now beats a dim lamp close by, and a spotlight
  turned away takes no place. Ranking by distance alone hid the lab's
  torch behind the coloured lamps. Medium was planned at 8; it takes 4,
  as the Note10+ measures below show it can't afford more.
- **The lab** ("Look: lamps"): a dark room of 1 m tiles, 31 coloured
  lamps drifting about with glowing bulbs, metal and plastic blocks and
  balls, and a torch that follows the pointer. Buttons for 8, 32 and 64
  lights, each level, and stopping the lamps. Tiles because each object
  gets its own few lights: one big floor would be lit only by the lamps
  brightest at it as a whole. The guide (§5 "Lamps and spotlights") says
  so.
- **Sheepdog** has four lamp posts, lit at night and dark by day.
- **Measured**, the lamps scene with 32 lights, 60 s each, alone, under
  `caffeinate`:

  | | low | medium | high |
  |---|---|---|---|
  | Mac (840×1088) | 120 fps (min 117) | 120 (min 119) | 120 (min 119) |
  | iPhone (1320×1309) | 120 (min 119) | 120 (min 119) | 120 (min 118) |
  | Note10+ (1080×1068) | 59.3 (min 56.9) | **56.4 (min 42)** | 51.8 (min 35) |

  Low, high, the Mac and the iPhone were measured before the lamp
  shading was made cheaper; it can only have helped them.

  On the renderer's bench (Mac, 32 lamps), choosing the lights adds about
  0.06 ms a frame at 1,000 objects and 0.5 ms at 10,000.
- **Not done: the Note10+ at medium**, its own level. It averages about
  56 fps with lamps, against 60 without; its ticks stay at 60, so it's the
  GPU. What was tried, each a 60 s run:

  | medium on the Note10+ | fps (min) |
  |---|---|
  | 32 lights, 8 per object (first try) | 52.2 (39) |
  | 4 per object | 51.4 |
  | 4 per object, cheaper lamp shading | 56.4 (42) |
  | as above, an 8-bit image instead of half floats | 56.2 (47) |
  | as above, 2 per object | 56.7 (49) |
  | 16 lights, 4 per object | 56.2 (46) |
  | no lamps | 59.9 (58.9) |

  So it isn't the number of lamps, nor how many light a pixel, nor the
  image's memory: lamps cost the Mali something fixed on the medium
  path that they don't on low's single pass, not yet found. Phase 6
  reworks the last pass for bloom and has to fit the same phone, so it
  picks this up; until then a game for the Note10+ class with lamps can
  set `world.quality = Quality.low`.

### Phase 3: shadows

- **The sun's shadows.** `Light.sun(shadows: true)`.
  - Cascaded shadow maps, so near shadows are sharp and far ones still
    exist.
  - Cascades, map size and filtering go by level: for example one
    cascade with hard edges at low, two with soft edges at medium, three
    with soft edges at high.
  - The cascades are fitted to the camera each frame and snapped to
    texels, so shadows don't shimmer as the camera moves.
- **Spotlights' shadows**, where they fit the budget: one map each, from
  a small shared atlas.
- **A point light's shadows: not in this stage.** They need six maps
  each.
- `MeshRenderer(castShadows:, receiveShadows:)`, as in Unity.

*Done when* the lab shows a sun and a spotlight casting shadows that
don't shimmer while the camera moves (a test checks it), and the budget
holds at each level.

**Done** (2026-10-14), except the Note10+ at medium and high (below):

- **The sun's shadows.** `Light.sun(shadows: true)`, or `light.shadows`
  at any time; the first sun with shadows casts them. Cascades fitted to
  the slice of view each covers (a sphere around it, so turning the
  camera doesn't resize them), their centres snapped to whole texels,
  and stretched back towards the sun to hold casters outside the view.
  They reach the level's distance from the camera (or the camera's
  `far`, if nearer) and fade out over the last tenth.

  | | low | medium | high |
  |---|---|---|---|
  | cascades | 1 at 1024² | 2 at 2048² | 3 at 2048² |
  | filter | 1 tap (smoothed across a texel) | 4 taps | 9 taps |
  | spot maps | 1 at 512² | 2 at 1024² | 4 at 1024² |
  | distance | 40 m | 60 m | 100 m |

- **No shimmer**: a unit test moves and turns the camera over 40 frames
  and checks that a still point keeps its place within its texel in
  every cascade, and that no cascade changes size. With the snapping
  taken out, it fails.
- **Spotlights' shadows**: `Light.spot(shadows: true)`; the level's
  number of spot maps go to the spotlights with shadows nearest the
  camera. *Changed from the plan:* the maps are layers of one texture
  array, not tiles of an atlas: the same memory, and no tile edges to
  clamp lookups to.
- **Point lights** cast none (this stage, as planned); one asked to is
  ignored.
- **`castShadows` and `receiveShadows`** on `MeshRenderer`, true unless
  set, as in Unity. *Changed from the plan:* they're properties
  (`MeshRenderer(mesh, material)..castShadows = false`), not constructor
  arguments, since Dart doesn't allow named arguments after the optional
  positional ones it already has.
- **Acne and peter-panning**: depth bias in the shadow pass, and each
  lookup moved out along the surface's own normal by about a texel, more
  where the light grazes it.
- ABI 23: `FyLight.shadows`, `FyMeshRenderer.cast_shadows` and
  `receive_shadows`. `FrameStats` counts the shadow maps drawn and the
  casters drawn into them.
- **The lab** ("Look: shadows"): a garden under a low sun (pillars, an
  arch, a picket fence, trees, an avenue running off 90 m) and a lamp
  post's spotlight, with the camera drifting. Switches for each light's
  shadows and for night, buttons for each level. `LAB_SHADOWS=false`
  starts it without shadows, for measuring.
- **Sheepdog**'s sun (and moon) casts shadows; its field and the bark's
  ring cast none.
- **The guide**: §5 "Shadows".
- **Measured**, the shadows scene, 60 s each, alone, under `caffeinate`:

  | | low | medium | high |
  |---|---|---|---|
  | Mac (840×1088) | 60 fps (min 59) | 60 (min 60) | 60 (min 60) |
  | iPhone (1320×1309) | 120 (min 119) | 120 (min 119) | 120 (min 120) |
  | Note10+ (1080×1068) | 59.8 (min 55.9) | **55.2 (min 45.9)** | **51.5 (min 37.9)** |
  | Note10+, shadows off | 59.9 (min 58.9) | 58.0 (min 50.9) | 58.1 (min 52.9) |

  The Mac is now on a 60 Hz display, which caps every scene there at 60
  (phase 2's lamps scene too), so its budget is from the renderer's
  bench instead (M3 Max, a whole frame, best of 50, 1280×720): the sun's
  shadows over 1,000 cubes take a frame from 0.58 to 0.86 ms at low,
  1.03 at medium and 1.02 at high; over 10,000, from 1.3 to 1.9–2.1 ms.
  Far inside 8.3 ms.
- **Not done: the Note10+ at medium and high.** At medium the scene is
  already under 60 without shadows (58.0: phase 2's open medium-path
  cost), and shadows take about 3 fps more. What was tried at medium,
  each a 60 s run:

  | medium on the Note10+ | fps (min) |
  |---|---|
  | 2 cascades at 2048², 4 taps (the level) | 55.2 (45.9) |
  | 2 cascades at 1024² | 54.9 (43.9) |
  | 1 tap | 54.6 (44.9) |
  | 1 cascade | 55.9 (43.9) |
  | no shadows | 58.0 (50.9) |

  Neither the maps' size, nor the filter, nor the number of cascades
  moves it, and its game ticks hold 60: the cost is fixed, on the medium
  path, like phase 2's lamps. Phase 6 reworks that path and takes both
  up. High was never meant for the Note10+; it's listed for the record.
  A game for that class of phone that wants shadows today can use low,
  which holds 60.

### Phase 4: the sky and what reflects it

- `world.sky`: one of
  - a gradient (horizon and zenith colours, which a game can take from
    its theme);
  - a plain colour, as today;
  - an HDR image (equirectangular `.hdr`), from CC0 sources such as Poly
    Haven, committed with credits.
- **The sky lights the scene.** Its blurred copy (prefiltered on the GPU
  when it's set, not each frame) becomes the ambient light and the
  reflections. So a shiny ball shows the sky, and a rough one only its
  colour.
- **Fog**: by distance and height, in the scene's own shader, so it
  costs almost nothing. `world.fog = Fog(color:, start:, end:)`, or the
  sky's horizon colour.

*Done when* a metal sphere reflects an HDR sky, a gradient sky changes
with the app's theme (day and night, as in Sheepdog), and fog fades far
objects into the sky.

### Phase 5: see-through

- Materials that blend: `Material(opacity:)`, and glTF's "blend"
  materials as they're meant to be.
- They are drawn after everything solid, sorted back to front by
  object. Within one object the order is left as it is: the guide says
  to split an object if that matters.
- They receive light and fog; they cast no shadow in this stage.
- Additive blending for glowing effects, ready for particles later.

*Done when* glass and water read right in front of and behind other
objects, with a golden test of the order.

### Phase 6: bloom, and a light AO

- **Bloom.** What's brighter than white glows: emissive materials, the
  sun on metal. It works on a chain of smaller and smaller images, which
  is cheap. It is on at medium and high; at low it is off, or at a
  quarter of the screen's size if the measures allow.
  `Camera(bloom: Bloom(strength:, threshold:))`.
- **Ambient occlusion, light.** Darkening where surfaces meet: corners,
  under objects, in folds.
  - Kept cheap on purpose: it is worked out at half (or quarter) size
    from the depth alone, with few samples, and blurred.
  - On at high, off at low and medium unless measures say medium can
    afford it.
  - The goal is a soft hint of contact, not Fynkel 1's full GTAO.
- Both are per camera, so a minimap can skip them.

*Done when* both hold the budget at the levels where they're on, and
cost nothing where they're off (measured).

### Phase 7: the look scene, Sheepdog, and the report

- **A lab "look" scene** that uses everything: a sun with shadows, a
  sky, lamps, a spotlight, glass, fog, glow and AO, with a quality
  switch. It is the scene every phase is measured in, and grows phase by
  phase.
- **Sheepdog gets the look.**
  - Shadows under the sheep and the fences.
  - A gradient sky from the theme, and fog at the field's edges.
  - At night, lamps along the fence, and the wolves' eyes glowing.
  - A quality setting on its settings page.
- **Measured** on the Mac, the iPhone and the Note10+, at each level.
- **A fresh agent** writes a small game from the guide alone, asking for
  a scene with shadows, lamps and a sky, to check the guide.
- **The stage 8 report.**

## 4. Decisions

Each has a recommendation. Accepting the plan accepts them, unless you
say otherwise.

1. **Animation is stage 9, and shipping stage 10.** *Decided by you,
   2026-10-14.*
2. **Bloom is in. Fog and AO are in if they're light.** Fog is in the
   scene's shader and nearly free. AO is cheap and partial, on at high
   only by default. *Decided by you, 2026-10-14.*
3. **Three quality levels**, picked by the device unless the game
   chooses. *Decided by you, 2026-10-14.*
4. **The final scene** is Sheepdog with the look, plus the lab's look
   scene, not a new showcase. *Recommended*: Sheepdog's night already
   asks for lamps, and a real game shows more than a demo would.
5. **The HDR image costs the low level its bloom**, if phase 1's
   measures say the Note10+ can't afford the extra pass. The low level
   would then keep today's single pass. *Recommended.*
6. **Lights are forward, with a per-object cap**, not clustered. A
   game's scenes have tens of lights, not thousands, and per-object
   lists are simple and cheap on phones. *Recommended*; clustering can
   come later if a game needs hundreds.
7. **Point lights cast no shadows in this stage.** *Recommended*: each
   needs six shadow maps, and a game can use a spotlight where a shadow
   matters.

## 5. Checklist

- [x] Phase 1: groundwork
- [x] Phase 2: lights (the Note10+ at medium left to phase 6)
- [x] Phase 3: shadows (the Note10+ at medium left to phase 6)
- [ ] Phase 4: the sky and what reflects it
- [ ] Phase 5: see-through
- [ ] Phase 6: bloom, and a light AO
- [ ] Phase 7: the look scene, Sheepdog, and the report
