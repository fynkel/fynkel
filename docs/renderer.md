# Renderer

`crates/fynkel_render` — what a frame is and how it is shaded. Blueprint §8.5;
renderer V1 (a) is §19.2d item 4 (M3-04), V1 (b) — shadows, transparency,
MSAA, render scale, quality profiles, debug lines — item 5 (M3-05). The
renderer reads one thing, the
[`Extract`](../crates/fynkel_core/src/world.rs) a `fynkel_core` world hands
it each frame, and every host draws through the same `ExtractRenderer`
(`new(device, target_format, w, h)` → `prepare(device, queue, &extract)` →
`draw(encoder, &target_view)`), so the picture is host-independent by
construction and `fynkel render` / the pixel tests are the same path as
the Flutter viewport.

## Units

Everything is physical; nothing is "0 to 1" until the post pass.

| Quantity | Unit | Where |
| --- | --- | --- |
| `DirectionalLight.intensity` | lux (illuminance on a facing surface) | the sun; default 10 000 = clear sky |
| `PointLight.intensity`, `SpotLight.intensity` | candela; illuminance at `d` metres is `I / d²`, windowed to 0 at `range` | |
| `Environment.ambient_intensity` | lux, constant from every direction | default 2 000 |
| `Environment.sky_intensity` | nits (cd/m²) at a white texel of the sky — and the scale of the image-based lighting it casts | default 5 000 |
| `PbrMaterial.emissive` × `emissive_strength` | **relative to the exposure's white** — 1.0 renders as white whatever the EV; not nits, so an authored glow survives any camera | |
| radiance in the HDR target | nits | `Rgba16Float` |
| `Camera.exposure` | EV100 | `exposure_mode: manual` |

**Exposure** (`fynkel_render::exposure`, Lagarde & de Rousiers 2014): the
camera's EV100 decides how many nits are white, `L_max = 1.2 · 2^EV100`;
the post pass multiplies radiance by `1 / L_max` before tone mapping. In
`auto` mode (the default) the EV is metered from the scene's constant
illuminance, `EV = log2(max(E_sun, E_ambient, 1) / 2.5)` — an incident-light
meter at ISO 100 — so a 10 000-lux sun meters at 11.97 and a white
Lambertian surface facing it lands at 0.66 before tone mapping. `manual`
uses `Camera.exposure` as is (default 12). `ExtractRenderer::ev100()`
reports the value used; `fynkel render --json` prints it as `ev100`.

A scene that authors **no lighting at all** (no light of any kind, no
`Environment`) is lit by the host's fallback: a white 10 000-lux sun along
`ExtractRenderer::light_dir` plus the default `Environment`. As soon as a
scene has any light or an `Environment`, what it authored is all there is
(a scene with only a `PointLight` is dark outside its range). A scene
without an `Environment` component uses `Environment::default()` (white
2 000-lux ambient, no sky).

## Passes

The chain is `shadow → depth prepass → ambient occlusion → AO blur →
opaque → transparent → bloom → post → presentation → overlay`, every pass with
fixed inputs and outputs; the opaque and transparent stages share one
render pass because they share the attachments. The three passes before
the opaque one are the screen-space stage — ambient occlusion (R2) and
contact shadows (R3d) — and run only while the quality profile allows
one of them *and* the frame's volumes give it an intensity; the bloom stage (R3b) runs only while
the profile allows it *and* the frame's volumes give it an intensity.
Without them the chain is exactly what it was, down to the bits. Colour
grading (R3c) is no pass: it is arithmetic in the post pass, skipped
while the grade is neutral.

| Pass | Reads | Writes | When |
| --- | --- | --- | --- |
| **shadow** | shadow uniforms (sun `view_proj`), casters' base-colour map (mask discard) | `SHADOW_FORMAT` (`Depth32Float`) map, `shadow_resolution`², cleared | shadows on, a casting sun, ≥ 1 caster |
| **depth prepass** | frame uniforms, the opaque instances, their base-colour map (mask discard) | the scene `Depth32Float` (`msaa` samples), cleared and **stored** | ambient occlusion or contact shadows on, ≥ 1 opaque draw |
| **ambient occlusion** | the frame uniform alone, the stored depth | `AO_FORMAT` (`Rg8Unorm`: red the occlusion, green the contact shadows) at the internal size | the same |
| **AO blur** | the frame uniform, the depth, the AO target | a second `Rg8Unorm` — what the opaque pass samples | the same |
| **opaque** | frame uniforms, shadow map, the AO target, material maps | HDR colour (`HDR_FORMAT` `Rgba16Float`, `msaa` samples) + `Depth32Float`, the colour cleared to transparent black and the depth **loaded** from the prepass (cleared when the stage did not run); at MSAA 4 the colour resolves into the single-sample HDR texture and the samples are discarded | always |
| **transparent** | the same, plus the depth just written (read-only) | the same colour attachment, blended | `blend` instances queued |
| **bloom** | the resolved HDR texture, the frame uniform | `BLOOM_FORMAT` (`Rgba16Float`) chain at half the internal size, up to `BLOOM_LEVELS` (6) mips: a prefilter into level 0, a downsample into each next level, a blended upsample back into each finer one | bloom on in the quality profile, the resolved intensity > 0 |
| **post** | resolved HDR texture, bloom level 0, frame + post uniforms | the host's target — or the LDR intermediate (target format, internal size) when `render_scale ≠ 1` | always |
| **presentation** | LDR intermediate | the host's target, bilinear | `render_scale ≠ 1` |
| **overlay** | overlay uniforms (`view_proj`, its inverse, the camera), the scene depth (`msaa` samples) | the host's target, loaded and blended | a grid, a line or a triangle queued (item 9f, § The overlay) |

