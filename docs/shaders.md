# Writing a shader

A shader decides how a surface looks. Most materials need no shader of
their own: the built-in **Lit** (`fynkel.pbr@1`) covers colour, metal,
roughness, normal maps, emission and the three alpha modes. Write a
shader when a surface has to do something Lit cannot — burn away, move,
glow at its edges, change with time or from code.

A Fynkel shader is **one `.shader` file**, like a Unity `.shader`: a
short header, then plain [WGSL](https://www.w3.org/TR/WGSL/). You write
one function, `surface()`, that says what the surface is at a pixel —
its colour, how metal and rough it is, its normal, what it emits — and
the engine does the rest: the sun and its shadows, point and spot
lights, the sky's light, ambient occlusion, bloom and grading. A lit
shader is lit exactly as Lit is.

The decisions behind this page are in
[ADR-0031](decisions/ADR-0031-shader-assets.md). Three finished examples
are in the showcase: `examples/showcase/assets/shaders/` (dissolve,
water, rim light), each with a scene and a render.

## A first shader

```text
// A tinted surface that pulses.
shader "Custom/Pulse" {
    lighting lit
    properties {
        tint   color = (1, 0.4, 0.1, 1)   "The colour at its brightest"
        speed  float = 1  range(0, 10)    "Pulses per second"
    }
}

fn surface(in: SurfaceInput) -> Surface {
    var s = default_surface();
    let beat = 0.5 + 0.5 * sin(globals.time * material.speed * 6.2831853);
    s.base_color = material.tint.rgb;
    s.emission = material.tint.rgb * beat;
    return s;
}
```

- The **header** (`shader "…" { … }`) is what the editor and the
  materials see: the name in the Shader menu, how it is lit, and its
  **properties** — the fields a material of this shader shows in the
  Inspector.
- The **body** is WGSL. `material.<name>` reads a property, `globals`
  holds the time and the camera, and `surface()` returns a `Surface`.

## Making one

**In the editor.** In the Project window, *New ▸ Lit Shader* or *New ▸
Unlit Shader* (also in the right-click menu). You get a working shader
with a colour and a texture, ready to change.

- Double-click it (or *Open in IDE* on its page) to edit it in your IDE.
  Saving it recooks it, and every material using it updates.
- Its page in the Inspector shows what the header says, **Compiled** or
  its errors — click one to open the IDE at that line — and the
  properties.
- *Create Material* on its page makes a material of it, next to it.
- On any material, the **Shader** menu at the top picks the shader:
  *Lit* or one of yours, grouped by the slashes in their names. Values
  whose names both shaders have (`base_color` and `baseColor` count as
  one) are kept.

**By hand.** Write `<name>.shader` anywhere under `assets/`. It needs a
sidecar for its id: the editor writes one, or run
`fynkel asset init assets/shaders/pulse.shader`. A material names the
shader by that id:

```json
{
  "$schema": "https://fynkel.dev/schemas/material.schema.json",
  "formatVersion": 2,
  "id": "<a new uuid>",
  "name": "Pulse",
  "shader": { "asset": "<the shader's id from its .meta.json>" },
  "parameters": { "speed": 2 },
  "textures": { "base_map": { "texture": { "asset": "<a texture's id>" } } },
  "alpha_mode": "opaque"
}
```

Anything left out of `parameters` or `textures` takes the header's
default. `fynkel shader validate <file>` checks a shader alone;
`fynkel validate` checks the whole project, materials included.

## The header

```text
shader "Menu/Path/Name" {
    lighting lit            // lit (default) | unlit
    alpha opaque, mask      // the alpha modes a material may pick (default: all three)
    shadows on              // on (default) | off
    properties {
        name type [= default] [range(min, max)] [srgb | linear] [required] ["tooltip"]
    }
}
```

- **The name** is the menu path in the material's Shader menu. Slashes
  make submenus.
- **`lighting lit`** runs the engine's light over your surface.
  **`lighting unlit`** draws `base_color` as it is: no light, and
  `metallic`, `roughness`, `normal` and `emission` are not used.
- **`alpha`** lists the modes a material of this shader may use:
  - `opaque`: `alpha` is not used;
  - `mask`: a pixel whose `alpha` is under the material's cutoff is not
    drawn — a real hole, in the shadow too (Unity's *Cutout*);
  - `blend`: see-through, drawn after the solid objects, casting no
    shadow (Unity's *Transparent*).
- **`shadows off`**: objects with this shader cast no shadow. They still
  receive shadows.
- `//` and `/* */` comments work in the header too.

### Properties

One per line. The Inspector shows the name made readable (`rim_power` →
*Rim Power*) and the tooltip on hover.

| Type | In WGSL | Default when left out | In the Inspector |
| --- | --- | --- | --- |
| `float` | `f32` | 0 | number (a slider with a `range`) |
| `int` | `i32` | 0 | whole number |
| `bool` | `bool` | false | checkbox |
| `vec2`, `vec3`, `vec4` | `vec2<f32>` … | zeros | numbers |
| `color3` | `vec3<f32>` | white | colour picker, no alpha |
| `color` | `vec4<f32>` | white | colour picker with alpha |
| `texture` | see below | the fallback texel | texture picker |

- A default is a number, `true`/`false`, or a list in parentheses:
  `(0.2, 0.6, 1)`.
- Colours are **linear**, as everywhere in the engine. The picker shows
  them as you expect and stores them linear.
- `range(min, max)` limits what a material may set.
- A name starts with a lowercase letter and may not be a WGSL word or
  start with `fynkel`.
- At most **8 textures** and **256 bytes** of values (about sixteen
  `vec4`s).

### Textures

`name texture [= white | black | normal | gray] [srgb | linear] [required]`

- The default is the colour used when a material binds no texture:
  `white` (the default), `black`, `normal` (a flat normal) or `gray`.
- For each texture property you get `sample_<name>(uv)`, which returns a
  `vec4<f32>`, and the raw `t_<name>` and `s_<name>` if you want to call
  `textureSample…` yourself.
- `required`: a material must bind a texture here.

**Colour space.** A texture is either colour (sRGB: base colour,
emission) or data (linear: normal maps, masks, roughness). Which one it
is belongs to the **image**, not the shader: set it on the texture's
page in the editor (*Colour space: linear (data)*), or in its sidecar:

```json
"settings": { "colorSpace": "linear" }
```

A normal map read as sRGB tilts every normal the same way — the surface
goes dark and the highlights move. If a normal map looks wrong, check
this first. (The `srgb`/`linear` word on the property documents what
the shader expects.)

## The body

### `surface()` — required

```wgsl
fn surface(in: SurfaceInput) -> Surface
```

What you get, `SurfaceInput`:

| Field | What it is |
| --- | --- |
| `world_pos` | the pixel's position in the world |
| `world_normal` | the mesh's normal there, unit length (turned to face the camera on the back of a double-sided material) |
| `view_dir` | unit vector from the surface toward the camera |
| `uv0`, `uv1` | the mesh's two UV sets |
| `color` | the vertex colour |
| `screen_uv` | 0…1 across the screen, (0, 0) at the top left |
| `front_facing` | whether you see the front of the triangle |

What you return, `Surface` (start from `default_surface()`, which is
Lit's defaults):

| Field | Default | What it does |
| --- | --- | --- |
| `base_color` | white | the colour, linear RGB |
| `alpha` | 1 | used by `mask` and `blend` |
| `metallic` | 0 | 0 plastic, wood, stone … 1 metal |
| `roughness` | 0.5 | 0 mirror … 1 matte (as glTF; Unity's *smoothness* is `1 − roughness`) |
| `normal` | (0, 0, 1) | in **tangent space**: (0, 0, 1) is the mesh's own normal; a normal map's texel is `rgb * 2 − 1` |
| `emission` | black | light the surface gives off, relative to white; above 1 glows with bloom |
| `occlusion` | 1 | darkens the sky's and the ambient light (not the sun's) |

### `vertex()` — optional

```wgsl
fn vertex(in: VertexInput) -> VertexInput
```

Moves a vertex before the engine places it: `position`, `normal`,
`tangent`, `uv0`, `uv1` and `color`, all in the **object's own space**.
Use it for waves, wind and swaying. The shadow and the selection
outline run it too, so a waving flag's shadow waves with it.

Only vertices move: a plane with few triangles bends only at its
corners. And in `vertex()` a texture is read with
`textureSampleLevel(t_<name>, s_<name>, uv, 0.0)`, since `sample_<name>`
works only in `surface()`.

### What else you can use

- **`material.<name>`**: every value property, with its WGSL type.
- **`globals.time`**: seconds of the world's clock (Unity's `_Time.y`);
  it stops while Play is paused. **`globals.delta_time`**: seconds since
  the last frame. **`globals.camera_pos`**: the camera, in the world.
  **`globals.screen_size`**: in pixels.
- Your own functions, constants and structs, above or below
  `surface()`. Only names starting with `fynkel` and the engine's own
  (the compiler says which if you hit one) are taken.

## From code

A game changes a shader material from Dart as Unity does with
`material.SetFloat`:

```dart
// A material of a .shader made at runtime (Unity's `new Material(shader)`).
final water = world.createShaderMaterial(
  'shaders/water.shader',                    // or the shader's AssetRef
  values: {'deep': Vec4(0.02, 0.1, 0.2, 0.8), 'flow': Vec2(0.05, 0.02)},
  alphaMode: AlphaMode.blend,
);
pond.set(pond.get<MeshRenderer>()!.copyWith(material: water));

// Change one property, every frame if you like: only its values are rewritten.
world.setMaterialValue(water, 'flow', Vec2(0.1, 0));
world.setMaterialValue(crateMaterial, 'amount', t);   // a material from a file works too
world.setMaterialValue(water, 'flow', null);          // back to the shader's default

world.materialValue(water, 'flow');                   // what code set, or null
world.destroyMaterial(water);                         // when a made material is done
```

- A value is a number, a `bool`, a `Vec2`/`Vec3`/`Vec4` (a colour is a
  `Vec3` or `Vec4`, linear) or an `AssetRef` for a texture.
- A wrong name, type or value outside the `range` throws an
  `ArgumentError` with the same message the cook would give, and the
  material keeps its values.
- `setMaterialValue` changes the material itself: every object using it
  changes. Give an object its own material to change it alone.
- The change lasts for this run. The material file keeps its values.

The showcase's `game.Dissolve` (`examples/showcase/lib/game/dissolve.dart`)
is a complete example: it burns a crate away and back by setting
`amount` every frame.

## When something is wrong

- A shader that does not compile lists its errors with their lines on
  its page, in the Console and from `fynkel shader validate`. The error
  is at the line in your `.shader` file, header and body alike.
- Its materials draw **magenta** (Unity's pink) until it compiles again.
  A material the shader has no longer (a property you removed, a value
  outside the new range) is a validation error on the material.
- A shader's error never stops the rest of the scene from drawing.

## Not yet

- **A Shader Graph** (nodes instead of code). It will come as a package
  that writes the same `.shader` file, so shaders written by hand and by
  graph work the same way.
- Custom lighting models, more than one pass, and reading the screen or
  the depth behind a surface (refraction, soft particles).
- Writing a shader's source from code at runtime: shaders are assets,
  cooked and checked before the game runs.

## See also

- [`documents.md`](documents.md) § Material and shader documents — the
  exact file formats and every diagnostic code.
- [`renderer.md`](renderer.md) § Shader assets — how the renderer
  compiles and draws them.
- [ADR-0031](decisions/ADR-0031-shader-assets.md) — why it works this
  way.
