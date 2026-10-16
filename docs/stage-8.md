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
- **The lab's look scene** ("Look: quality levels", grown into "Look: everything" in phase 7): metal and plastic
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

  After hardening phase 4 (2026-10-14), the same scene at medium: 56.9
  (min 37.9). Still open.

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

  After hardening phase 4 (2026-10-14), at medium: 57.9 (min 39.9) with
  shadows, 59.9 (min 58.9) without. Phase 2's hardening had cost both
  about 9 fps (V10's GPU polling); undone. Still open at 57.9.

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

**Done** (2026-10-15):

- **`world.sky`**: `Sky.gradient(zenith:, horizon:, ground:)`,
  `Sky.color(c)` or `Sky.image(skyImage, intensity:, rotation:)`, each
  with `lighting` (what its light is multiplied by; 0 for none). A
  gradient's colours are shown as picked, not tone mapped, so a theme's
  colours come out as they are; an image is light, tone mapped as the
  scene is. `Camera.showSky = false` shows the clear colour instead (a
  minimap), while the sky still lights.
- **`.hdr` images**: `SkyImage.load(bytes)` decodes on a helper isolate,
  or `assets.skyImage('sky.hdr')` (and `preload` by `.hdr`).
  fynkel_load decodes Radiance RGBE into floats (the `image` crate's
  `hdr` feature); the GPU keeps it as RGB9E5, 4 bytes a pixel. The lab's
  is Poly Haven's "Kloofendal 48d Partly Cloudy (Pure Sky)", 1k, CC0,
  credited in `examples/lab/assets/CREDITS.md`.
- **The sky lights the scene.** When a gradient or an image is set, the
  GPU works out three cubes once: the sky itself (128² with mips), a
  copy blurred for each roughness (64², 6 mips, GGX importance samples
  as Karis's split sum) and the light each way gets (16², cosine
  weighted). The forward shader reads the blurred copy along the
  reflection at the surface's roughness and the light cube along its
  normal, so a mirror ball shows the clouds and a rough one only the
  sky's colour, lit from above by the zenith and below by the ground.
  The cubes kept take 0.27 MB of GPU memory, and an image 4 bytes a
  pixel more (2 MB for a 1k one): `ViewStats.skyBytes`. A plain colour
  makes no cubes: it adds to the ambient light. *Changed from the plan:*
  the scene doesn't hide the sky from itself (no occlusion), so a ball
  under a roof still mirrors the sky; reflections of the scene are for
  later.
- **The sky behind everything**: one triangle at the far plane, drawn
  after the opaque objects where nothing was.
- **Fog**: `world.fog = Fog(color:, start:, end:, height:)`, by distance
  from the camera between `start` and `end`, thinning upwards when
  `height` is given; without a colour it's the sky's horizon (an image's
  is the average of its rows within 10° of level). In the forward
  shader, after tone mapping, so it costs a few instructions a pixel.
- ABI 25: `fy_sky_image_decode`, `_free` and `_size`,
  `fy_world_sky_set` (`FySky`), `fy_world_fog_set` (`FyFog`),
  `FyCamera.show_sky` and `FyViewStats.sky_bytes`. Steady frames under a
  sky in fog still allocate nothing, on the calling thread and in
  `prepare`.
- **Tests**: the renderer's (`tests/sky.rs`): a gradient's colours seen
  as picked straight up, level and down; a plain sky lighting as ambient
  light does, and a gradient or image of one colour the same; a rough
  ball blue on top and red below under a blue sky over red ground; a
  mirror ball showing the sky's colours where a rough one shows their
  mix; an image seen turned and scaled; fog fading a wall by distance,
  into its colour or the horizon. Then the same through the C interface
  and in Dart, and the lab page driven as a player would.
- **The lab** ("Look: sky and fog"): metal balls from a mirror to fully
  rough, plastic ones in front, and an avenue of pillars 100 m long,
  under the Poly Haven sky, a gradient day or night, a plain colour or
  none, with fog. The camera swings, so reflections move over the balls.
  `LAB_SKY` and `LAB_FOG` start it otherwise, for measuring.
- **Sheepdog**'s sky is a gradient of its theme's colours, a day in the
  light theme and a night in the dark one, lighting the field; the far
  field fades into its horizon. Its minimap shows the ground's colour.
- **The guide**: §5 "The sky and fog".
- **Measured**, the sky scene under the Poly Haven image with fog, 60 s
  each, alone, under `caffeinate`:

  | | low | medium | high |
  |---|---|---|---|
  | iPhone (1320×1309) | 118.8 fps (min 82) | 120 (min 120) | 120 (min 119) |
  | Note10+ (1080×1068) | 59.8 (min 52.9) | 59.2 (min 54.9) | 55.9 (min 50.9) |
  | Note10+, no sky, no fog | | 59.9 (min 58.9) | |

  The iPhone's low run held 120 but for one second at 82 and a few at
  111–117 near its end, over the wireless link; medium and high, which
  draw more, held 120 throughout. On the Note10+ the sky and fog cost
  medium about 0.7 fps, inside its 60. High isn't the Note10+'s level
  (phases 2 and 3 have it there for the record).

  The Mac is on a 60 Hz display, so from the renderer's bench (M3 Max,
  1280×720, best of 50): a sky that lights, with fog, adds 0.0–0.02 ms to
  a frame of 1,000 cubes (0.37–0.41 → 0.38–0.39 ms) and nothing
  measurable at 10,000. **Setting a new sky costs about 6 ms, once**
  (5.9–6.5 ms for the frame that works out its light, a gradient or a
  1k image alike), on the GPU. On a phone that's several frames: a game
  changes its sky between scenes or behind a fade, not every frame. A
  sky's light could later be worked out over a few frames if a game
  needs to change it live.

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

