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
| `Environment.sky_intensity` | nits (cd/m²) at a white texel of the sky | default 5 000 |
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

The chain is `shadow → opaque → transparent → post → presentation`, every
pass with fixed inputs and outputs; the opaque and transparent stages
share one render pass because they share the attachments.

| Pass | Reads | Writes | When |
| --- | --- | --- | --- |
| **shadow** | shadow uniforms (sun `view_proj`), casters' base-colour map (mask discard) | `SHADOW_FORMAT` (`Depth32Float`) map, `shadow_resolution`², cleared | shadows on, a casting sun, ≥ 1 caster |
| **opaque** | frame uniforms, shadow map, material maps | HDR colour (`HDR_FORMAT` `Rgba16Float`, `msaa` samples) + `Depth32Float`, both cleared (colour to transparent black); at MSAA 4 the colour resolves into the single-sample HDR texture and the samples are discarded | always |
| **transparent** | the same, plus the depth just written (read-only) | the same colour attachment, blended | `blend` instances or debug lines queued |
| **post** | resolved HDR texture, frame + post uniforms | the host's target — or the LDR intermediate (target format, internal size) when `render_scale ≠ 1` | always |
| **presentation** | LDR intermediate | the host's target, bilinear | `render_scale ≠ 1` |

1. **Shadow**: the sun's orthographic view (`src/shadow.rs`, below) is
   rendered depth-only, no culling, depth bias constant 2 / slope 2, one
   `draw_indexed` per batch over its casters (they sort first inside each
   `(mesh, material)` batch, `Batch::casters`). `alpha_mode: mask`
   casters discard exactly as in the opaque pass; `blend` materials never
   cast. `MeshRenderer.cast_shadows` opts a mesh out; placeholders and
   missing-mesh cubes cast.
2. **Opaque**: culled batches first, then double-sided ones, then the
   **sky**, when the `Environment` names a texture: a fullscreen triangle
   at depth 1 with `LessEqual`, so only untouched texels get the
   equirectangular lookup (`u = 0.5 + atan2(d.x, −d.z) / 2π` — `u = 0.5`
   looks down −Z, `v` from up to down; `sky_rotation` turns the sky about
   +Y) × `sky_intensity`.
3. **Transparent**: every `blend` instance sorted **back to front** by the
   distance of its origin from the camera (whatever the record order),
   one `draw_indexed` each, depth tested and not written, blended
   `SrcAlpha / OneMinusSrcAlpha` (colour) and `One / OneMinusSrcAlpha`
   (alpha, so the coverage accumulates). Then the **debug lines**
   (`LineList`, depth `LessEqual` without write, no lighting).
