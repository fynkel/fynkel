# Renderer

`crates/fynkel_render` — what a frame is and how it is shaded. Blueprint §8.5;
renderer V1 (a) is §19.2d item 4 (M3-04). The renderer reads one thing, the
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

1. **HDR pass** into `HDR_FORMAT` (`Rgba16Float`) + `Depth32Float`, both
   cleared (colour to transparent black). Opaque meshes, culled batches
   first, then double-sided ones. Then the **sky**, when the
   `Environment` names a texture: a fullscreen triangle at depth 1 with
   `LessEqual`, so only untouched texels get the equirectangular lookup
   (`u = 0.5 + atan2(d.x, −d.z) / 2π` — `u = 0.5` looks down −Z, `v` from
   up to down; `sky_rotation` turns the sky about +Y) × `sky_intensity`.
2. **Post pass** into the host's target: exposure → ACES (Stephen Hill's
   fit of the RRT + sRGB ODT) → sRGB encode unless the target format is an
   sRGB format (then the hardware encodes). The HDR alpha channel is a
   tag, not coverage: `0` background → the exact `CLEAR_COLOR` bytes
   (`clear_rgba8()`; what the pixel tests compare against), `1`
   scene-referred radiance → exposed and tone mapped, `2`
   display-referred → clamped and encoded only (unlit: the magenta
   missing-mesh cube).

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

Every material map is sampled with one repeat/linear sampler at `uv0`:

| Map | Use | Fallback |
| --- | --- | --- |
| `base_color_texture` (sRGB) | × `base_color` × vertex colour; alpha feeds the mask | white |
| `normal_texture` (linear) | tangent-space normal, `xy` × `normal_scale`, in the frame `(t, b = (n × t) · w, n)` from the cooked vertex tangent; a zero tangent (no UVs at cook) keeps the vertex normal | flat (128, 128, 255) |
| `metallic_roughness_texture` (linear) | `B` × `metallic`, `G` × `roughness` (glTF layout) | white |
| `occlusion_texture` (linear) | `1 + occlusion_strength · (R − 1)` on the ambient term | white |
| `emissive_texture` (sRGB) | × `emissive` | white |

`alpha_mode: mask` discards texels whose alpha is below `alpha_cutoff`
inside the opaque pass; `blend` is drawn opaque until renderer V1 (b)
(item 5: transparency, shadows, MSAA, quality profiles). `double_sided`
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
culling or clustering yet — V1 (b) revisits the budget when shadows
arrive. One sun (the first `DirectionalLight`).

## Uniform layout

`Frame` (bind group 0, 736 bytes, `shaders/frame.wgsl`): `view_proj`,
`inv_view_proj` (the sky's ray reconstruction), `camera_pos` (`w` =
exposure factor `1 / L_max`), `sun_dir` (`w` = present), `sun_color` (rgb
× lux), `ambient` (rgb × lux, `w` = `L_max`), `sky` (`x` intensity, `y`
rotation), `counts` (`x` lights, `y` sky drawn, `z` encode sRGB), then
`lights: array<Light, 8>` — `position` (`w` range), `color` (rgb × cd,
`w` 0 point / 1 spot), `direction` (`w` cos outer), `params` (`x` cos
inner).

Per instance (vertex buffer 1, 128 bytes, locations 6–13): model matrix,
`tint` (base colour factor), `emissive` (rgb × strength), `params`
(metallic, roughness, normal_scale, occlusion_strength), `flags` (alpha
cutoff or −1, unlit, double-sided, 0).

## Pipelines and caches — the variant cap

Exactly **four** render pipelines exist, all created in `new`
(`PIPELINE_VARIANTS`); nothing is compiled at draw time:

| Pipeline | Target | Culling | Depth |
| --- | --- | --- | --- |
| `mesh` | HDR | back | write, `Less` |
| `mesh_double_sided` | HDR | none | write, `Less` |
| `sky` | HDR | none | read, `LessEqual` |
| `post` | host format | none | none |

Material features (maps present or not, mask, unlit) are data — instance
flags and fallback textures — not variants. A new variant needs a row in
this table. Bind groups are cached per material `AssetRef` (five texture
views + the sampler, the fallback view for every unbound slot) and per
texture; the post bind group is rebuilt on `resize` (it holds the HDR
view), the sky bind group when the `Environment` names a different
texture. Instances are batched by `(mesh, material)` in order of first
appearance; the culled batches draw before the double-sided ones, so a
frame switches pipeline at most once between them.

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
+ rotation. `tests/project.rs` renders the fixture project (rusted robot
with its swatch, normal and metallic-roughness maps under its `Sun`) from
the cache alone. `FYNKEL_RENDER_DUMP_DIR=<dir>` / `FYNKEL_RENDER_DUMP=<file>`
write the frames as PPM for eyeballing. `src/exposure.rs` has the unit
tests for the meter.
