# ADR-0031: Shader assets — one `.shader` file, a surface function in WGSL, drawn by the engine's own lighting

**Status:** Accepted — proposed by the assistant 2026-09-26 after the user's "let's continue with the shader assets"; the two-file draft was rejected by the user ("i do not like the two files per shader"); one file, code use and the graph path talked through; confirmed by the user 2026-09-26 ("ok let's go with that")  
**Date:** 2026-09-26  
**Related:** ADR-0012 (wire records), ADR-0016 (the authoring service), ADR-0026 (code-first building), ADR-0028 (editor extensions), ADR-0029 (packages), ADR-0030 (Inspector previews), blueprint § Materials and shaders, `docs/documents.md` § Material and shader documents, `docs/ideas.md` § 24.12 (shader graph), `docs/renderer.md`

## Context

On 2026-09-22 the user asked for a Unity-like shader → material system,
with a Shader-Graph-like node editor later as a package. The rendering
arc (R1–R3d) and the package architecture (ADR-0029) are done, so
shaders are next.

**How Unity does it.**
- A **shader** is one asset file. It has a **properties** block (floats,
  colours, vectors, textures) and the code that uses them.
- A **material** picks its shader from a dropdown at the top of its
  Inspector, then shows that shader's properties as fields.
- Code creates materials with `new Material(shader)` and drives them with
  `SetFloat`/`SetColor`.
- In URP most shaders are Shader Graphs. The *master stack* is a fixed
  list of outputs: Base Color, Normal, Metallic, Smoothness, Emission,
  Ambient Occlusion, Alpha. URP does the lighting.

**The other engines** split the work the same way. Unreal's Material
Editor feeds Base Color, Metallic and Roughness. Godot is closest to this
ADR: one `.gdshader` file of `uniform` properties with hints, and a
`fragment()` function that sets `ALBEDO`, `ROUGHNESS` and so on. Its
visual shader editor generates that same code. In every case the user
describes the **surface** and the engine does the **light**.

**What Fynkel has today** (mapped 2026-09-26):
- **An interface manifest.** `.shader.json` holds `shaderId`, a `source`,
  `passes`, typed `parameters` with defaults and ranges, and `textures`
  slots with colour space and a fallback texel. The built-in
  `fynkel.pbr@1` ships one inside the engine.
- **Material validation for custom shaders** (`FYN-D0300`–`D0312`).
  Materials name their shader by the string `name@version`.
- **Only test fixtures use either.**
- **No cooked record.** A custom-shader material has none
  (`docs/status.md` § Known limits).
- **One hard-wired shader.** The renderer draws only its PBR shader
  (`mesh.wgsl`).
- **No custom materials in the editor.** It edits only PBR materials.
- **No time in the frame.** The frame uniform has no time, so no shader
  could animate.

## Decision

### 1. The contract: an interface and a surface function

What the engine cooks, draws and gives to materials is always the same
pair:

- **an interface**: the properties (name, type, default, range, tooltip),
  the texture slots, the lighting, which alpha modes are allowed, and
  whether it casts shadows. This is the existing manifest type
  (`ShaderManifest`), now filled from the header below and kept inside
  the engine;
- **WGSL** that defines `surface()` and, optionally, `vertex()` against
  the versioned contract **`surface@1`**.

The `.shader` file (§ 2) is how a person writes that pair. A future
**Shader Graph** (§ 8) will be a second way to produce the same pair.
The cooker, the renderer, materials and the code API see only the pair,
so a graph shader is never second-class. There is one text format, not
two.

### 2. One file: `<name>.shader`

A short header, then plain WGSL:

```
shader "Examples/Water" {
    lighting lit
    alpha opaque, blend
    properties {
        deep      color    = (0.02, 0.10, 0.20, 1)   "Colour of deep water"
        flow      vec2     = (0.05, 0.02)
        roughness float    = 0.1  range(0, 1)
        ripples   texture  = normal
    }
}

fn surface(in: SurfaceInput) -> Surface {
    var s = default_surface();
    let n = sample_ripples(in.uv0 + globals.time * material.flow);
    s.base_color = material.deep.rgb;
    s.alpha = material.deep.a;
    s.normal = n.xyz * 2.0 - 1.0;
    s.roughness = material.roughness;
    return s;
}
```

**The header.**
- `shader "<menu path>"` is the name shown in the material's Shader
  dropdown. Slashes make submenus, as in Unity.
- `lighting lit | unlit`. Default `lit`.
- `alpha` lists the alpha modes a material may choose, from `opaque`,
  `mask` and `blend`. By default all three are allowed.
- `shadows on | off`. Default `on`. A `blend` material never casts, as
  today.