**Done** (2026-10-15):

- **`Material(opacity:, blending:)`**: `opacity` is the colour's alpha;
  made below 1, a material is `Blending.transparent` unless `blending`
  says otherwise. Setting `opacity` later changes the alpha alone.
  *Changed from the plan:* Dart calls it `Blending`, as `Blend` is
  already the camera rigs' blend and Flutter has `BlendMode`; in Rust
  and the C interface it's `blend`.
- **Transparent** is laid over what's behind as glTF's BLEND says (the
  "over" operator): everything the surface shows, its light, highlights,
  glow and fog, times its alpha, plus what's behind times one minus it.
  The shader returns its colour premultiplied, so the blend is one
  multiply-add, and a cutoff still drops what's below it. A model's
  BLEND materials load as this; the warning that they're drawn opaque is
  gone, and the Khronos AlphaBlendModeTest golden now shows the wall
  through its blend panel.
- **Additive** adds the surface's light times its alpha (`ONE, ONE`),
  fading to nothing in fog, for glows now and particles later.
- **Order**: the solid draws are batched and sorted as before; the
  blended ones go in a list of their own, sorted furthest first by the
  depth of their bounds' centre along the view (ties keep the scene's
  order, so equal ones don't flicker), and drawn after the solid scene
  and the sky. Runs of the same pipeline, textures and mesh are still
  one instanced call: instances are drawn in order, so a run of 400
  panes is one call and still back to front. They test the depth but
  don't write it, and alpha-to-coverage is off for them.
- **Shadows**: blended draws aren't casters; they receive shadows and
  every light, and fog.
- **The pipelines**: `Cover {Opaque, Cutout, Transparent, Additive}`
  replaces the cut-out flag in a pipeline's variant (96 now, still made
  only when first used); the two new ones have their own fragment
  entries, `fs_transparent` and `fs_additive`.
- ABI 26: `FyMaterial.blend` (`FY_BLEND_OPAQUE`, `_TRANSPARENT`,
  `_ADDITIVE`); another number is refused with "N isn't a kind of
  blend". fynkel_world's `Material.blend` and fynkel_load's
  `MaterialData.transparent`. Steady frames with see-through draws
  still allocate nothing in `prepare` (the alloc test's busy scene has
  them now).
