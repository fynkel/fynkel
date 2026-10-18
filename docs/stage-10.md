# Stage 10 plan: shaders

Status: accepted 2026-10-18 ("let's go"), with the recommended answers to §4. Scope and rules:
[ADR-0001](decisions/0001-fynkel-2-code-first.md). Stage 9 is done
([report](stage-9-report.md)).

Every Fynkel object today is drawn by one shader: the engine's
physically based one, with a colour, textures, metal and roughness, a
glow, and see-through or additive blending. That is enough for things
that look like things. It isn't enough for a game with its own look:
water that moves, lava that flows, a ghost made of light, an enemy that
flashes when hit or burns away when it dies, grass in the wind, or a
whole game drawn as a cartoon or in big pixels.

**The goal.** A game writes its own shaders in a few lines, and they are
lit, shadowed, skinned and fogged like everything else:

```dart
final lava = Material(
  shader: await assets.shader('lava.wgsl'),
  values: {'flow': 0.4, 'hot': Color(0xFFFF6A00)},
);
floor.get<MeshRenderer>()!.material = lava;
enemy.get<MeshRenderer>()!.set('flash', 1); // this enemy only
```

```wgsl
// assets/lava.wgsl
struct Values { flow: f32, hot: vec4f }

fn surface(in: SurfaceInput) -> Surface {
  var s = standard(in);
  let n = noise(in.uv * 4.0 + vec2f(0.0, frame_time() * values.flow));
  s.emission = values.hot.rgb * smoothstep(0.4, 0.8, n) * 3.0;
  return s;
}
```

All three kinds the road asks for (agreed 2026-10-17):

1. **Surfaces**: water, lava, holograms, dissolving, a hit flash. The
   shader says what a point of the surface is (its colour, how rough,
   how bright); the engine lights it.
2. **Movement**: wind, waves, wobble. The shader moves vertices, after
   stage 9's skinning, and the shadow moves with them.
3. **Whole styles**: cartoon (toon) lighting, outlines, a pixel-art look,
   and effects over the whole screen. More than one material: the
   lighting itself, and the picture after it's drawn.

**The road after it**: stage 11 sound, stage 12 shipping, 1.0. Animation
2 (2D blends, root motion, IK, face shapes, retargeting) is agreed for
after this stage.

## 1. What there is today

- **One lit shader** (`crates/fynkel_render/src/forward.wgsl`), joined to
  `skin.wgsl` with `concat!`. It's specialised by WGSL override
  constants (double-sided, shadowed, textured, sky-lit, AO), into 384
  pipelines made when first drawn and kept on disk.
- **The shadow pass** (`shadow.wgsl`) and **AO's depth** (`ao_depth.wgsl`)
  have their own vertex stages. A shader that moves vertices must move
  them in all three, or shadows and AO stay where the mesh was.
- **Stage 9's door**: each vertex stage is "skin, then the rest"
  (`vertex(in, model_rows)`), so a shader's own vertex code goes after
  the skin.
- **The frame's numbers** (`Frame`): the camera, 4 suns, 64 lamps in
  clusters, shadows, sky, fog. **No time**: a shader can't move on its own
  yet.
- **Bind groups 0–3 are all used**; group 1 is the material's five
  textures. Vertex locations 11–13 are free (14–15 went to skinning).