4. **Post**: exposure → ACES (Stephen Hill's fit of the RRT + sRGB ODT) →
   sRGB encode unless the target format is an sRGB format (then the
   hardware encodes). The HDR alpha channel is a tag with coverage:
   `0` background → the exact `CLEAR_COLOR` bytes (`clear_rgba8()`; what
   the pixel tests compare against); `(0, 1]` scene-referred radiance
   premultiplied by coverage (a multisample resolve at a silhouette, a
   blend material over the background) → `aces(rgb / coverage × exposure)`
   composited over the clear colour by `coverage`; `> 1.5` display-referred
   (unlit magenta cube, debug lines: alpha 2 × coverage) → clamped and
   encoded only. At coverage 1 the result is bit-identical to V1 (a).
5. **Presentation**: only with a render scale, a bilinear fullscreen copy
   of the LDR intermediate to the target size.

### Shadows (`src/shadow.rs`, `shaders/shadow.wgsl`)

One shadowed directional light — the sun — one cascade, one square map
(`shadow_resolution` texels, default 2048). Each frame `shadow::fit`
bounds the camera frustum out to `SHADOW_DISTANCE = 60 m` (or the
camera's `far`) with a sphere, looks along `−to_sun` with the eye at the
origin, snaps the sphere centre to the shadow texel grid in light space
(so the map does not shimmer as the camera moves) and builds the ortho box
`±r` around it, reaching `3r + 10 m` back toward the sun so casters
outside the view still shadow what is in it. `SunFit::texel_world` is the
map's world size per texel. The sun's shadow is active when the quality
profile has `shadows`, the sun casts (`DirectionalLight.cast_shadows`,
or the host's fallback sun) and at least one caster exists;
`FrameStats::shadow_map` reports the map side (0 when none) and
`shadow_casters` the casters.

Receivers (`MeshRenderer.receive_shadows`, instance `flags.w`) offset the
lookup point 1.5 texels along the geometric normal
(`SHADOW_NORMAL_OFFSET_TEXELS`, the mapped normal is not used for this),
project it by `sun_view_proj`, and take a 3×3 **PCF** with a linear
`LessEqual` comparison sampler (`textureSampleCompareLevel`) — nine taps
one texel apart, so the penumbra is about three texels wide. Points
outside the map or beyond its far plane are lit. Only the sun's direct
term is shadowed; ambient and punctual lights are not.

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
4. the engine defaults (`profile: "default"`: shadows on, 2048, MSAA 4,
   scale 1, anisotropy 8) when the project declares no quality block.

A profile becomes a `Quality` through `clamped()`: `msaa` 1 or 4,
`shadow_resolution` a power of two in `256..=8192`, `render_scale` in
`0.25..=2`, `anisotropy` in `1..=16`; `textureMaxSize` is the cook's
concern (D3) and the renderer ignores it. Switching is cheap where it can
be: pipelines are rebuilt only when the sample count changes,
attachments when the internal size or samples change, the shadow map
when its resolution changes, the material sampler (and with it the cached
material bind groups) when the anisotropy changes. `quality()` reports the
`QualityStatus` in force; `quality_profiles()` lists the names. Every
surface exposes the switch — `fynkel render --quality <profile>`, the
Apple viewport's `fynkel_apple_viewport_set_quality`,
`FynkelViewportController.quality` (the `fynkel create` starter cycles it
on **Q**).

### Debug primitives (`src/debug.rs`)

`ExtractRenderer::debug()` is a `DebugDraw` queue of world-space
coloured lines — `line`, `aabb` (twelve edges), `axes` — that `prepare`
uploads and clears, so a host queues what it wants drawn between frames.
They draw at the end of the transparent stage, depth tested against the
scene and never written, in display-referred colour (`[1, 0, 0, 1]` is
pure red whatever the exposure). Physics shapes come with the physics
item.

Placeholder cubes (entities without a `MeshRenderer`, when
`placeholders` is on) are lit like a rough white dielectric in their
palette tint; a mesh missing from the cache is the unlit magenta cube.

## Shading (`shaders/mesh.wgsl`)

Cook-Torrance per light: GGX normal distribution (`α = roughness²`),
height-correlated Smith visibility (Heitz), Schlick Fresnel with
`f0 = mix(0.04, albedo, metallic)`; Lambert diffuse `albedo · (1 − metallic) / π`.
Radiance per light is `(diffuse + specular) · E · N·L` with `E` the
light's illuminance at the point (the sun's lux; `I / d² · (1 − (d/range)⁴)²`
for point and spot lights, spot lights further scaled by
`smoothstep(cos outer, cos inner, cos θ)`). Constant ambient adds
`(diffuse + f0) · E_ambient / π · occlusion` — the V1 stand-in for
image-based lighting. Emissive adds `emissive · strength · L_max`.

Every material map is sampled with one repeat/trilinear sampler at `uv0`
(anisotropic up to the profile's `anisotropy`; mip chains come with a
later cook item, so today the anisotropy only sharpens the base level):

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

## Lights per draw

Up to `MAX_LIGHTS = 8` point + spot lights are uploaded per frame in one
uniform array and every draw sees all of them; when a scene has more, the
eight nearest to the camera are kept and `FrameStats::lights_dropped`
counts the rest (`fynkel render --json`: `lights`, `lightsDropped`). No
culling or clustering yet — V1 (c) (item 6, frustum culling and stress
scenes) is where the budget gets measured. One sun (the first
`DirectionalLight`), the only light that casts a shadow.

## Uniform layout

`Frame` (bind group 0, 816 bytes, `shaders/frame.wgsl`): `view_proj`,
`inv_view_proj` (the sky's ray reconstruction), `sun_view_proj` (world →
shadow map clip), `camera_pos` (`w` = exposure factor `1 / L_max`),
`sun_dir` (`w` = present), `sun_color` (rgb × lux), `ambient` (rgb × lux,
`w` = `L_max`), `sky` (`x` intensity, `y` rotation), `shadow` (`x` active,
`y` `1 / resolution`, `z` normal offset in metres), `counts` (`x` lights,
`y` sky drawn, `z` encode sRGB), then `lights: array<Light, 8>` —
`position` (`w` range), `color` (rgb × cd, `w` 0 point / 1 spot),
`direction` (`w` cos outer), `params` (`x` cos inner). Bind group 0 also
holds the shadow map (`texture_depth_2d`, binding 1 — a 1×1 map when
shadows are off) and its comparison sampler (binding 2). The shadow pass
binds its own 64-byte `view_proj` uniform at group 0.

Per instance (vertex buffer 1, 128 bytes, locations 6–13): model matrix,
`tint` (base colour factor), `emissive` (rgb × strength), `params`
(metallic, roughness, normal_scale, occlusion_strength), `flags` (alpha
cutoff, −1 opaque or −2 blend; unlit; double-sided; receives shadows).

## Pipelines and caches — the variant cap

Exactly **nine** render pipelines exist, all created in `new`
(`PIPELINE_VARIANTS`) and rebuilt as a set only when the sample count
changes; nothing is compiled at draw time:

| Pipeline | Target | Samples | Culling | Depth | Blend |
| --- | --- | --- | --- | --- | --- |
| `mesh` | HDR | msaa | back | write, `Less` | — |
| `mesh_double_sided` | HDR | msaa | none | write, `Less` | — |
| `transparent` | HDR | msaa | back | read, `Less` | src-alpha |
| `transparent_double_sided` | HDR | msaa | none | read, `Less` | src-alpha |
| `sky` | HDR | msaa | none | read, `LessEqual` | — |
| `shadow` | shadow map (depth only) | 1 | none | write, `Less`, biased | — |
| `debug` | HDR | msaa | none | read, `LessEqual` | — |
| `post` | host format | 1 | none | none | — |
| `present` | host format | 1 | none | none | — |

Material features (maps present or not, mask, blend, unlit) are data —
instance flags and fallback textures — not variants. A new variant needs
a row in this table. Bind groups are cached per material `AssetRef`
(five texture views + the sampler, the fallback view for every unbound
slot) and per texture; the post and present bind groups are rebuilt with
the attachments (`resize`, a quality change), the frame bind group with
the shadow map, the sky bind group when the `Environment` names a
different texture, and every material bind group when the sampler
changes (anisotropy). Opaque instances are batched by `(mesh, material)`
in order of first appearance, casters first within a batch; the culled
batches draw before the double-sided ones, so a frame switches pipeline
at most once between them. Transparent instances are not batched: their
order is their depth.

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

## Tests

`crates/fynkel_render/tests/shading.rs` — one pixel test per feature:
manual/auto exposure, no light = black + point light side and range +
the light budget, spot cone, ambient colour and the default
`Environment`, orthographic vs perspective, the pipeline count, normal +
roughness maps on the fixture's textures (and the cooked tangents they
need — the `MESH` info assertion is in `fynkel_import/tests/cook.rs`),
alpha mask, double-sided back faces, emissive without light, sky backdrop
+ rotation; from V1 (b): the sun's PCF shadow on a slab (darker under the
cube, a soft edge, gone with `shadows: false`, a 512 map, a non-casting
sun), `cast_shadows` / `receive_shadows` per `MeshRenderer`, blend
materials compositing back to front whatever the record order, MSAA 4
blending silhouette edges (and 8 ≡ 4, 2 ≡ 1), render scale 0.5 / 0.75 /
2 presenting at the target size (and back to native pixel-identical),
the fixture project's `low` / `medium` / `high` profiles switching at
runtime (an unknown name refused, the override), debug lines on top and
depth tested. `tests/project.rs` renders the fixture project (rusted
robot with its swatch, normal and metallic-roughness maps under its
`Sun`) from the cache alone. `FYNKEL_RENDER_DUMP_DIR=<dir>` /
`FYNKEL_RENDER_DUMP=<file>` write the frames as PPM for eyeballing.
`src/exposure.rs`, `src/shadow.rs`, `src/quality.rs` and `src/debug.rs`
have unit tests for the meter, the light fit, the clamps and the
primitives. The M0 capture (`benchmarks/m0/results/2026-09-20-12859e1`)
shows the cube scene unchanged at MSAA 4 with the shadow pass: 122 / 120
frames per second, engine CPU ≈ 0.1 ms, GPU ≈ 0.7–1 ms.