- **Tests**: the renderer's (`tests/blend.rs`): glass in front of a wall
  shows half of each and behind the wall is hidden, at every level; three
  half see-through squares given in three orders come out the same,
  furthest first (the golden `transparent_order`); five of one kind are
  one call; glass over the sky; additive glows adding up; a cutoff on a
  see-through surface; thick fog hiding glass and glow alike; and no
  shadow from a see-through or additive pane where an opaque one casts
  one. Then the C interface, the loader, and Dart (`blend_test.dart`),
  and the lab page driven as a player would.
- **The lab** ("Look: glass, water and glows"): three tinted panes
  turning among a ring of cubes, so each is in front of the glass and
  then behind it; balls half in a pool of water; additive sparks
  circling over it; the camera swinging round. A slider sets the glass's
  opacity, and a switch adds a crowd of 400 see-through panes
  (`LAB_CROWD` starts with it, for measuring).
- **The guide**: §5 "See-through: glass, water and glows", the Unity
  table, §19 (glass doesn't bend light; see-through objects cast no
  shadow), and §21 (BLEND materials are see-through now).
- **Measured**, the glass page, 60 s each, alone, under `caffeinate`:

  | | low | medium | high | medium, 400 more panes |
  |---|---|---|---|---|
  | iPhone (1320×1309) | 119.9 fps (min 115) | 120 (min 120) | 120 (min 119) | 120 (min 120) |
  | Note10+ (1080×1068) | 59.8 (min 56.9) | 56.7 (min 39.9) | 53.5 (min 40.9) | 48.7 (min 35.9) |

  The iPhone holds 120 everywhere, the crowd included. The Note10+
  holds 60 at low; at medium it's the shortfall phases 2 and 3 left to
  phase 6, and see-through surfaces add to it, as each pixel under them
  is drawn once per layer: the crowd of 400 overlapping panes costs it
  about 8 fps more. Left to phase 6 with the rest.

  From the renderer's bench (M3 Max, best of 50), every cube see-through
  with the sun's shadows: `prepare` 0.04 ms for 1,000 cubes (as opaque)
  and 0.46–0.48 ms for 10,000 (0.26–0.38 opaque), the sort by distance;
  a whole frame of 10,000 1.2–1.9 ms against 1.1–1.2.

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

**Done** (2026-10-15), with the Note10+ short of the budget (below):

- **`Camera(bloom: Bloom(strength:, threshold:), ambientOcclusion:
  AmbientOcclusion(intensity:, radius:))`**, both on unless set to
  null. A minimap or second view turns them off and costs nothing.
  Sheepdog's minimap does. ABI 27: six `FyCamera` fields; fynkel_world's
  Camera fields 8–13.
- **Bloom** (`bloom.rs`, `bloom.wgsl`): at medium and high, off at low.
  - The scene image's alpha now says how much of a pixel is light.
    Colours shown as picked (the clear colour, a gradient sky) don't
    glow however bright; an image sky's sun does.
  - A prefilter takes what's over the threshold (eased in over half of
    it, weighed down where bright so a lone pixel doesn't flicker) into
    a chain of five images. The first is a quarter of the view's size,
    each half the one before. Then 13-tap downs and tent ups add each
    back into the larger.
  - The last pass adds the glow to the light and tone maps again. It
    adds the change, so a pixel with no glow is exactly as it was.
  - The chain is `Rg11b10Ufloat` where the GPU can draw into it,
    half-float otherwise. *Changed from the plan:* it was to start at
    half size; on the Note10+ that cost 30–36 fps at medium against 40
    now.
- **Ambient occlusion** (`ao.rs`, `ao.wgsl`): at high only.
  - The solid scene's depth again, at half size.
  - From it, 8 samples in a spiral per pixel (as McGuire's Scalable
    Ambient Obscurance), the normal taken from the nearer neighbours so
    edges don't bend it.
  - A 4 × 4 blur across pixels at about the same depth.
  - Each 4 × 4 of pixels turns its spirals all 16 ways once (a Bayer
    matrix), so the blur leaves it even. Random turns left blotches
    along a wall's foot (97..119 against 2/255 now).
  - The forward pass darkens only the ambient and sky light by it, and
    only for solid surfaces.