- `properties { … }` holds one line per property:
  - `name type [= default] [range(min, max)] ["tooltip"]`;
  - the name is a WGSL identifier;
  - the Inspector shows it nicified (`roughness` → Roughness, as in
    Unity).

**Property types.**
- The values: `float int bool vec2 vec3 vec4 color3 color`. `color` is
  RGBA and is shown with the colour picker.
- `texture`, whose default is the fallback texel: `white`, `black`,
  `normal` or `gray`.
  - Texture options: `srgb | linear` (default `srgb`; `linear` for
    `normal`) and `required`.
  - Each texture property is a slot.

**The body.** Everything after the header is WGSL. `//` and `/* */`
comments work in the header too.

**Identity.** A `.shader` file is an asset with a `.meta.json` sidecar
(the automatic sidecars), so it has a stable id. A material names its
shader by **asset id**, as Unity uses a GUID. Renaming or moving a shader
never breaks its materials. The built-in keeps its string `fynkel.pbr@1`,
so a material's `shader` is either that string or an asset reference.

**Migration.** `.shader.json` stops being a project document (only
fixtures used it). *Amended 2026-09-26 (step 2):* no `fynkel migrate`
step — a `.shader.json` in a project is `FYN-D0015`, and its suggestion
is the `.shader` header that says the same; a material naming a custom
`name@version` is `FYN-D0300`, whose suggestion shows the asset
reference. A converter would have had no real file to convert.

### 3. What the user's code sees (`surface@1`)

The engine writes a *prelude* above the body and the real entry points
below it. The user never writes `@vertex`, `@fragment`, bindings or
light code.

- **`fn surface(in: SurfaceInput) -> Surface`**: required.
  - `SurfaceInput` holds `world_pos`, `world_normal`, `view_dir`, `uv0`,
    `uv1`, `color` (vertex colour), `screen_uv` and `front_facing`.
  - `Surface` holds:
    - `base_color` (linear) and `alpha`;
    - `metallic` and `roughness` (roughness as glTF, not Unity's
      smoothness);
    - `normal` (tangent space, default `(0, 0, 1)`);
    - `emission` (relative to white, as the PBR emissive);
    - `occlusion`.

    `default_surface()` gives the PBR defaults.
- **`fn vertex(in: VertexInput) -> VertexInput`**: optional. It moves the
  vertex's `position` and `normal` in object space before the engine
  places it. The depth, shadow and selection passes run it too, so a
  waving flag's shadow and outline wave with it.
- **`material`**: a uniform struct generated from the properties, with
  the same names.
- **Textures, one set per texture property**: `sample_<name>(uv)`, plus
  the raw `t_<name>` and `s_<name>`. An sRGB slot arrives linear.