1. **Shadow**: the sun's orthographic view (`src/shadow.rs`, below) is
   rendered depth-only, no face culling, depth bias constant 2 / slope 2,
   one `draw_indexed` per batch over its shadow range (the casters inside
   the sun's box, `Batch::shadow_first..+shadow_count`). `alpha_mode:
   mask` casters discard exactly as in the opaque pass; `blend` materials
   never cast. `MeshRenderer.cast_shadows` opts a mesh out; placeholders
   and missing-mesh cubes cast.
2. **Depth prepass, ambient occlusion, AO blur**: § Ambient occlusion
   and § Contact shadows below.
3. **Opaque**: back-face-culled batches first, then double-sided ones,
   one `draw_indexed` per batch over its scene range (the instances
   inside the camera frustum, `Batch::first..+count`), then the
   **sky**, when the `Environment` names a texture: a fullscreen triangle
   at depth 1 with `LessEqual`, so only untouched texels get the
   equirectangular lookup (`u = 0.5 + atan2(d.x, −d.z) / 2π` — `u = 0.5`
   looks down −Z, `v` from up to down; `sky_rotation` turns the sky about
   +Y) × `sky_intensity`.
4. **Transparent**: every `blend` instance sorted **back to front** by the
   distance of its origin from the camera (whatever the record order),
   one `draw_indexed` each, depth tested and not written, blended
   `SrcAlpha / OneMinusSrcAlpha` (colour) and `One / OneMinusSrcAlpha`
   (alpha, so the coverage accumulates).
5. **Post**: exposure → the grade (§ Colour grading; skipped while
   neutral) → the tone mapper (ACES, Stephen Hill's fit of the RRT + sRGB
   ODT, unless a volume picks another) → lift, gamma, gain → sRGB encode unless the target format is an sRGB format (then the
   hardware encodes). The HDR alpha channel is a tag with coverage:
   `0` background → the exact `CLEAR_COLOR` bytes (`clear_rgba8()`; what
   the pixel tests compare against); `(0, 1]` scene-referred radiance
   premultiplied by coverage (a multisample resolve at a silhouette, a
   blend material over the background) → `aces(rgb / coverage × exposure)`
   composited over the clear colour by `coverage`; `> 1.5` display-referred
   (the unlit magenta cube: alpha 2 × coverage) → clamped and
   encoded only. At coverage 1 the result is bit-identical to V1 (a).
   With bloom (§ Bloom) its level 0 is added to the exposed radiance
   before ACES, and screen-blended over the uncovered part of a pixel.
   The grade applies to lit pixels only: the clear colour and
   display-referred texels are never graded.
6. **Presentation**: only with a render scale, a bilinear fullscreen copy
   of the LDR intermediate to the target size.
7. **Overlay**: the grid, then the lines, then the triangles, straight
   into the host's target at its own resolution (§ The overlay).

### Shadows (`src/shadow.rs`, `shaders/shadow.wgsl`)

One shadowed directional light — the sun — one square map
(`shadow_resolution` texels, default 2048) holding `CASCADES = 4`
cascades as a **2×2 atlas**, so the sun costs the same GPU memory it did
with a single fit (which matters on a phone).

`shadow::fit_cascades` splits the view out to `SHADOW_DISTANCE = 60 m`
(or the camera's `far`, or the far side of the casters when they stop
sooner) into four slices by the *practical* split scheme — the
logarithmic and uniform splits mixed by `SPLIT_LAMBDA = 0.75`, so the
slice nearest the camera is a few metres deep and the last one is tens.
Each slice's frustum corners are bounded by a sphere, that sphere is
**tightened to the casters' own bounds** when they are smaller (one
`Sphere::union` over the frame's casters, accumulated in the same
`prepare` loop that builds the instances — a scene whose casters span
twelve metres is never fitted to sixty; the union is taken over the
casters that reach the *loose* fit whether or not `frustum_culling` is
on, so culling still cannot change the picture), and `shadow::fit_sphere` then
looks along `−to_sun` with the eye at the origin, snaps the sphere
centre to that cascade's texel grid in light space (so the map does not
shimmer as the camera moves) and builds the ortho box `±r` around it,
reaching `3r + 10 m` back toward the sun so casters outside the view
still shadow what is in it. `SunFit::texel_world` is that cascade's
world size per texel. At 2048 the nearest cascade is about six times
finer than the single fit it replaced.

The shadow pass draws once per cascade: the viewport moves to that
cascade's quadrant (inset one texel, so the cleared border stops a PCF
tap from reaching the neighbour) and a **dynamic offset**
(`SHADOW_UNIFORM_STRIDE = 256`) picks its `view_proj` out of the one
shadow uniform buffer. The caster set is the same in every cascade —
what a cascade does not cover its projection clips away — so
`FrameStats::shadow_draws` is the batches with casters times
`shadow_cascades`, while `shadow_casters` stays the distinct casters.
Per-cascade caster culling is not done yet
(`docs/status.md` §3). The sun's shadow is
active when the quality profile has `shadows`, the sun casts
(`DirectionalLight.cast_shadows`, or the host's fallback sun) and at
least one caster exists; `FrameStats::shadow_map` reports the map side
(0 when none), `shadow_cascades` the cascades in it.

Receivers (`MeshRenderer.receive_shadows`, instance `flags.w`) pick the
first cascade whose projection contains the point, offset the lookup 1.5
texels **of that cascade** along the geometric normal
(`SHADOW_NORMAL_OFFSET_TEXELS`, the mapped normal is not used for this;
per cascade, so a near shadow keeps contact with its caster instead of
peter-panning), remap into the cascade's atlas rect and take a 3×3
**PCF** with a linear `LessEqual` comparison sampler
(`textureSampleCompareLevel`) — nine taps one atlas texel apart, so the
penumbra is about three texels of that cascade wide. Over the outer
`CASCADE_BLEND = 0.1` of a cascade's *sides* the result is cross-faded
into the next one, so the change of texel density is a gradient and not
a seam (the sides only: a cascade's depth range reaches `3r + 10 m` back
toward the sun, so every receiver sits near its far plane and a depth
term in the band would blend everywhere — which is what it did in the
first cut, and it cost 0.8 ms of the scene pass at stress scale).
Points no cascade covers — past the shadowed range — are lit. Only the
sun's direct term is shadowed; ambient and punctual lights are not.

### Ambient occlusion (`shaders/ao.wgsl`, R2)

The sun's shadow says what the sun cannot see; ambient occlusion says
what the *sky* cannot see — the contact darkening in a corner, under a
crate, along a seam — and without it every ambient-lit surface is the
same flat value whatever is standing next to it.

It is screen-space, computed from depth, in three passes between the
shadow and the scene:

1. **The depth prepass** draws every opaque and mask instance the scene
   pass will draw — the same four pipelines, the same batches, the same
   ranges — with `fs_depth` (colour dropped, only the alpha-mask discard
   left) over the scene pass's **own vertex stage**. Two things make the
   depths agree to the bit, and both are load-bearing: `VsOut.clip` is
   `@invariant`, and the prepass culls exactly as the scene pass does
   (`Pipelines::depth(skinned, double_sided)` mirrors `opaque(…)`). The
   scene pass then loads this depth rather than clearing it, and its
   compare is `LessEqual` so an equal depth still shades. A prepass that
   culled differently would write a *nearer* depth than the scene pass
   computes and reject nearly every fragment — which is exactly what a
   single no-culling prepass did before this was fixed.
2. **The SSAO pass** reads that depth (`texture_depth_multisampled_2d`
   at MSAA, sample 0 — AO is low-frequency, per-sample occlusion buys
   nothing) and writes the red byte of an `Rg8Unorm` pixel (the green is
   the contact shadows', § Contact shadows). Per pixel: the world
   point from `inv_view_proj`, the normal from the four neighbouring
   depths (the nearer neighbour on each axis, so a silhouette does not
   bend it) flipped to face the camera, then `AO_SAMPLES` (12) points in
   a hemisphere about that normal, the radius (`frame.ao.x`, 0.6 m
   unless a volume says otherwise — § Volumes) out, crowded
   toward the origin by a golden-angle spiral and turned per pixel by
   interleaved gradient noise. A sample occludes when the surface drawn
   at its pixel is nearer the eye than the sample point by more than
   `AO_BIAS` (2 cm) — both lie on the same ray, so their distances order
   them — weighted by `radius / |s − p|` so an occluder beyond the
   hemisphere fades out instead of darkening across a gap. The result is
   `(1 − occlusion)^intensity` (`frame.ao.y`; 0 — the estimate is
   skipped and the red is 1 — unless a volume gives it one: § Volumes).
3. **The blur** averages a 4×4 square of that (both channels), dropping taps more than
   2 % of the view distance away along the ray, which is what turns the
   per-pixel rotation back into a smooth term without smearing across a
   silhouette.

`light.wgsl` multiplies the blurred value into `ao` — the material's own
occlusion factor — so it scales **only** the ambient and IBL term:

```wgsl
radiance += mix(ambient_term, ibl_term, frame.sky.z) * ao;
```

Direct light is not occluded by it (that is the shadow map's job, and
darkening a sunlit face with a screen-space guess is the classic way to
make a scene look dirty). Blend instances are skipped: they are not in
the prepass, so the AO under them belongs to whatever is behind them.

The stage is one `Rg8Unorm` pair at the internal size (4 bytes a pixel on
top of the attachments; 2 before R3d gave it the green channel) and one query pair in the GPU timer — its begin
is the prepass's, its end the blur's, so `FrameStats::gpu_ao_us` and
`ViewportStatsSnapshot.gpuAo` are the whole stage.
`FrameStats::ambient_occlusion` says whether the occlusion was drawn;
the profile's `ambientOcclusion` turns it off, and with `contactShadows`
off too the chain, the attachments and the picture are what they were
before R2. A volume whose intensity resolves to 0 skips the three passes
too unless contact shadows need them (the attachments stay, so walking
out of it costs nothing to switch back).

Measured (`benchmarks/m3/results/2026-09-22-94490ad`): 1.81 ms p95 on the
stress scene, where the whole GPU frame nevertheless fell from 3.56 to
3.07 ms p95 — the depth prepass hands the scene pass a complete depth
buffer, and at 5 530 instances early-Z saves more than the three passes
cost. In a nearly empty frame there is no overdraw to save and the cost
is just cost: the reference game's GPU frame went 4.01 → 6.11 ms p95. A
half-resolution AO target is the lever if that ever matters — the
estimate is low-frequency and blurred anyway.

### Volumes (R3a, `fynkel_core::volume`)

The *look* of a frame — how strong the occlusion is, the bloom (R3b),
the grade (R3c) — is scene data, not a project setting, and it
can change with where the camera is. That is a **volume**: a
`fynkel.Volume` component (id 25) on an entity, pointing at a **profile**
document.

- **The profile** (`<name>.volume.json`, `schemas/volume.schema.json`,
  `DocumentKind::Volume`; [documents.md](documents.md) § Volume profiles)
  is override blocks of optional fields. A field that is present
  overrides what is below it; a field that is absent is inherited. R3a's
  one block is `ambientOcclusion { radius (m, > 0), intensity (the
  exponent, ≥ 0; 0 = off) }` — R2's two constants, now data; R3b adds
  `bloom { intensity (≥ 0; 0 = off, the default), threshold (exposed
  brightness, default 0.9), scatter (`0..=1`, default 0.7), tint ([r,
  g, b], per channel) }` (§ Bloom); R3c adds `colorGrading { tonemapper,
  postExposure, temperature, tint, contrast, saturation, hueShift,
  colorFilter, lift, gamma, gain }` (§ Colour grading); R3d adds
  `contactShadows { intensity (`0..=1`; 0 = off, the default), length
  (m, default 0.15), thickness (m, default 0.05), distance (m, default
  50) }` (§ Contact shadows). `fynkel
  document new volume <name>` writes one with the engine defaults spelled
  out.
- **The component** says where the profile applies: `shape` `global`
  (everywhere), `box` (`size`, full extents in the entity's local units)
  or `sphere` (`radius`); `blend_distance` (metres outside the shape
  over which it fades out), `weight` (`0..=1`), `priority` (higher blends
  over lower; ties in scene order), `enabled`. Rotation and scale apply:
  the shape is measured in the entity's local space and the fade
  distance back in world metres. Its value rules are `FYN-H0011`.
- **Resolution** runs once a frame on the CPU, in `prepare`
  (`ExtractRenderer::resolve_volumes` → `volume::resolve`): start from
  the engine defaults (`PostSettings::default()`: every effect off, as
  in Unity — a scene without a volume has no ambient occlusion and no
  bloom; until 2026-09-24 the default was R2's ambient occlusion at
  0.6 m ^1.35, which made a volume's weight barely visible),
  take every enabled volume in the extract (`Extract::volumes`), its
  amount = `weight × factor(camera position)` — 1 inside the shape, `1 −
  d / blend_distance` at distance `d` outside it, 0 beyond — and, lowest
  priority first, lerp each present field toward the profile's value by
  that amount. The result is `ExtractRenderer::post_settings()`;
  `FrameStats::volumes` counts the volumes that reached the camera, and
  `fynkel render` prints both (`ambient occlusion 0.90 m ^1.60, bloom
  1.00 >0.70, 1 volume(s)`; `volumes`, `bloom` and `post` in `--json`).
- **The quality profile is the ceiling.** A volume shapes an effect the
  project's quality profile has on; it cannot turn on one the profile has
  off (`ambientOcclusion: false` or `bloom: false` keeps that stage off
  whatever a volume says) — the profile is what the *device* can afford, the volume is
  what the *scene* wants.

Profiles are read through the asset store (`Assets::volume_profile`, the
cooked document's `JSON` chunk) once per reference and cached like the
materials; a scene depends on the profiles its volumes name, so they are
retained with it. A profile that cannot be read is one `asset_errors`
line and contributes nothing. Nothing about volumes is on the GPU but
the result: `Frame.ao` carries the frame's radius and exponent,
`Frame.bloom` and `Frame.bloom_tint` the bloom, `Frame.grade` the grade.

Deliberately not in R3a: a volume per camera (there is one camera a
frame), a volume *mask* layer, animated profiles (a behaviour can set
`weight` or `enabled` through the component like any other field), and
non-linear fades.

### Bloom (`shaders/bloom.wgsl`, R3b)

Light brighter than white spreads into its neighbourhood — a lamp head,
the sunlit face of a pillar, an emissive panel — the way it does in a
lens and an eye. It is a volume block (§ Volumes), off by default: a
scene without a volume that names `bloom.intensity` renders exactly as
R3a did, and the stage does not run.

1. **Prefilter** (level 0, half the internal size): four `textureLoad`s
   of the resolved HDR, each ×exposure (`Frame.camera_pos.w`, so the
   threshold is in *exposed* units: 1 = white on the screen) and clamped
   at 1024 so a single hot texel cannot become a square, averaged with
   Karis weights `1 / (1 + luma)` (the fireflies of a specular highlight
   do not flicker the glow). Background texels (alpha 0) and
   display-referred ones (alpha > 1.5: the unlit magenta cube)
   contribute nothing. Then a soft-knee threshold: `threshold`
   with a knee of half of it, so brightness enters the glow over a band
   instead of at an edge.
2. **Downsample**, level by level down to 2 texels on the short side
   (at most six levels): Jimenez's 13-tap filter (Call of Duty: Advanced
   Warfare), bilinear, which is what keeps the chain free of the blocky
   pulsing a 2×2 box leaves.
3. **Upsample**, coarsest first: a 9-tap tent into each finer level,
   **blended** over what the downsample left there with
   `Constant / OneMinusConstant` — the blend constant is `0.05 + 0.9 ×
   scatter` (`set_blend_constant`), so `scatter` 0 keeps the glow tight
   around its source and 1 carries the coarse levels wide. No extra
   targets: every level is a mip of one texture, read in one pass and
   written in the next.
4. **Composite** in the post pass: `aces(radiance × exposure + glow ×
   intensity × tint)` for a lit pixel; over the background (and the
   uncovered part of an edge pixel) the glow screen-blends onto the clear
   colour, `bg + (1 − bg) × aces(glow)`, so a lamp against the sky still
   haloes.

The bloom passes bind the one-entry frame group the AO passes use (group
0) and their own `{sampler, texture}` group 1; the post pass gained its
own layout with the chain's level 0 at binding 3 (a 1×1 black texture
while the quality profile has bloom off). The chain costs `8 bytes ×
(w/2 × h/2) × 4/3` of memory; the stage is timed as part of the post
pair (§ GPU timestamps), so the Apple ABI did not change.

`bloom` in the quality profile (default true) is the ceiling as for AO:
with it false the chain is not even allocated. Deliberately not in R3b:
lens dirt, anamorphic streaks, a physically based (convolution) bloom,
and per-light flares.

### Colour grading (`shaders/post.wgsl`, R3c)

The last block of the profile (§ Volumes) is the grade: Unity's
Tonemapping, White Balance, Color Adjustments and Lift Gamma Gain in one
`colorGrading` block. Every field is neutral by default — ACES and
nothing else, the picture the engine always drew — and while the
resolved grade is neutral apart from the tone mapper the post pass
skips it: a scene without the block, or with every field at its neutral
value, renders to the bit what it did before R3c (the showcase's
`r3b.png`). In the post pass, on a lit pixel's exposed radiance (with
the bloom already added):

1. **Before the curve**, one matrix (`Grading::pre_contrast`):
   `postExposure` (stops, `× 2^ev`), then the white balance — Unity's
   `ColorBalanceToLMSCoeffs`: `temperature` moves the illuminant along
   the daylight locus and `tint` across it (both `-100..=100`), and the
   picture is scaled in LMS (CAT02) to take that illuminant to D65, so
   positive is warmer and negative cooler.
2. **Contrast** (`-100..=100`) in log space around middle grey:
   `0.18 × (c / 0.18)^(1 + contrast/100)`.
3. **After the curve**, one matrix (`Grading::post_contrast`): the
   `colorFilter` (linear RGB), the `hueShift` (degrees, a turn about the
   grey axis, so grey stays grey), the `saturation` (`-100` is the Rec.
   709 luma, `100` twice as far from it).
4. **The tone mapper**: `aces` (the default), `neutral` (Unity's: John
   Hable's curve with its constants, white at 5.3 — little hue or
   saturation shift, a starting point for grading by hand) or `none`
   (clamped at white). Choosing one is not a grade; it applies with
   nothing else set. A choice does not blend: any part of a volume sets
   it, as Unity treats its enum parameters.
5. **Lift, gamma, gain** on the tone-mapped `0..=1`, per channel:
   `gain × (c + lift × (1 − c))`, then `^(1 / gamma)`, clamped — lift
   (`-1..=1`) raises the darks with white held, gain (`0..=4`) scales,
   gamma (`> 0`) bends the midtones.

Everything but the curve is folded on the CPU into `Frame.grade`
(`GpuGrade`: the two matrices, the contrast exponent, the tone mapper,
lift, 1 / gamma, gain, and whether the grade runs); the numbers blend
across volumes like every other field, colours per channel.
`FrameStats::grading` says the frame was graded; `fynkel render` prints
`graded` (and a tone mapper other than ACES), `--json` has `grading` and
the resolved `post.grading`. The editor's Volume Profile Inspector has a
Color Grading card (the tone mapper a menu, lift/gamma/gain as R, G, B).

There is no quality flag: the grade is a few dozen instructions a pixel,
no memory. Deliberately not in R3c: a LUT (baking the grade into a 3D
texture, and loading one made elsewhere), Unity's shadows/midtones/
highlights and channel mixer, curves, and grading the clear colour.

### Contact shadows (`shaders/ao.wgsl`, R3d)

The shadow map is a few centimetres a texel at best, and its depth bias
lifts a shadow off its caster by about as much: where an object meets
the ground, and in the creases of a model, the sun's shadow is missing or
detached. Contact shadows (Unity HDRP's) fill that in from the screen:
a profile's `contactShadows` block (§ Volumes) gives them an
`intensity` — the shadow's opacity, `0..=1`, 0 (off) by default.

They are the green channel of the screen-space stage (§ Ambient
occlusion), so they share its depth prepass, its target and its blur;
the stage runs when either effect is on. Per pixel, from the world point
the depth gives: `CONTACT_STEPS` (12) steps of a march `length` metres
(0.15) toward the sun, dithered per pixel and started a pixel's footprint
off the surface along its normal. A step is in shadow when the surface
drawn at its screen position — on the step's own view ray — is in front
of it by more than a footprint (`CONTACT_BIAS`) and less than
`thickness` (0.05 m) plus that: a surface is taken to be that thick, so
a march passing behind a thin post is not stopped by it. The first such
step makes the pixel `1 − intensity`; past `distance` (50 m) from the
camera there are none, faded over the last tenth. There is no test of
the normal against the sun: the normal from depth is least sure at a
foot, where contact shadows matter most, and a face turned from the sun
gets no sunlight to darken anyway.

`light.wgsl` darkens the **sun's** light only, by the nearer of the two
shadows (`min(shadow map, contact)`), on instances that receive shadows
and are in the prepass (not blend ones). Point and spot lights are not
marched. With the green channel off it is 1 and nothing reads it: a
frame without the block renders to the bit what it did before (the
test checks intensity 0 and the profile's off against no volume).

The quality profile's `contactShadows` (default on) is the ceiling. It
also keeps the stage's targets: a profile with `ambientOcclusion` off
and `contactShadows` on still has the pair. No new pass, pipeline or
target: the green channel doubles the pair's bytes (2 → 4 a pixel).
`FrameStats::contact_shadows` says they were marched; `fynkel render`
prints `contact shadows <intensity> over <length> m`, `--json` has
`contactShadows` and `post.contact*`; the editor's Volume Profile
Inspector has a Contact Shadows card. They need a sun: without one the
stage does not run for them. Deliberately not in R3d: per-light contact
shadows (Unity's per-light toggle), and a `distanceScaleFactor` that
lengthens the march with distance.

### Instancing and culling (`src/culling.rs`)

Every frame `prepare` rebuilds one instance buffer from the extract
(ADR-0011: thousands of objects, one draw per distinct `(mesh, material)`
pair). Each record's mesh carries its cooked bounds — `MeshPayload
{ bounds_min, bounds_max }`, turned into a `Sphere` (centre, half
diagonal) when the mesh is uploaded; the placeholder cube's is the unit
cube's — and `Sphere::transformed` carries it into world space (centre
by the model matrix, radius by the largest axis scale, so a non-uniform
scale stays conservative). Two `Frustum`s are built before the instance
loop (Gribb–Hartmann planes of a `view_proj` with `0..=1` clip depth):
the camera's and, when the sun casts, the light's box from `shadow::fit`
— the loose fit around the whole shadowed range, which is why it still
precedes the instances even though the cascades are fitted after them.
The test is sphere-vs-plane with the sphere kept when it touches, so
culling never changes the picture: `frustum_culling = false` renders
pixel-identically (the culling test asserts it — which is why the
casters' bound the cascades are tightened to is taken against the loose
fit either way, not against whatever culling left).

Each opaque instance is tagged with where it is drawn — a caster outside
the view but inside the light's box is **shadow only**, a caster in view
**both**, a non-caster in view **scene only**; anything in neither is
dropped before upload, and a `blend` instance out of view is dropped
outright (it never casts). Within a batch the instances are laid out
`[shadow only][both][scene only]`, so the shadow draw is the first two
categories and the scene draw the last two: two contiguous ranges, one
upload, no duplicate instance. `FrameStats::instances` is what the scene
passes draw, `culled` the records outside the camera frustum (drawn into
the shadow map or not), `draws` the scene pass's mesh draw calls (opaque
batches with something in view + one per transparent instance),
`shadow_draws` the shadow pass's (the batches with casters times the
cascades), `batches` the distinct opaque pairs.
The stress capture checks `draws ≤ batches` at 5 000 meshes
(`benchmarks/m3/capture.py`, below).

### Transparency

`alpha_mode: blend` is an instance flag (`flags.x = BLEND`), not a
material variant: the mesh shader writes `clamp(base.a)` as the alpha tag
instead of 1 and the transparent pipelines blend by it. Blend materials
are lit like any other (the sun's shadow included), draw after every
opaque surface, never cast a shadow and never write depth, so two
overlapping blend meshes composite correctly only in their sort order
(per mesh, not per triangle — the usual V1 limit).

### MSAA and render scale

`msaa` 4 multisamples the scene passes (colour and depth) and resolves
into the single-sample HDR texture; the post pass sees the resolved
coverage in the alpha tag, so a silhouette over the background blends
toward the clear colour instead of picking one sample. `msaa` 1 renders
directly. A profile asking for 2 runs at 1, for 8 at 4 (V1 implements
exactly these two; the profile keeps its number). `render_scale` sizes
the scene passes and the post pass at `round(target × scale)` (`0.25..=2`)
and the presentation pass scales the result to the target; at `1` the
post pass writes the target directly and no intermediate exists.
`FrameStats::{msaa, render_width, render_height}` say what ran.

### Quality profiles (`src/quality.rs`)

The renderer is the first consumer of the project's `quality` block
(blueprint D3): `fynkel cook` copies the profiles into the cache's
`index.json` (`project.quality`), `fynkel_assets::ProjectInfo::quality`
carries them, and `ExtractRenderer` resolves the active one at each
`prepare`:

1. an explicit `set_quality(Quality)` override (`profile: "custom"`),
2. the profile named by `set_quality_profile(Some(name))` when the store's
   project has it,
3. the project's default profile (`unknown_profile` names the requested
   one when it was not found — the request is kept for the next store),
4. the engine defaults (`profile: "default"`: shadows on, 2048, ambient
   occlusion on, bloom and contact shadows allowed, MSAA 4, scale 1, anisotropy 8) when the project declares
   no quality block.

A profile becomes a `Quality` through `clamped()`: `msaa` 1 or 4,
`shadow_resolution` a power of two in `256..=8192`, `render_scale` in
`0.25..=2`, `anisotropy` in `1..=16`; `textureMaxSize` is the cook's
concern (D3) and the renderer ignores it; `ambientOcclusion`, `bloom`
and `contactShadows` are straight on/offs. Switching is cheap where it can
be: pipelines are rebuilt only when the sample count changes,
attachments when the internal size, the samples or whether the screen-space stage or bloom has targets change,
the shadow map
when its resolution changes, the material sampler (and with it the cached
material bind groups) when the anisotropy changes. `quality()` reports the
`QualityStatus` in force; `quality_profiles()` lists the names. Every
surface exposes the switch — `fynkel render --quality <profile>`, the
Apple viewport's `fynkel_apple_viewport_set_quality`,
`FynkelViewportController.quality` (the `fynkel create` starter cycles it
on **Q**).

### The overlay (`src/overlay.rs`, `shaders/overlay.wgsl`, item 9f)

What is drawn *over* the picture — the editor's grid, outlines, icons and
handles, a game's debug drawing — goes through `ExtractRenderer::overlay()`
(ADR-0019 §1), an `Overlay` queue that `prepare` uploads and clears. It
draws **after** post and presentation, into the host's target at the
target's resolution, so it is never exposed, tone mapped, bloomed or
resolved through MSAA coverage: `[1, 0, 0, 1]` is pure red whatever the
camera, the profile or the sample count. Colours are linear and alpha is
real (`SrcAlpha / OneMinusSrcAlpha`); the pass encodes to sRGB itself
when the target is not an sRGB format.

- **Lines** (`push_line(a, b, colour, width, flags)`) are instanced
  screen-space quads: each end is projected, clipped against the near
  plane, and the segment widened by half its `width` in target pixels
  plus one, with a one-pixel analytic edge (`smoothstep` on the distance
  from the centre line), so a 1.5 px line is smooth at every angle.
- **Triangles** (`push_triangle(a, b, c, colour, flags)`) are flat and
  unlit; each vertex is pushed out two pixels and the fragment fades by
  its screen-space distance to the three edges, so their edges are
  anti-aliased like the lines'.
- **The ground grid** (`grid(Grid)`) is one fullscreen triangle that
  intersects each pixel's view ray with `y = 0`. The cell is a power of
  ten chosen from the larger of the pixel's footprint and the camera's
  height (so it neither shimmers far away nor turns to a carpet up
  close), three levels (minor, ×10 major, ×100) cross-fading as the
  camera moves, lines by screen-space derivatives (no moiré), faded with
  distance and at grazing angles, and the X and Z axes in their colours.
  It reads the scene depth and never writes it, so it does not z-fight a
  floor at `y = 0`: under a floor it is hidden.
- **Occlusion.** Every primitive compares its distance from the camera
  with the scene's, reconstructed from the depth attachment (the farthest
  of its MSAA samples, so an edge never hides what is behind it) through `inv_view_proj`, with a tolerance of
  ×1.001 + 2 mm. What is behind the scene is drawn at `OCCLUDED_ALPHA`
  (0.25) of its alpha, not dropped; `ON_TOP` opts a primitive out (the
  handles). While the overlay has something to draw the scene pass
  stores its depth instead of discarding it.

`ExtractRenderer::debug()` keeps its `DebugDraw` API — `line`, `aabb`
(twelve edges), `axes` — and is now a client of the overlay: its lines
join the frame's at `DEBUG_LINE_WIDTH` (1.5 px), occluded-faded. The
Apple viewport takes the overlay as `fynkel_apple_viewport_overlay`
(ABI v6) and `package:fynkel_flutter` as `ViewportOverlay` /
`FynkelViewportController.overlay`. `FrameStats` counts `debug_lines`
(every overlay line), `overlay_triangles` and `grid`. Physics shapes
come with the physics item.

**The selection outline** (`outline(selected, children, OutlineStyle)`,
`src/outline.rs`, `shaders/outline.wgsl`) is Unity's: an orange ring just
outside the silhouette of what is selected and a blue one around its
children, one ring around the union of what each list covers. It is two
passes after post and presentation and before the overlay's lines, so a
handle or gizmo line still draws over it. The *mask* pass draws every
listed entity that has a draw record in view — meshes, placeholder cubes,
skinned meshes in their pose, `blend` materials by their whole surface,
`mask` materials cut at their cutoff — from their own instance buffer
(the scene's batches are left alone) into an `R8Unorm` texture at the
target's size, one sample, with no depth test, so a selected object
behind a wall is still ringed where it would be; each writes 1.0
(selected) or 0.5 (children) and the pipelines blend by `Max`, so an
entity in both lists is selected. The *composite* pass is one fullscreen
triangle: inside the mask it discards (the object keeps its own shading),
outside it searches a square of `ceil(width) + 1` texels for the nearest
covered one and draws its colour with alpha `clamp(width + 0.5 − d, 0, 1)`
— at the default width of 2 target pixels a one-pixel solid ring and a
half-alpha fringe, the same at every render scale and sample count
(`width` is in target pixels, capped at `MAX_OUTLINE_WIDTH` = 16; 0 or
less draws nothing). The nearest texel's value picks the colour (`> 0.75`
is `selected`), colours are linear with straight alpha and are encoded to
sRGB like the overlay's. `OutlineStyle::default()` is orange
`[1, 0.4, 0, 1]`, blue `[0.37, 0.47, 0.61, 1]`, width 2. Like the rest of
the overlay it is consumed by the next `prepare`; with nothing queued, or
nothing listed in view, neither pass runs and the frame is bit-identical
to one without it. `FrameStats.outlined` counts the instances drawn into
the mask. The Apple viewport takes it as `fynkel_apple_viewport_outline`
(ABI v8) and `package:fynkel_flutter` as `ViewportOverlay.outline`; the
editor gets the entities to list from its ids through the authoring
preview's `entities` kind (the live world's `handles`).

**Extra worlds** (`push_extra_world(ExtraWorld { extract, transform,
gray })`) draw other worlds into the same frame as the extract `prepare`
is given — the editor's Prefab Mode *in context* (the prefab's preview
is the main world; the scene it was opened from is an extra world at
`inverse(instance world matrix)`, so the prefab stays at its own origin,
greyed in Unity's "Gray" mode) and multi-scene editing (the active
scene's preview is the main world, the other loaded scenes' previews
are extra worlds at identity). They are not an overlay: `prepare` walks
the main extract's records, then each extra world's with its model
matrices pre-multiplied by `transform`, into the one instance buffer —
the same batches, the same depth buffer, depth prepass and AO, the same
light pool (an extra world's point and spot lights are placed by the
transform, their range scaled by its largest axis, and are culled and
assigned per object like the main world's), the same shadow cascades
(its casters cast into them and its surfaces receive). Skinned meshes
are posed by the extra world's own palettes, uploaded after the main
extract's (each instance's first joint offset by the palettes before
it). The main extract's camera, `Environment`, volumes and so exposure
and post are the frame's; an extra world's first directional light is
the sun only when the main extract has none (and its lights count as
authored, so the fallback sun stays off). Meshes and materials come from
the renderer's asset store — the main world's; every preview of one
authoring session shares one cache. The selection outline only rings
the main extract's entities (handle bits are per world). **Grey** is
bit 1 of the instance's fourth flag (bit 0 is "receives shadows"); the
mesh stage replaces the final shaded colour — lit radiance with
emissive and ambient, or an unlit colour — by `0.5 · dot(rgb, (0.2126,
0.7152, 0.0722))` in every channel (desaturated to its Rec. 709
luminance, then dimmed by half, before exposure and tone mapping; alpha
and the blend tag kept), so a grey world is still depth-tested,
shadowed and casting. The queue is consumed by the next `prepare`
(`clear_extra_worlds` drops it); without extra worlds the frame is
bit-identical to what it was before they existed (the shading tests'
46 dumped frames compare equal byte for byte). `FrameStats.extra_worlds`
counts the ones drawn. The Apple viewport takes them as
`fynkel_apple_viewport_extra_worlds(vp, worlds, transforms, flags,
count)` (ABI v9: `FynkelWorld*`s, 16 column-major floats and one flag
word each — `FYNKEL_APPLE_EXTRA_WORLD_GRAY` = 1 — extracted by the call;
a later call replaces an earlier one, a NULL world is skipped, -1 for a
NULL viewport, a NULL array with a count or a non-finite transform) and
`package:fynkel_flutter` as `ViewportOverlay.extraWorld(worldAddress,
{transform, gray})`, queued by `FynkelViewportController.overlay` and
cleared with the overlay; the editor passes each preview's
`ScenePreview.worldAddress`. Picking stays per preview: every hit of
`ScenePreview.pick`/`pickRay` carries its `distance` along the ray, so
the nearest across several identity-placed previews is the smallest.

Placeholder cubes (entities without a `MeshRenderer`, when
`placeholders` is on) are lit like a rough white dielectric in their
palette tint; a mesh missing from the cache is the unlit magenta cube.

## Shading (`shaders/mesh.wgsl`, `light.wgsl`, `pbr.wgsl`)

The mesh shader is three files after `frame.wgsl` (ADR-0031):
`mesh.wgsl` the vertex stage (static and skinned), `light.wgsl` the
engine's light — everything below once a surface is known, in
`light_surface(in, front_facing, LitSurface)` — and `pbr.wgsl` the
built-in `fynkel.pbr@1` surface: its maps and factors, the alpha
cutoff, the unlit path and the depth/mask fragments. A `lit` shader asset
ends in the same `light_surface`, so it is lit exactly as the built-in
is. The split changed no pixel (the reference game and showcase render
byte-identical before and after). `Frame.time` carries the world's time
(`Extract::time`, interpolated between ticks, still while paused), the
seconds since the previous frame and the internal target's size — a
shader's `globals`.

Cook-Torrance per light: GGX normal distribution (`α = roughness²`),
height-correlated Smith visibility (Heitz), Schlick Fresnel with
`f0 = mix(0.04, albedo, metallic)`; Lambert diffuse `albedo · (1 − metallic) / π`.
Radiance per light is `(diffuse + specular) · E · N·L` with `E` the
light's illuminance at the point (the sun's lux; `I / d² · (1 − (d/range)⁴)²`
for point and spot lights, spot lights further scaled by
`smoothstep(cos outer, cos inner, cos θ)`). The environment adds one
term × `occlusion`: with an HDR sky the split-sum image-based lighting
below, otherwise the constant ambient `(diffuse + f0) · E_ambient / π`.
Emissive adds `emissive · strength · L_max`.

Every material map is sampled with one repeat/trilinear sampler at `uv0`
(anisotropic up to the profile's `anisotropy`) over the **full mip chain
the cook wrote** (M3-08b: `TEXR` carries every level down to 1×1, box
filtered in linear light for sRGB maps; `upload_rgba8` creates the
texture with `mip_level_count = mip_levels` and writes each level, so a
distant textured floor stops shimmering):

| Map | Use | Fallback |
| --- | --- | --- |
| `base_color_texture` (sRGB) | × `base_color` × vertex colour; alpha feeds the mask | white |
| `normal_texture` (linear) | tangent-space normal, `xy` × `normal_scale`, in the frame `(t, b = (n × t) · w, n)` from the cooked vertex tangent; a zero tangent (no UVs at cook) keeps the vertex normal | flat (128, 128, 255) |
| `metallic_roughness_texture` (linear) | `B` × `metallic`, `G` × `roughness` (glTF layout) | white |
| `occlusion_texture` (linear) | `1 + occlusion_strength · (R − 1)` on the ambient term | white |
| `emissive_texture` (sRGB) | × `emissive` | white |

`alpha_mode: mask` discards texels whose alpha is below `alpha_cutoff`
inside the opaque pass (and the shadow pass); `blend` goes to the
transparent stage (above). `double_sided`
materials draw without culling and flip the normal on back faces
(`@builtin(front_facing)`). Roughness is clamped to 0.045 so the GGX lobe
stays finite. Normals are transformed by the model matrix and
renormalised (no inverse transpose: non-uniform scale skews them — accepted
for V1).

Tangents come from the cook: `fynkel_import::tangents::ensure` generates
an orthonormal frame per vertex (Lengyel accumulation over the triangles,
Gram-Schmidt against the normal, handedness in `w`) for any mesh whose
source had none, and the `MESH` chunk info records `"tangents":
"authored" | "generated" | "none"`. This closes the M2 compromise
("tangent generation is a renderer decision").

### Image-based lighting (E1)

An `Environment` whose `sky` names a **`.hdr`** image lights the scene
from that image; every other sky (a PNG or JPEG backdrop) keeps the
constant ambient. Split-sum (Karis 2013), with the two environment
integrals cooked once per image and the BRDF integral built once per
renderer:

- The cook writes an `ENVM` chunk next to the texture
  ([documents.md](documents.md)): a GGX-**prefiltered specular cube**,
  128² with 6 mips for roughness 0 → 1 (256 importance samples a texel,
  each sampling the equirect at a mip chosen by its solid angle, so a
  rough level does not sparkle), a cosine-convolved **irradiance cube**
  at 32² storing `E / π`, and `meanIrradiance` for metering. Both are
  RGBA16F, in the same equirect frame the sky pass uses, faces in
  WebGPU order (+X −X +Y −Y +Z −Z).
- The renderer uploads them as two `TextureCube`s (bind group 0,
  bindings 4 and 5, with a trilinear clamp-to-edge `cube_sampler` at 3)
  and builds a 64 × 64 RGBA16F **BRDF LUT** at 6 (`create_brdf_lut`, 512
  samples per texel, the same height-correlated Smith visibility as
  `mesh.wgsl`, written in `ensure_fallbacks`). A scene without an
  environment binds 1 × 1 black cubes, which wgpu zero-initialises.
- `light.wgsl` samples `irradiance_cube` along `rotate_sky(n)` and
  `specular_cube` along `rotate_sky(reflect(−v, n))` at
  `roughness · (mips − 1)`, and the LUT at `(N·V, roughness)`:
  `diffuse · irradiance + prefiltered · (f0 · a + b)`, all × the
  `sky_intensity` the backdrop uses — so the sky you see and the light
  it casts are the same image at the same exposure. `rotate_sky` (in
  `frame.wgsl`) is shared with the sky pass, so `sky_rotation` turns the
  lighting with the backdrop.
- Auto exposure meters the environment too: the mean irradiance ×
  `sky_intensity` joins the ambient term in `EV = log2(max(E_sun,
  E_ambient, 1) / 2.5)`.

`FrameStats::ibl` says whether the frame sampled an environment, and
`fynkel render --json` reports it. The cubes are uploaded when the
`Environment`'s sky changes (`ibl_dirty` → `rebuild_frame_bind_group`);
their bytes count in the renderer's GPU estimate. No pipeline variant is
added: the shader mixes between the two terms with `frame.sky.z`.

## Lights per object (E1)

A frame has a **pool** of up to `LIGHT_POOL = 64` point + spot lights, and
each object is shaded by the `MAX_LIGHTS = 8` of them that reach it — the
blueprint's Tier-1 budget, now spent per object instead of per frame.

Two passes, both on the CPU in `prepare`:

1. **Into the pool.** A light whose range sphere misses the camera
   frustum is left out: only what is inside the frustum is shaded, so
   this cannot change the picture (it is exact, not conservative, which
   is why the `frustum_culling` switch does not gate it). If more than
   `LIGHT_POOL` still reach the view, the nearest to the camera win and
   `FrameStats::lights_dropped` counts the rest.
2. **Onto the object.** Each instance's world bounding sphere — the one
   frustum culling already computes — is tested against every light in
   the pool: those whose range reaches it are scored by the shader's own
   windowed inverse square at the nearest point of the bounds, times the
   light's brightest channel. The strongest `MAX_LIGHTS` win; the list is
   the pool indices, one per byte. Shadow-only instances and unlit
   materials get no list — neither is shaded by a punctual light. A light
   whose `culling_mask` lacks the object's layer bit is skipped here
   (ADR-0020), so the mask costs nothing on the GPU.

`FrameStats`: `lights` (the pool), `lights_dropped`, `lights_max_per_object`
and `lights_overflow` (instances that had more than eight on them);
`fynkel render --json` prints all four (`lights`, `lightsDropped`,
`lightsMaxPerObject`, `lightsOverflow`). No clustering and no compute
pass: the assignment is a loop over the pool per instance, which is why
the pool is bounded.

That loop is what the stress scene's many-lights variant measures
(`generate.py --lights`, below): 5 571 instances against a full pool of
64 cost **+0.45 ms of `prepare`** (2.10 ms p95 against 1.64 ms for the
same frame with one light), and that is the ceiling — the pool is capped,
so authoring more lights only grows the frustum pass over them, which
runs once a frame. A cluster grid buys nothing at this budget; it becomes
the answer when the *pool* has to grow, not when the scene does.

One sun (the first `DirectionalLight`), the only light that casts a
shadow, and it is not part of any of this — every object sees it (its
mask is deferred by ADR-0020).

## Layer masks (ADR-0020, L3)

Each `ExtractRecord` carries `layer_bit`, the bit of its entity's
`fynkel.Layer`. It is `1` (`default`) when the entity has none. The scene
camera's `culling_mask` is tested first in `prepare`'s record loop. A
record outside it is skipped before anything else: it is not drawn, it
does not cast into the shadow map, and it gets no light list. Placeholders
and missing-mesh cubes are skipped the same way. `FrameStats::layer_hidden`
counts these records, and `fynkel render --json` prints it as
`layerHidden`. The host's own camera (a scene without a `Camera`) and the
editor's camera draw every layer. Point and spot light masks apply only in
the per-object assignment above. The debug lines, the overlay, the
editor's outlines and icons, and picking do not go through the record
loop, so no mask applies to them.

## Uniform layout

`Frame` (bind group 0, 4 944 bytes, `shaders/frame.wgsl`): `view_proj`,
`inv_view_proj` (the sky's ray reconstruction), `cascades` (four
`Cascade { view_proj, rect, params }`: world → that cascade's shadow
clip, its quadrant in atlas uv, and its normal offset in metres),
`camera_pos` (`w` = exposure factor `1 / L_max`),
`sun_dir` (`w` = present), `sun_color` (rgb × lux), `ambient` (rgb × lux,
`w` = `L_max`), `sky` (`x` intensity, `y` rotation, `z` image-based
lighting active, `w` specular mips − 1), `shadow` (`x` active,
`y` one atlas texel in uv, `z` the blend band, `w` cascades), `ao` (the
volumes' resolution, § Volumes: `x` radius, `y` exponent — 0 when off),
`contact` (§ Contact shadows: `x` opacity — 0 when off —, `y` length,
`z` thickness, `w` distance), `bloom` (`x`
threshold, `y` knee, `z` the upsample blend `0.05 + 0.9 × scatter`, `w`
intensity — 0 when the stage did not run), `bloom_tint` (rgb), `grade`
(§ Colour grading: `params` — `x` contrast exponent, `y` 1 when the grade
runs, `z` tone mapper 0 ACES / 1 neutral / 2 none —, `pre` and `post`
three column vectors each, `lift`, `inv_gamma`, `gain`), `counts` (`x` lights,
`y` sky drawn, `z` encode sRGB, `w` the screen-space stage ran), then `lights: array<Light, LIGHT_POOL>`
(64) —
`position` (`w` range), `color` (rgb × cd, `w` 0 point / 1 spot),
`direction` (`w` cos outer), `params` (`x` cos inner). Bind group 0 also
holds the shadow map (`texture_depth_2d`, binding 1 — a 1×1 map when
shadows are off), its comparison sampler (binding 2), the environment's
cube sampler (3), irradiance cube (4), prefiltered specular cube (5) and
BRDF LUT (6) (§ Image-based lighting). The shadow pass
binds its own 64-byte `view_proj` uniform at group 0, one slot per
cascade at `SHADOW_UNIFORM_STRIDE` (256) bytes, chosen by a dynamic
offset.

Per instance (vertex buffer 1, 128 bytes, locations 6–13): model matrix,
`tint` (base colour factor), `emissive` (rgb × strength; `w` = the
instance's first joint in the palette, § Skinning), `params`
(metallic, roughness, normal_scale, occlusion_strength), `flags` (alpha
cutoff, −1 opaque or −2 blend; unlit; **the slot of this instance's light
list**; receives shadows). `flags.z` was the double-sided flag, which no
shader read — the CPU picks the pipeline from it — and the pipeline's
sixteen vertex attributes (0–5 vertex, 6–13 instance, 14–15 skin weights)
are all taken, so the list itself lives in a storage buffer.

Bind group 2 (`skin_layout`, vertex stage, part of the mesh and shadow
pipeline layouts and always bound): two read-only storage buffers —
binding 0 `array<mat4x4<f32>>`, the frame's joint palettes (§ Skinning; 64
matrices before the first growth, `next_power_of_two` beyond), and
binding 1 `array<vec4<u32>>`, one light list per instance in the instance
buffer's order (§ Lights per object; 256 before the first growth). Either
outgrowing its capacity rebuilds the one bind group.

### Skinning (M3-19, [animation.md](animation.md))

A skinned mesh (`MeshPayload::is_skinned`: a `SkinWeight` per vertex
after the indices) uploads a second vertex buffer, `GpuMesh.skin_buffer`,
bound at slot 2 as `VsSkin { joints: vec4<u32> @14 (Uint16x4), weights:
vec4<f32> @15 }`. The extract hands the renderer `Extract.joints` — the
concatenated palettes of every skinned mesh whose `fynkel.Skin` resolved
this frame, each `inverse(mesh_world) · joint_world · inverse_bind` from
the joints' *interpolated* world matrices — and per mesh an
`ExtractMesh.skin: Option<ExtractSkin { first_joint, joint_count }>`.
`prepare` writes the palettes to the group 2 storage buffer once per
frame (grown and its bind group recreated when the count outgrows it),
carries `first_joint` in `Instance.emissive.w`, and keys batches by
`(mesh, material, skinned)` so skinned instances of one mesh still
instance in one draw. `vs_skinned` computes `pose = Σᵢ wᵢ ·
joints[min(base + jᵢ, arrayLength − 1)]` and `model = instance_model ·
pose`, then calls the static path's `vertex(in, model)` — one shading
path, one shadow path (`shadow.wgsl` has the same entry, so a posed
character casts its posed shadow). A skinned mesh whose entity has no
`Skin` component, or whose skin the world does not hold, draws through
the static pipeline as cooked: its bind pose. `FrameStats.skinned` and
`.joints` count the frame's skinned instances and palette size (the
standalone's stats line, `fynkel render`'s summary); `fynkel render
--steps <n>` ticks the world `n` times before drawing so a pose can be
captured, and registers the scene's prefabs, clips and skins first.
Test: `tests/project.rs` `skinned_meshes_follow_their_joint_palette` —
the fixture's Rig scene at tick 1 matches the same extract with the skin
dropped (bind pose), and after 30 ticks the bent column has moved
hundreds of pixels to the left while the unposed extract has not.

## GPU timestamps and the memory estimate (M3-16)

With `Features::TIMESTAMP_QUERY` (`device_features(adapter)` asks for it;
Metal has it everywhere Fynkel runs) the renderer writes a timestamp
pair at the boundaries of the shadow, scene, post and present passes,
and a fifth pair split across the ambient-occlusion stage's three passes
(begin at the prepass, end at the blur), into one of two alternating
query sets, resolves the **previous** frame's
set at the start of the next `draw` (Apple's passes overlap, so a
same-frame resolve reads unwritten end stamps) and maps the copy without
waiting; `FrameStats::gpu_shadow_us`/`gpu_ao_us`/`gpu_scene_us`/`gpu_post_us` are
the passes (the post pair begins at the bloom prefilter when bloom ran,
so `gpu_post_us` includes it) and `gpu_frame_us` the span `max(end) − min(begin)`, three or four
frames late, with `gpu_timed`/`gpu_untimed` as the read-back counters.
`prepare_us`/`encode_us` time the two CPU halves, and `gpu_bytes` is the
estimate of what the renderer holds on the GPU (mesh buffers, textures'
mip bytes, the cube map, attachments per texel × samples, the shadow
map, its own buffers). [instrumentation.md](instrumentation.md) has the
full table.

## Pipelines and caches — the variant cap

Exactly **thirty** render pipelines exist, all created in `new`
(`PIPELINE_VARIANTS`) and rebuilt as a set only when the sample count
changes; nothing is compiled at draw time:

| Pipeline | Target | Samples | Culling | Depth | Blend | Vertex entry |
| --- | --- | --- | --- | --- | --- | --- |
| `mesh` | HDR | msaa | back | write, `LessEqual` | — | `vs_main` |
| `mesh_double_sided` | HDR | msaa | none | write, `LessEqual` | — | `vs_main` |
| `transparent` | HDR | msaa | back | read, `LessEqual` | src-alpha | `vs_main` |
| `transparent_double_sided` | HDR | msaa | none | read, `LessEqual` | src-alpha | `vs_main` |
| `mesh_skinned`, `mesh_skinned_double_sided`, `transparent_skinned`, `transparent_skinned_double_sided` | the same four | | | | | `vs_skinned` (M3-19, § Skinning) |
| `sky` | HDR | msaa | none | read, `LessEqual` | — | |
| `shadow` | shadow map (depth only) | 1 | none | write, `Less`, biased | — | `vs_main` |
| `shadow_skinned` | the same | | | | | `vs_skinned` |
| `depth`, `depth_double_sided` | none (depth only) | msaa | back / none | write, `LessEqual` | — | `vs_main` (R2, § Ambient occlusion) |
| `depth_skinned`, `depth_skinned_double_sided` | the same two | | | | | `vs_skinned` |
| `ao`, `ao_blur` | `R8Unorm` | 1 | none | none | — | |
| `bloom_prefilter`, `bloom_down` | `Rgba16Float` | 1 | none | none | — | (R3b, § Bloom) |
| `bloom_up` | the same | 1 | none | none | constant (scatter) | |
| `post` | host format | 1 | none | none | — | |
| `present` | host format | 1 | none | none | — | |
| overlay `grid`, `lines`, `triangles` | host format | 1 | none | none (the scene depth is a texture) | src-alpha | `vs_grid`, `vs_line`, `vs_triangle` (item 9f, § The overlay) |
| `mask`, `mask_double_sided` | `R8Unorm` (the outline mask) | 1 | back / none | none | `Max` | `vs_main`, `fs_mask` (§ The overlay, the selection outline) |
| `mask_skinned`, `mask_skinned_double_sided` | the same two | | | | | `vs_skinned` |
| outline `composite` | host format | 1 | none | none | src-alpha | fullscreen |

Material features (maps present or not, mask, blend, unlit) are data —
instance flags and fallback textures — not variants. A new variant needs
a row in this table. The item 19 rework of the cap: skinning is a
different vertex input layout (a third vertex buffer) and a different
vertex entry, which wgpu cannot make data, so it doubles the mesh and
shadow rows — the 9 of M3-05 became 14, and R2's depth prepass (which
must cull exactly as the opaque pass does) plus the two AO passes made
20, and R3b's three bloom passes 23 (the overlay's three making 25, and
the selection outline's four mask twins and its composite 30 — the mask
must cull and cut exactly as the scene does, so it is the mesh rows
again with one more fragment entry), still all created up front and never at draw time; nothing else about a skinned draw (material, lights,
shadow, blend) differs, and `Pipelines::opaque(skinned, double_sided)` /
`transparent(…)` pick the variant. Bind groups are cached per material `AssetRef`
(five texture views + the sampler, the fallback view for every unbound
slot) and per texture; the post and present bind groups are rebuilt with
the attachments (`resize`, a quality change), the frame bind group with
the shadow map and with the environment's cubes, the sky bind group when
the `Environment` names a different texture, and every material bind group when the sampler
changes (anisotropy). Opaque instances are batched by `(mesh, material)`
in order of first appearance, laid out per batch as § Instancing and
culling describes; the back-face-culled batches draw before the
double-sided ones, so a frame switches pipeline at most once between
them. Transparent instances are not batched: their order is their depth.

Eviction: at the start of `prepare` the renderer compares
`Assets::evictions` with what it last saw and drops the GPU copies of
every asset the store no longer holds — a material bind group whenever any
of its maps went — then reloads what the extract still references.

## Deviations from the blueprint rule

The §19.2d scope rule named the optional sky a `fynkel.sky@1` material.
M3-04 implements it as a texture reference on `fynkel.Environment`
(`sky: Option<AssetRef>` to an equirectangular image, `sky_intensity`,
`sky_rotation`) instead: the sky has no per-material parameters beyond
those three, a material document would have been an envelope around one
texture reference, and the `Environment` component is where a scene
author looks for it. A `fynkel.sky@1` shader id remains free for a
procedural or custom sky later.

## The showcase

`examples/showcase` (README there) is the scene to look at: an evening
sun with long shadows, a spot and two point lights, the sky — an HDR
one since E1, so it lights the scene as well as backing it
(`tools/sky.py` writes `assets/textures/sky.hdr`) —, textured PBR,
metals that read as metal, overlapping blend panels, a mask card, an
orbiting camera, three quality profiles, and (M3-19) the reference game's rigged runner —
a skinned mesh looping its run clip in place, the skinned caster in the
shadow map. `fynkel render --project examples/showcase --steps 10` draws
the view the app opens on with the runner mid-stride; `renders/<item>.png`
is committed at the end of every renderer item, so a change is a
before/after in the history and a number in the app's HUD.

## Tests

`crates/fynkel_render/tests/shading.rs` — one pixel test per feature:
manual/auto exposure, no light = black + point light side and range +
the light budget, spot cone, ambient colour and the default
`Environment`, orthographic vs perspective, the pipeline count, normal +
roughness maps on the fixture's textures (and the cooked tangents they
need — the `MESH` info assertion is in `fynkel_import/tests/cook.rs`),
alpha mask, double-sided back faces, emissive without light, sky backdrop
+ rotation, the HDR sky lighting a cube through its cooked environment
(a PNG sky does not) with `sky_rotation` steering the lookup; from V1 (b): the sun's PCF shadow on a slab (darker under the
cube, a soft edge, gone with `shadows: false`, a 512 map, a non-casting
sun); from R1: a sixty-metre corridor with a caster at each end, both
shadowed (every quadrant of the atlas drawn and sampled where it
belongs), the near one keeping contact with the floor, and no seam in
the luminance of the lit floor where the cascades change; from R2: a
cube standing on an ambient-lit floor — the floor at its foot darkens by
a quarter, the open floor four metres away is bit-identical to the same
frame with `ambientOcclusion: false`, and the occlusion fades outward
over about forty centimetres instead of ending at an edge; from R3b: a
bright cube under a hot environment — a volume with bloom intensity 0 is
bit-identical to the quality profile's `bloom: false`, the profile's
off beats a volume's on, the glow lifts the background next to the cube
and fades with distance, a threshold above the scene's brightness
changes nothing, a red tint only reddens, and a wider scatter reaches
farther; from R3d: the same cube under a sun that casts no map shadow —
contact shadows darken the sunlit floor at its foot, leave the open
floor and the cube's own sunlit top alone, run without ambient
occlusion, need a sun, and at intensity 0 or with the profile's
`contactShadows: false` are bit-identical to no volume;
`cast_shadows` / `receive_shadows` per `MeshRenderer`, blend
materials compositing back to front whatever the record order, MSAA 4
blending silhouette edges (and 8 ≡ 4, 2 ≡ 1), render scale 0.5 / 0.75 /
2 presenting at the target size (and back to native pixel-identical),
the fixture project's `low` / `medium` / `high` profiles switching at
runtime (an unknown name refused, the override), debug lines drawn and
depth tested; from item 9f: overlay lines widening with their width and
anti-aliased at the edge, fading behind the scene unless `ON_TOP`, the
same pixels at EV 0 and EV 6 and at MSAA 1 and 4, filled triangles with
anti-aliased edges, and the grid on a floor at `y = 0` (the same as a
floor a millimetre below), hidden under one 20 cm above, and denser near
the camera than far; the selection outline: a selected cube ringed just
outside its silhouette (full colour one pixel out, half two out, nothing
three out at MSAA 1, the same ring at MSAA 4) and untouched inside,
children in their own colour with selected winning an entity in both,
a cube ringed behind the geometry that hides it, the width in target
pixels (2 against 6, 0 drawing nothing, the same ring at render scale
0.5 and 2), the frame after an outline — or with one naming nothing, an
entity the extract lacks or one on a layer the camera leaves out —
bit-identical to a renderer's that never had one, a blend material ringed by its whole surface, and (in
`tests/project.rs`) the fixture's skinned meshes ringed in their pose,
not their bind pose; extra worlds: a cube in an extra world drawn
through its transform bit-identical to the same cube in the main world,
lit by the main world's sun, and gone from the next frame (bit-identical
to a renderer's that never had one; an empty or cleared queue changes
nothing), a grey extra world desaturated (channel spread under 1) and
at most 0.8 of its normal brightness while the main world's pixels stay
as they were, an extra world's point light lighting the main cube in
front of it and not behind it, an extra world's cube shadowing the main
slab and a grey extra slab (with the only sun) receiving the main
cube's shadow, and (in `tests/project.rs`) the rig scene's skinned
columns posed by the extra world's palettes, not the main extract's;
from V1 (c): frustum culling dropping four off-screen cubes
(one of them still casting its shadow into the view) with the picture
identical to `frustum_culling = false`. `tests/offscreen.rs` counts a
40 × 25 grid of placeholders as drawn + culled = 1 000 in one draw.
`tests/project.rs` renders the fixture project (rusted
robot with its swatch, normal and metallic-roughness maps under its
`Sun`) from the cache alone. `FYNKEL_RENDER_DUMP_DIR=<dir>` /
`FYNKEL_RENDER_DUMP=<file>` write the frames as PPM for eyeballing.
`src/exposure.rs`, `src/shadow.rs`, `src/quality.rs`, `src/culling.rs`
and `src/debug.rs` have unit tests for the meter, the light fit (the
splits tiling the shadowed range, every cascade finer than the next and
the nearest six times finer than the single fit it replaced, the
casters' bounds tightening a cascade without dropping a caster), the
clamps, the sphere/frustum test and its union, and the primitives. The M0 capture
(`benchmarks/m0/results/2026-09-20-12859e1`) shows the cube scene
unchanged at MSAA 4 with the shadow pass: 122 / 120 frames per second,
engine CPU ≈ 0.1 ms, GPU ≈ 0.7–1 ms.

### The stress scene (`benchmarks/stress`, `benchmarks/m3/capture.py`)

`benchmarks/stress/generate.py` writes an ordinary project (git-ignored;
the generator is the source) with 5 000 static meshes — the showcase's
cube, pyramid and quad over four materials, 12 distinct `(mesh,
material)` pairs — plus 500 "Body" cubes and 300 "Behaviour" pyramids
standing in for the rigid bodies and Dart behaviours items 8 and 10 add,
a floor, a casting sun, an `Environment` and a camera above the near edge
looking across the field (the corners and the strip behind it are
culled). Since M3-08 the bodies are real: every Body cube has a dynamic
`fynkel.RigidBody` (mass `size³·2`) and a box `fynkel.Collider` on layer
`bodies`, the floor and every static mesh a box collider, so the project
is also the physics gate — `capture.py` runs it headlessly for 600 and
1 800 ticks (`physics.step_time_us` while the bodies fall,
`settled_step_time_us` once they rest: p50 0.64 ms both, against 0.28 ms
without physics; 1 430 collision enters, the checksum repeated;
`docs/physics.md`). Since M3-10 the 5 000 "static" meshes move too:
native `Rotator`/`Oscillate`/`LookAtCamera` components (`docs/catalogue.md`),
colliders kept — the settled tick is p50 2.34 ms, physics 2.0 ms of it
(`--no-movers` restores the still baseline), and `capture.py` tabulates
the per-system times from the headless report. `benchmarks/m3/capture.py` generates, locks and cooks it,
renders it once for the counts, runs it in the standalone host
(`--cache <dir>`) and the Flutter host (`FYNKEL_PROJECT`) and writes
`benchmarks/m3/results/<date>-<commit>.{json,md}` with the item 6 gate:
5 000 meshes at 120 Hz, `draws ≤` distinct pairs, `culled > 0`, and (item
8) ≥ 500 bodies touching, p50 tick under 4 ms falling and settled, the
checksum repeated. First
capture (`2026-09-20-cf2b3eb`, profile `medium`): 5 804 entities, 5 343
instances drawn in 15 draws (458 culled, 5 702 casters in 14 shadow
draws), standalone 122 frames/s with prepare + encode + submit 1.4 ms
p50, Flutter 120 ticks/s with `render()` 2.6 ms and GPU 3.2 ms p50.

`generate.py --lights N` scatters N lamps over the same field on their
own grid — three in four point lights (range 8 m), every fourth a spot
(14 m) — without moving anything else (the default is 0, and the bare
project regenerates identical). `capture.py` generates, locks, cooks and
runs that variant (`benchmarks/stress/project-lights`, 256 lights) right
after the baseline run and gates the E1 light culling on it: the pool
fills (64 of the 256 reach the view, 189 dropped), no object takes more
than 8 (738 of 5 530 instances clipped to the budget), and `prepare`
stays within a millisecond of the same scene without them. Capture
`2026-09-22-64221f7`: 121 frames/s against 122, prepare p50/p95
2.00 / 2.10 ms against 1.56 / 1.64, GPU frame 3.73 / 4.24 ms against
2.93 / 2.94 — the shading of eight lights a pixel costs more than the
assignment does.

Capture `2026-09-22-5abaac3` is the same field after R1, and it is what
the cascades cost: 122 frames/s, p99 interval 8.66 ms, eighteen checks
green. The shadow pass runs four times a frame for **0.31 / 0.55 ms**
against 0.40 / 0.40 with one cascade — each cascade is a quarter of the
map, so the rasterised area did not change and only the vertex work
multiplied. The scene pass carries the rest: 1.73 / 3.08 ms against
1.85 / 2.42, about +0.7 ms at p95 for the per-pixel cascade selection
and the blend band.

`generate.py --app` wraps the same project in the `fynkel create`
starter app (runners, `main.dart` with the HUD, the Spinner behaviour;
the starter's own scene, cube, material, swatch and test are dropped)
and puts the showcase's `game.Orbit` on the camera (target the field's
centre, radius 60, height 26, phase 0 — the bare camera's exact pose, so
the first frame matches the bare render — at 0.08 rad/s, so the culled
count breathes as the view turns), so it runs on a phone: `fynkel run
--project benchmarks/stress/project -d <iphone>` — the cook stages the
cache into the bundle (M3-06a) and the HUD's first line is the frame
rate. On macOS from the bundle: 4 600–4 700 instances in view, 15
draws, 120 fps with `render()` 2.7 ms. The bare project stays what CI
and `capture.py` measure; the ids and the field are identical, the app's
scene differs by that one component.
