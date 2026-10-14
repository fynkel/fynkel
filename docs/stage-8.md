# Stage 8 plan: the look

Status: proposed 2026-10-14. Scope and rules:
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

- [ ] Phase 1: groundwork
- [ ] Phase 2: lights
- [ ] Phase 3: shadows
- [ ] Phase 4: the sky and what reflects it
- [ ] Phase 5: see-through
- [ ] Phase 6: bloom, and a light AO
- [ ] Phase 7: the look scene, Sheepdog, and the report