- **Tests**: the renderer's `tests/bloom.rs`, 6 tests: what glows and
  where; nothing at low; colours shown as picked; view sizes; the
  golden `bloom`. And `tests/ambient_occlusion.rs`, 9 tests: the floor
  at a box's foot; nothing below high; an open floor and a lone box
  unchanged; lights untouched; view sizes; nothing solid; smooth along a
  wall; the golden `ambient_occlusion`. Then the C interface, the
  world, Dart (`camera_look_test.dart`), Sheepdog's minimap, and the lab
  page driven as a player would (`glow_test.dart`).
- **The lab** ("Look: glow and contact"): lamp bulbs and a turning ring
  of glowing blocks, metal balls in the sun, crates against a wall. It
  has switches and sliders for both, and `LAB_BLOOM` / `LAB_AO` for
  measuring.
- **The guide**: §5 "Glow and contact", the Unity table, §21.
- **Measured**, the glow page, 60 s each, alone, under `caffeinate`, on
  and off:

  | | low | medium | high |
  |---|---|---|---|
  | iPhone (1320×1309), on | 119.8 (min 110) | 120 (min 120) | 120 (min 118) |
  | iPhone, off | 119.9 (min 116) | 120 (min 120) | 120 (min 120) |
  | Note10+ (1080×1068), on | 59.9 (min 59.9) | 40.4 (min 32.0) | 27.2 (min 24.0) |
  | Note10+, off | 59.9 (min 59.9) | 58.5 (min 50.9) | 55.5 (min 50.9) |

  The iPhone holds 120 at every level, with them and without.

  On the Mac (M3 Max, best of 50, 2796×1290, 1,000 cubes with the
  sun's shadows), bloom adds about 0.5 ms at medium, and bloom and AO
  0.4–0.7 ms at high. At low both cost nothing.

- **The Note10+ doesn't hold the budget** at medium or high with them
  on. That's accepted (your call, 2026-10-15) and left as found. What
  was found, with GPU timestamps and the GPU's clock read during the
  runs:
  - Its GPU runs at its lowest clock, 156 of 754 MHz, 72–84 % busy.
    The governor raises it only when the GPU is busier than that.
  - It isn't, because frames are serial. Flutter draws on the same GPU,
    and its small job waits behind ours. So Flutter's frames, and
    Dart's ticks with them, come at about 37 a second. The render
    thread idles about 20 ms of each 27 waiting for the next scene, and
    the GPU idles with it.
  - At that clock each pass has a large fixed cost:

    | pass | ms |
    |---|---|
    | the last pass, alone | 3–4 |
    | bloom's prefilter | 3.8 |
    | each small pass | about 0.7 |
    | all of AO | about 3 |

    A whole frame at medium is 13.6 ms without bloom and 22.6 with it;
    at low, 7.2.
  - This is also the medium shortfall phases 2–5 left here: at low the
    whole frame fits even at the lowest clock.
  - Not waiting for the GPU before presenting didn't raise the clock.
    Keeping the GPU fed would mean pacing frames differently on
    Android, a piece of work of its own.

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
   chooses. *Decided by you, 2026-10-14.* **Changed in phase 7
   (2026-10-16), your call:** a quality is settings a game can change
   (bloom, ambient occlusion, HDR, lamp highlights, shadows on or off,
   cascades, resolutions, softness, distance), as Unity's quality levels
   are; the three levels are where a game starts from. Cameras have no
   bloom or ambient occlusion unless given them, and a debug build says
   when the quality leaves out what a camera asks for. ABI 28. Volumes
   (global and local, blended) come later, as their own work.
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
- [x] Phase 4: the sky and what reflects it
- [x] Phase 5: see-through (the Note10+ at medium left to phase 6)
- [x] Phase 6: bloom, and a light AO (the Note10+ short at medium and high, accepted)
- [ ] Phase 7: the look scene, Sheepdog, and the report