- **`globals`**: `time`, `delta_time`, `camera_pos` and `screen_size`
  (Unity's `_Time` and friends). The renderer's frame gains `time` and
  `delta_time` from the world clock, so a paused Play pauses them.

**`lit`** runs exactly the built-in PBR's light:
- the cascaded sun shadow, contact shadows, and the object's point and
  spot lights;
- the IBL or ambient light with the screen-space AO;
- then bloom and grading in post.

To share it, `mesh.wgsl` is split into a lighting library and the
built-in's surface. **The built-in's pixels must not change**: the
existing pixel tests hold it to the bit.

**`unlit`** writes `base_color` as today's unlit path does.

**`alpha_mode`** picks the pass, as for PBR materials. `mask` discards
under the cutoff using `Surface.alpha`, and `blend` draws in the
transparent pass.

A change that could break user code is **`surface@2`**. `surface@1`
keeps working or is migrated.

### 4. One crate reads and checks shaders: `fynkel_shader`

A new crate that depends only on `naga` (already under `wgpu`). It
holds:

- **The parser.** The header's parser gives errors with line and column.
- **The contract.** The `surface@1` prelude and entry points.
- **The assembler.** Interface + body → one WGSL module, with a line map
  back to the `.shader` file.
- **The parameter layout**, explicit and following WGSL's uniform rules:
  - properties in file order;
  - `float`, `int` and `bool` take 4 bytes, `vec2` 8;
  - `vec3`/`color3` take 16 bytes aligned to 16, and `vec4`/`color`
    take 16;
  - the whole block is **at most 256 bytes**.
- **The limits.** At most **8 texture properties**, so group 1 is one
  uniform, 8 textures and 8 samplers, under Metal's 16-sampler limit on
  phones.
- **Validation.**
  - `naga` checks the assembled module. Its errors are **`FYN-D0313`**,
    placed at the line in the `.shader` file.
  - A header error is **`FYN-D0315`**.
  - A missing or mis-typed `surface`/`vertex` is **`FYN-D0314`**.
  - The existing interface rules (`D0310`–`D0312`) apply to the parsed
    header.

**Users of the crate:**
- the cooker, for the check;
- the renderer, to build the same module;
- `fynkel shader validate`;
- later, the graph.

None of them can disagree about what a shader is.

### 5. Cooked records

- **`SHDR`**, for a shader. It holds:
  - the interface (property offsets and types, slots with colour space
    and fallback, lighting, alpha modes, shadows);
  - the contract version;
  - the body.

  The cook refuses a shader that does not compile. The runtime never
  meets broken code from the cache.
- **`MATC`**, for a custom material. It holds:
  - the shader's asset id;
  - the property block, already in GPU layout, with defaults filled;
  - a texture ref and sampler settings per slot;
  - `alpha_mode`, `alpha_cutoff` and `double_sided`.

  The shader is a dependency, so editing it recooks its materials.

The cache version goes up. The wire fingerprint and the Apple ABI stay
unchanged: these are cooked chunks, not wire records.

### 6. The renderer

- **Pipelines per shader.** A shader's pipelines are built when its first
  material loads, and kept while any material uses it. There is one
  pipeline per variant:
  - skinned or not;
  - single- or double-sided;
  - the pass: scene, depth prepass, shadow, or selection mask.
- **Bind groups.** Group 1 is per material: the property uniform, the
  textures, the samplers. Groups 0 and 2 (frame, skin, lights) are the
  ones every mesh uses.
- **Batching.** Draws batch by (mesh, material) as today.
- **The error shader.** A material whose shader is missing or failed
  draws in magenta, like Unity's pink.

### 7. Using shaders: files, the CLI, code and the editor

**No editor is needed.** A `.shader` is a text file:
- write it in any editor;
- `fynkel shader validate <file>` checks it;
- `fynkel cook` checks every shader in the project, so CI catches broken
  ones;
- `fynkel render` and `fynkel run` show it.

**From Dart code** (ADR-0026), Unity's `new Material(shader)` and
`SetFloat`:

```dart
final water = world.createMaterial(
  shader: 'shaders/water.shader',
  values: {'deep': Color(0.02, 0.1, 0.2), 'roughness': 0.1},
);
pond.meshRenderer.material = water;

crate.material.set('dissolve', t);   // every frame
```

- **Where a value goes.** `material.set` changes that material, and every
  object using it sees the change, as in Unity.
- **Per-object values.** Unity's `renderer.material` (a per-object
  copy) and MaterialPropertyBlock stay out of scope.
- **Checking.** Values are checked against the interface (type, range)
  at the call, with the same diagnostics as the documents.
- **Runtime changes.** A runtime `set` rewrites the material's uniform
  only. No recook, no new pipeline.
- **No shader source at runtime.** Code picks shaders, creates materials
  and changes values. It never compiles shader source at runtime:
  - errors belong at the cook, with a line number;
  - phones should not pay compile time at start-up;
  - Unity does not allow it either.

**The editor, as Unity:**
- **Project ▸ Create ▸ Shader ▸ Lit / Unlit** writes one file from a
  template. Double-click opens it in the IDE.
- **The shader's Inspector page** shows:
  - the header's facts and the properties table;
  - *Compiled*, or the errors, each opening the IDE at its line;
  - **Create Material**.
- **The material Inspector:**
  - a **Shader dropdown** at the top, by menu path: the built-in Lit,
    the project's shaders, then the packages' shaders;
  - the properties as fields: a slider for a ranged float, the colour
    picker, X Y Z W, a toggle, a texture field you can drop a texture on;
  - **switching shader keeps same-named values**. The PBR fields map to
    the built-in's names (`base_color` ↔ `baseColor`, …).
- **Everything else follows.** The material preview (ADR-0030) draws
  custom materials. Saving the file recooks it, the Scene, Game and
  preview update, and the errors go to the Console.
- **Packages** ship shaders under `Packages/<name>/` (ADR-0029).

### 8. The Shader Graph fits later, on the same contract

Its own ADR, as a package with its own editor window (ADR-0028,
ADR-0029). It works as Unity's does, by generating code:

- **The asset.** A `.shadergraph` asset holds nodes, wires and a
  Blackboard of properties.
- **Nodes are WGSL snippets.** Multiply is `a * b`. Sample Texture calls
  `sample_<name>`. Time reads `globals.time`. Fresnel is a helper. A
  **Custom Function** node takes your own WGSL.
- **Compiling.** It walks back from the master stack (the `Surface`
  fields) and writes the `surface()` body. The Blackboard becomes the
  interface.