- **The picture after the scene**: bloom, then the "finish" pass (a copy,
  or bloom's glow added). Medium and high draw into a scene image first.
  Low draws straight to the screen, so there's no image to work on. The
  scene's depth is never kept.
- **Materials from Dart** (`Material`, `fy_material_*`): the glTF fields,
  `Blending`, and `MeshRenderer.color` for one object's tint. No custom
  values.

What Fynkel 1 taught (its ADR-0031, shader assets, read again for this
plan):

- **The surface function was right.** A `.shader` file's
  `fn surface(SurfaceInput) -> Surface`, with the engine writing the
  rest, worked, and a copy of the built-in look written as a shader
  matched it. The contract was versioned (`surface@1`).
- **The cost was around it**: its own file format with a header, a
  cook step, an editor page, generated layouts. In Fynkel 2 the file is
  plain WGSL, and the values are found in it.
- **Moving vertices broke culling.** A vertex moved outside the mesh's
  bounds was cut off at the screen's edge. A shader that moves things
  must say how far.
- **Time had to be added** to the frame. It's missing here too.

## 2. How each phase is done

As in stages 8 and 9: one PR or a few per phase, each with its lab
scene, its guide section, its tests (golden images where a picture is
the answer), and plain words.

- **The frame budget.** The Mac and the iPhone hold 60 with a scene
  full of custom shaders (water, a field of grass in the wind, toon
  characters, a screen effect), and 120 when asked; the Note10+ holds 60
  at low.
- **No hidden cost.** A world with no custom shader draws exactly as
  today, with the same pipelines and the same goldens.
- **Errors are found early and said plainly.** A shader that doesn't
  compile fails to load with its file, line and the message, like a
  missing asset. On a hot reload, the old shader stays and the error is
  printed. A game's tests can check its shaders without a GPU.

## 3. Phases

### Phase 1: surfaces

- **Time** in the frame: `frame_time()`, `frame_delta()`, following
  the game's `time` (paused when the game pauses, slowed with `time.scale`).
- **`assets.shader('water.wgsl')`**: a WGSL file in the game's assets,
  with `fn surface(in: SurfaceInput) -> Surface`. The engine adds the
  rest: the lighting, shadows, fog, skinning, every quality level. The
  shader is checked (naga, already in wgpu) as it loads.
- **`SurfaceInput`**: the uv, the world position and normal, the view
  direction, the screen position, the material's standard values. A
  shader can start from the standard look (`standard(in)`) and change
  one thing.
- **`Surface`**: colour, alpha, normal, metallic, roughness, emission,
  occlusion. Or **unlit** (`fn unlit(...)`), for holograms and things
  that shine on their own.
- **Values** from the shader's own `struct Values`, found by name:
  numbers, vectors, colours, and textures (up to 4). Set from Dart as
  `Material(shader:, values:)` and `material.set('flow', 0.6)`. The types
  are checked: a colour for a `vec4f`, a texture for a `texture_2d`.
- **One object's values**: up to four numbers per object, named in the
  shader (`struct PerObject { flash: f32, burn: f32 }`), set on the object's
  `MeshRenderer` (`set('flash', 1)`) and tweenable. All the enemies share one
  material, and each flashes on its own (decision 4).
- **A small library**: noise, hashes, fresnel, `remap`, triplanar
  sampling, so common effects don't start from nothing.
- **Hot reload** re-reads a changed shader.
- **Shown with**, in the lab: a hit flash, a dissolve (with an edge that
  glows, and the shadow dissolving too), lava, a hologram.

*Done when* a shader that copies the standard look matches the built-in
one's goldens at every quality level; each lab shader has its golden; a
broken shader's error names its line; and a world without custom shaders
gives today's goldens unchanged.

### Phase 2: movement

- **`fn vertex(in: VertexInput) -> VertexInput`** in the same file:
  moves the position and normal, in the object's space or the world's,
  after the skin. It runs in the scene, the shadow pass and AO's depth,
  so the shadow sways with the grass. As built: `position` and `normal`
  are the world's; moving `local` moves the vertex in the mesh's own
  space, turned, scaled and bent with the object (the Knight's cape);
  `origin` is the object's place, for each copy's own sway. It reads
  values, `PerObject` and the time, not textures (the depth passes have
  none to give it; the shader is refused saying so).
- **How far it moves**: the material says (`Material(…, reach: 0.3)`),
  and the engine grows the object's bounds by that much for culling.
  Without it, a moved vertex can be cut off at the screen's edge.
- **Many copies**: a field of 2,000 grass blades in the wind, each with
  its own phase from its position, is one draw per mesh.
- **Shown with**: grass and trees in the wind, waves on water, a
  wobbling jelly, a skinned character with a cape that moves.

*Done when* goldens of moved meshes and their shadows match at set
times; culling keeps a moved vertex on screen; and the grass field
holds the budget.

### Phase 3: what's behind

Water looks wrong without knowing what's under it: the shore needs foam
where the water is shallow, and the floor should bend through it.

- **The scene's depth** (`scene_depth(in.screen)`), for shallow water,
  soft edges where a see-through thing meets the floor, and a force
  field that glows where it touches.
- **The scene's colour** behind a see-through surface
  (`scene_color(uv)`), for water and glass that bend what's behind them.
- **Paid only when used.** Keeping depth and copying the colour cost
  memory and bandwidth, most of all on phones' tile GPUs. They are made
  only in a frame where a shader asks for them, and they work at low too.
- **Shown with** a pond: shore foam, the floor bent and darkened with
  depth, waves from phase 2.

*Done when* the pond's goldens match at every level, and a frame without
such a shader keeps no depth or copy (checked by the renderer's own
counts).

As built: `scene_depth(screen)` (metres along the view; the far end
where there's only sky), `depth_behind(in)` (how far behind this point
the scene is), `view_depth(p)` and `scene_color(screen)` (the scene as
light, so returned as it is it shows the scene unchanged). A shader
that reads either is drawn see-through, over the solid scene, whatever
its blending, and casts no shadow. In a frame with one in view the
scene pass splits in two: the solid scene and the sky, keeping its
multisampled depth and resolving its colour into an image; one sample
of the depth copied into another; then that colour put back and the
see-through drawn over it. Cut-outs are in the depth, and everything
else see-through draws as it would without the shader (a test checks
it). `FrameStats::behind` and `behind_bytes` count it; the images go a
second after the last frame that read them. The cost is 28 bytes a
pixel at medium and high (16 of them the kept multisampled depth), 24
at low: about 80 MB on an iPhone's full screen, less with its render
scale. Phase 5 measures it on the phones; if it's too much, the depth
can come from a depth-only pass of the solid scene instead (no
multisampled depth kept, the geometry drawn twice).

### Phase 4: whole styles

- **Lighting of your own**: `fn light(in: LightInput) -> vec3f` in a
  shader replaces how each sun and lamp lights a point (its direction,
  colour, shadow and distance given), so a toon shader steps the light
  into bands. Shadows, clusters and fog stay the engine's.
- **Outlines** on a material (`Material(…, outline: Outline(width: 0.02,
  color:))`): the mesh drawn again, a little larger and inside out,
  behind it (decision 5). Skinned and moved meshes are outlined too.
- **The pixel look**: `camera.pixels = 240` draws the game 240 pixels
  high and enlarges it with hard edges, steady as the camera moves
  (positions snapped to the pixel grid). With a palette if asked.
- **Screen effects**: `camera.effects = [ScreenEffect(await
  assets.shader('old_tv.wgsl'), values: {...})]`, each a
  `fn effect(in: EffectInput) -> vec4f` over the finished picture, in
  order: colour grading, vignettes, a red flash when hurt, an old TV,
  dithering. They work at low too: the engine draws into an image only
  when there's an effect.
- **Shown with** a toon scene in the lab (KayKit characters, banded
  light, outlines), the pixel look on the garden, and a few effects.

*Done when* the styles' goldens match at every level, the pixel look
doesn't shimmer when the camera moves slowly (two frames compared), and
effects compose in order.

### Phase 5: games, the measure, the guide and the report

- **Sheepdog**: grass in the wind, a pond, wolves that burn away when
  chased off, a sheep that flashes when a wolf bites (decision 6).
- **Coins**: coins that shine as holograms, and a "toon" setting that
  switches the whole game's style.
- **Measured** on the Mac, the iPhone and the Note10+: each lab scene,
  and Sheepdog with everything on, at each level. The cost of making a
  new shader's pipelines (the first frame it's drawn), and `preload` to
  move it to a loading screen. If the budget isn't met, the fix comes in
  this phase.
- **The guide**: a new section, "Shaders" (after §13 Animation), §5's
  materials, §20 updated, §21 "From Unity" (Shader Graph, Surface
  Shaders, URP's renderer features and their Fynkel equivalents);
  `docs/stage-10-report.md`.

## 4. Decisions

Each has a recommendation. Accepting the plan accepts them, unless you
say otherwise.

1. **Shaders are written in WGSL, in files in the game's assets.**
   *Recommended*: WGSL is what the renderer already speaks (wgpu), it
   runs everywhere we ship, and the file is hot reloaded. No shader
   graph: in a code-first engine the code is the graph, and a graph
   editor is a whole editor. GLSL and HLSL aren't read.
2. **The engine owns the lighting; a shader writes the surface.**
   *Recommended*: as Unity's Surface Shaders and Fynkel 1 did. A shader
   is a few lines, and it's lit, shadowed, fogged and skinned at every
   quality level with nothing more. A shader's own lighting (phase 4) is
   a function inside that, not a whole new pipeline.
3. **Values are found in the shader, not declared twice.** The shader's
   `struct Values` is the list; Dart sets them by name, and a wrong name
   or type is an error saying what the shader has. *Recommended*: one
   place to change, as Unity's `Properties` but without a second
   language.
4. **One object's values are four numbers, set on its renderer.**
   *Recommended*: a hit flash or a dissolve is per enemy, and copying
   the material for each one makes a material per enemy, and breaks
   drawing many copies together. Four numbers ride with each object's
   drawing data (a free vertex location), cheap and enough for flashes,
   burns and phases. More than four: copy the material.
5. **Outlines are the mesh drawn again inside out**, not found from the
   picture's edges. *Recommended*: it's what most toon games do, works at
   low and on phones, follows skinning and wind, and costs one extra draw
   of the outlined things only. Outlines from the picture's edges need
   the scene's depth and normals at full size on every frame: phase 3
   makes depth possible, so that can come later if a game wants it.
6. **The games**: Sheepdog gets grass, a pond, burning wolves and a
   flash; coins gets hologram coins and a toon setting. *Recommended*:
   each kind of shader in a real game, and the toon setting shows a
   whole style switched by code. Say if you'd rather have a new small
   game for the styles.
7. **Not in this stage**: a shader graph; compute shaders and
   particles (particles are first after 1.0); decals; reflections of the
   scene itself; shaders that make new triangles (geometry,
   tessellation); a game's own passes and render targets (draw this
   layer into a texture); and a shader's own variants and keywords.
   *Recommended*: each is real work, and none is needed for the three
   kinds. They go in the guide's §20.

## 5. Checklist

- [x] Phase 1: surfaces
- [x] Phase 2: movement (Mac: 2,000 swaying blades add about 0.1 ms a frame at every level; phones in phase 5)
- [x] Phase 3: what's behind (memory: 28 bytes a pixel while a reader is in view; phones in phase 5)
- [ ] Phase 4: whole styles
- [ ] Phase 5: games, the measure, the guide and the report