- **The result** is the same pair as § 1. It is cooked, drawn and used
  exactly like a `.shader`.
- **Node previews** use the ADR-0030 preview stage.
- **Show Generated Code** prints a `.shader` you can keep.

Nothing in this ADR has to change for it.

### 9. Out of scope

These are later, beside the surface shader, not replacing it:

- **Your own lighting model**: `lighting custom` with a `light()`
  function, as Godot does.
- **Other kinds of shader**: post-processing and full-screen shaders,
  compute, custom render passes.
- **Variants and keywords**: the interface's `features` stays reserved.
- **Per-object overrides**: per-object material copies and property
  blocks.
- **Tessellation and geometry stages.**
- **A Fynkel syntax mode for VS Code.** Until then, mapping `.shader` to
  WGSL gives highlighting for the body.

## Alternatives considered

- **Two files** (the `.shader.json` manifest and a `.wgsl`). This was the
  first draft, and the user rejected it. Properties and code change
  together, so they belong in one file, and the Project would have had
  to hide the pair as one asset.
- **One pure `.wgsl` file,** with properties read from a WGSL `struct`.
  Rejected. WGSL has no place for defaults, ranges, colour spaces or
  fallbacks, so they would live in special comments: fragile, with poor
  errors.
- **Supporting both formats.** Rejected. It would mean two parsers, two
  sets of errors and docs, and a "which do I use?" with no good answer.
  A second format can still be added later if one is needed. Removing
  one people use cannot.
- **Whole shaders**, where the user writes `@vertex`/`@fragment` and
  binds everything. Rejected. Every shader would re-implement cascades,
  lights, IBL, AO and contact shadows, and break when they change.
- **Compiling only at runtime.** Rejected. Errors would show as a magenta
  object in the game instead of a line number on save.
- **Rewriting the built-in PBR as a surface shader now.** Deferred.
  - Its per-instance factors are what let every PBR object share one
    pipeline.
  - A test instead holds a PBR-copy `.shader` close to the built-in's
    pixels.

## Consequences

- **Users** write water, dissolve, scrolling, rim and fresnel effects in
  about twenty lines, with no light code. From Dart, they create
  materials and drive their values.
- **Lighting work pays twice.** Every improvement to shadows, lights or
  AO reaches every custom shader.
- **New code:**
  - a new crate, `fynkel_shader`;
  - a new asset kind, `.shader`, with a migration from `.shader.json`;
  - material `shader` as an asset reference;
  - `mesh.wgsl` split, and the frame grown by `time` and `delta_time`;
  - new chunks and a cache version bump.

  These are shared-crate touches, so they go in their own commits.
- **Pipeline creation takes time** on the first load of each shader (Metal
  compiles in milliseconds to tens of milliseconds). A pipeline cache can
  come when a capture shows the need.
- **Other hosts.** WGSL through `naga` reaches Metal today and Vulkan,
  D3D12 and WebGPU later. There is no second shader language.

## Order

1. **`fynkel_shader`:**
   - the header parser, the `surface@1` contract, the assembler, the
     layout and the limits;
   - `naga` validation mapped to file lines;
   - `D0313`–`D0315`.
2. **The asset:**
   - `.shader` as an asset kind with a sidecar;
   - the material's `shader` as an asset reference, validated through
     the parsed interface;
   - `fynkel shader validate`/`inspect` on `.shader`;
   - a `.shader.json` refused with its header as the suggestion
     (amended, see § 2);
   - the fixtures moved over.
3. **Split `mesh.wgsl`** into the lighting library and the built-in's
   surface, and add `time`/`delta_time` to the frame. Every existing
   pixel test must stay identical.
4. **The cook:** `SHDR` and `MATC`, the dependencies, the cache version.
5. **The renderer:**
   - pipelines per shader, per-material bind groups, every pass, the
     error shader;
   - pixel tests: an unlit colour, a property and a texture moving the
     pixels, `vertex` moving the shadow, the error colour, and a PBR-copy
     shader close to the built-in.
6. **Code:** `createMaterial(shader:, values:)` and `material.set(…)` in
   `package:fynkel`, with the runtime uniform write.
7. **The editor:**
   - Create ▸ Shader and the shader's page with its errors;
   - the material Inspector's Shader dropdown and fields;
   - recooking on save.
8. **Examples:** dissolve (mask, noise, a glowing edge, driven from code),
   water (time, a scrolling normal) and rim light (fresnel emission) in
   the showcase, each with a committed render.
9. **Docs:** `docs/documents.md`, `docs/renderer.md`, and a "Writing a
   shader" page. Then the user's check in the app.
