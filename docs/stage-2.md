# Stage 2 plan: a small scene you can play with

Status: accepted 2026-10-08, with the three answers in §5. Scope and rules:
[ADR-0001](decisions/0001-fynkel-2-code-first.md). Stage 1 is done
([report](stage-1-report.md)).

Stage 2 ends when this Dart program shows a textured ground and a glTF
model, which you can tap and drag, at the display rate on macOS, iOS and
Android:

```dart
final world = World();
final camera = world.spawn(Camera(), Transform(position: Vec3(0, 2, 6), lookAt: Vec3.zero));
world.spawn(Light.sun(), Transform(forward: Vec3(-0.4, -1, -0.3)));

final grass = await Texture2D.load(await bytesOf('assets/grass.png'));
world.spawn(
  Transform(scale: Vec3(20, 1, 20)),
  MeshRenderer(Mesh.plane(), Material(baseColorTexture: grass, uvScale: 10)),
);

final model = await Model.load(await bytesOf('assets/robot.glb'));
final robot = world.spawnModel(model, Transform(position: Vec3(0, 0, 0)));
final head = robot.find('Head');          // a child object, by its glTF name

FynkelView(
  world: world,
  onFrame: (dt, input) {
    if (input.tapped case final point?) {
      final hit = world.raycast(camera.rayThrough(point));
      if (hit?.object.isPartOf(robot) ?? false) head!.rotate(Vec3.up, 0.5);
    }
    camera.orbit(around: Vec3.zero, by: input.drag);   // in examples/garden, not the engine
  },
);
```

`bytesOf` is the app's own loading (Flutter's `rootBundle`, a file, the
network): the engine takes bytes and never touches files (rule 2: one
public API, no hidden setup).

Not in stage 2: more than one light, shadows, transparency (alpha
blending), skinning and animation, physics, audio, saving, texture
compression, Windows and Linux. Each comes back later on its own.

## 1. Lessons from Fynkel 1, and what we do differently

| Fynkel 1 | Fynkel 2 |
|---|---|
| glTF went through an offline import and cook pipeline: a 1,100-line importer, a cache, sub-asset keys and sidecar files, in a crate with 17 dependencies (FBX and audio included). | A game loads a `.glb` at run time from bytes. The loader depends only on `gltf` and `image` (PNG and JPEG only). Parsing and decoding never run on Flutter's UI thread or the render thread. A cooked format comes back only if load times measured on a phone demand it. |
| Hierarchy changes were pushed to the renderer each frame by walking descendants (about 1.85 ms a frame on a phone before tuning). | World transforms are computed in one pass in the extract that already runs every frame, parents before children, with no walks. A benchmark with 10,000 objects in deep and wide trees sets the budget (§3.1). |
| Input was 1,300 lines of Rust with bindings and action maps. | Game code is Dart, and Flutter already delivers pointer and key events. So input is a small Dart object read each frame (§3.4). The engine adds only what Flutter can't do: rays from the camera and raycasts against meshes. |
| Textures were compressed offline (ADR-0061, 4–5× smaller). | Plain RGBA8 with mipmaps in stage 2, with each texture's GPU memory measured and reported. Compression comes back when a real scene needs the memory. |

## 2. What's new, by crate

```
crates/
  fynkel_mesh/      NEW (step 2): CPU meshes: the 48-byte vertex, built-in shapes, checks, normals, MikkTSpace tangents
  fynkel_world/     parents, children and names, world transforms, recursive despawn; Texture and Model resources
  fynkel_render/    vertex format with UVs and tangents, custom meshes, textures and samplers, textured PBR, alpha mask
  fynkel_load/      NEW (step 3): PNG/JPEG decoding into plain data, and .glb parsing (step 4). No GPU, no world, no threads of its own.
  fynkel_ffi/       loading entry points, resources handed to the render worker, raycasts
packages/
  fynkel/           Texture2D, Model, Mesh.custom, parent/children/find, raycast, Ray
  fynkel_flutter/   FrameInput (pointers, taps, drags, keys) passed to onFrame
examples/
  garden/           the stage 2 acceptance app
```

`fynkel_mesh` (*added while building step 2*) holds triangle meshes on the
CPU. The renderer uploads them, the world keeps them (raycasts in step 5
read the triangles), and `fynkel_load` will produce them, so none of
those three has to know another. `fynkel_load` turns bytes into plain
values (`ImageData`, `ModelData`) and is tested without a GPU. Dependencies still only point down;
`fynkel_load` doesn't know the world or the renderer. *Settled in step 3:*
`fynkel_world` depends on `fynkel_load` (a world keeps `ImageData`), and
the renderer takes raw pixel slices, so it depends on neither.

## 3. Design decisions

### 3.1 Hierarchy

- Each object has at most one parent. `Transform` stays the local transform, relative to the parent. An object without a parent is a root.
- *Changed while building step 1:* parents and names are kept by the world beside the components, not as `Parent` and `Name` components. A parent component could be overwritten with `insert` without the cycle check or the parent's list of children knowing, and components are plain `Copy` values, which a name (a string) isn't. Through the API nothing differs: `object.parent`, `object.name`.
- `object.parent = other` (or `null`), `object.children`, `object.worldTransform` (read only). Setting a parent that would make a cycle is an error (`FY_STATUS_INVALID_VALUE`), never a hang.
- `object.despawn()` despawns its children too. That's what a game almost always wants, and the opposite (orphans left in place) is easy to write by hand.
- An object may have a name (not unique; `''` is no name). `object.find('Head')` searches its descendants, depth first. glTF nodes keep their names.
- `object.isPartOf(other)` is true for `other` itself and everything below it.
- World transforms are computed during the extract, in one pass over objects sorted parents first. The order is kept between frames and rebuilt only when a parent changes. Stage 1's test that a steady frame allocates nothing on the Rust heap must still pass.
- The budget, checked by a benchmark on the Mac and measured on both phones: 10,000 objects (one flat level, 100 × 100, and a chain 100 deep) extract in under 1 ms on the iPhone.
- `object.worldTransform` is read only. Its position and rotation are exact; its scale is approximate when a parent with an uneven scale has a rotated child, since the result is skewed (Unity's "lossy" scale).

### 3.2 Meshes and textures

- Vertices gain UVs and tangents (position, normal, tangent with handedness, UV: 48 bytes). Built-in shapes get both. When a glTF mesh has no tangents, they're generated with MikkTSpace, as glTF requires.
- `Mesh.custom(positions:, normals:, uvs:, indices:)` makes a mesh from code (rule 2: everything a loader can do, code can do too). Normals and tangents are computed when left out.
- *Settled in step 2:*
  - **Mesh objects.** A `Mesh` is an engine object of its own, made without a world and shared, not copied, by every world that uses it. `Mesh.raw` takes packed lists (including tangents), like Flutter's `Vertices.raw`. `Mesh.inBackground(() => ...)` builds one on a helper isolate.
  - **Why the helper isolate.** MikkTSpace takes about 1 µs per triangle, so a big mesh with UVs shouldn't be built on the UI thread. Textures in step 3 follow the same pattern.
  - **Computed normals.** They're weighted by angle, so they don't depend on how a face was cut into triangles. A shared vertex is smooth; a sharp edge needs a vertex per side.
  - **Generated tangents.** A vertex whose faces need different tangents (a mirrored texture's seam) is split.
  - **Tangent sign.** A tangent's `w` makes `cross(normal, tangent) * w` point up the image, as glTF normal maps expect.
  - **Built-in shapes.** Their tangents are worked out exactly, not generated, and a test checks that they agree with MikkTSpace.
- `Texture2D.load(bytes)` decodes PNG or JPEG on a helper isolate (in Rust, through FFI), then hands the pixels to the world. The render worker uploads them; the Dart UI thread never waits for decoding or for the GPU. `Texture2D.rgba(width, height, pixels)` makes one from code.
- Colour textures (base colour, emissive) are sRGB; data textures (normal, metallic-roughness, occlusion) are linear. The material slot decides, so the same image can't be read the wrong way.
- Mipmaps are built in linear light (*on the GPU, changed in step 3, below*), so a texture doesn't darken as it shrinks. A golden test checks a distant checkerboard is mid-grey, not darker.
- `Material` gains `baseColorTexture`, `normalTexture`, `metallicRoughnessTexture`, `occlusionTexture`, `emissive`, `emissiveTexture`, `uvScale`, and `alphaCutoff` for cut-out leaves and fences (glTF's MASK). One sampler: repeat, trilinear, anisotropic 8× where supported.
- Textures and meshes are freed when disposed, or when the world is, and the GPU memory is checked to come back.
- *Settled in step 3:*
  - **The Dart class is `Texture2D`**, Unity's name. `Texture` would clash with Flutter's `Texture` widget in every file that imports both.
  - **Mipmaps are built on the GPU, not the CPU** (changed from the plan). The material slot decides the colour space, so one decoded image may be needed both ways; CPU mips would need a chain per space, built before the first use. Instead each view uploads the base level and draws each smaller level from the one above with a bilinear pass (`mipmap.wgsl`). In an sRGB texture the hardware averages in linear light, so the distant checkerboard stays mid-grey: a version that averages the stored sRGB values measures 118 against 181 and fails the test.
  - **One GPU texture per (texture, colour space) a material uses.** The world keeps the decoded image, shared through `Arc`; the extract uploads each pair the first frame a material uses it. The view keeps it until the world lets the texture go, so switching a material between two textures doesn't upload again.
  - **Empty slots** read a 1×1 default: white for colour, occlusion and metallic-roughness, and a flat normal stored as 16-bit float so it's exactly (0, 0, 1). Every stage 1 golden matches unchanged with the textured shader.
  - **Cut-outs** (`alphaCutoff` > 0) are drawn by their own pipeline with alpha-to-coverage and sharpened alpha (`(a − cutoff) / fwidth(a) + 0.5`), so edges are smooth under MSAA and don't thin out in the distance. Opaque objects keep a pipeline without `discard`, so tile GPUs keep hidden-surface removal for them. Draws are sorted by (cut-out, textures, mesh), and the pass switches the pipeline and the texture bind group only when they change.
  - **`emissiveIntensity`** (0 to 10⁶) multiplies `emissive`, as a light's intensity does its colour.
  - **Freeing.** `Texture2D.dispose()` is refused (`inUse`, naming the Dart material) while a material in a world uses it, and `Mesh.dispose()` while an object does; a built-in shape can't be disposed. Each view frees the GPU copy on its next frame. `ViewStats` reports `textureBytes` and `meshBytes`, counted on the render worker.
  - **`Texture2D.load` copies the bytes into native memory** and sends only the address to the helper isolate. Sending the list itself copies it on the calling isolate, which took 12–41 ms for a 50 MB file; the native copy takes about 5 ms. A test runs a 1 ms timer through a 4096² load (170 ms on the Mac, the timer waiting at most 4–6 ms) and fails if it ever waits half the load: decoding on the calling isolate freezes it for all of it. The bound is a share of the load, not a frame's 16 ms, because on the loaded CI machine the timer alone was 27 ms late. Sending the list instead of the address costs too little next to the load for that test to see, so the copy is checked by hand (the numbers above).
  - **Known gap for step 4** (*closed in step 4, §3.3*). A mirrored object (negative scale on one axis) has its triangles' winding flipped, so back-face culling hides it. Its normals and tangents are already right; step 4 adds the front-face variant, since glTF files mirror with negative scale.

### 3.3 Models (glTF)

- `Model.load(bytes)` takes a `.glb` (one file, with everything inside). `.gltf` with separate files would need a way to fetch them; it comes later if wanted. Parsing runs on a helper isolate, like textures.
- A `Model` is a resource: its meshes, materials and textures are uploaded once. `world.spawnModel(model, transform)` makes one object per glTF node, with `Parent`, `Name`, `Transform` and `MeshRenderer`, and returns the root. Spawning the same model twice shares the meshes and textures.
- Supported: meshes (triangles; several primitives per mesh), the metallic-roughness material with all five textures, MASK alpha, node transforms and names, `KHR_texture_transform` and `KHR_materials_emissive_strength`. Everything else (skins, animations, morph targets, cameras, lights, BLEND) is skipped with one readable warning per kind, never an error.
- Tests: Khronos's glTF sample models load and render to golden images (Box, BoxTextured, NormalTangentMirrorTest, AlphaBlendModeTest's MASK part, a model with 100+ nodes), and broken files give readable errors.
- *Settled in step 4:*
  - **The parser is the `gltf` crate** (no default features; images go through `fynkel_load`'s own decoding, and nothing is read from files). A file whose `extensionsRequired` lists an extension we don't know is refused before validation, so the message names the extension. It costs 853 KB of release library (6.3 → 7.2 MB on the Mac), mostly its JSON parser; a smaller hand-written one is possible later if size matters.
  - **A root object is always made**, named after the glTF scene, with the file's top-level nodes below it, so a model with several top-level nodes (AlphaBlendModeTest has 9) is still one object to move or despawn. `spawnModel(model, transform)` places the root.
  - **A node with several primitives** gets an unnamed child per primitive, each with its own `MeshRenderer`; a node with one gets the renderer itself. So one object still means one mesh and one material.
  - **Primitives that read the same accessors share a mesh**, whichever nodes and materials use them: the tower's three glTF meshes (the same cube in three materials) are one mesh, and the truck's wheels one.
  - **Materials.** glTF's linear factors become sRGB `Color`s (`Color::from_linear`); `KHR_materials_emissive_strength` becomes `emissiveIntensity`. Values outside glTF's ranges (nothing in the format enforces them) are clamped, with a warning, rather than refused by the world later. A primitive without a material gets glTF's default (white, metallic 1, roughness 1). Dart materials gain `name`, and a loaded model's are named as in the file.
  - **UV transform per material**, not per slot: `uvScale` is now a `Vec2` (an API change from step 3's `double`) and `uvOffset` is new. The first textured slot's `KHR_texture_transform` is used for all; a material whose slots differ, or rotate, gets a warning.
  - **Double-sided materials** (`doubleSided`) are drawn without culling, and a back face's normal is flipped, so the back is lit like the front. **Mirrored objects** (a negative determinant) draw with clockwise front faces. Both are pipeline variants: 2 (opaque, cut-out) × 3 (front, mirrored, both sides), switched only between batches.
  - **BLEND materials are drawn opaque**, with a warning (decision 3: no transparency in stage 2).
  - **Warnings** are one line per kind of thing skipped, in the order met, each said once however often it's met: lights, cameras, animations, skins, morph targets, vertex colours, points and lines, extra UV sets, unsupported samplers (clamp, mirror, nearest), normal-map scale and occlusion strength, unusable transforms, BLEND, unlit, unknown extensions. Unusable normals or tangents are recomputed, with a warning.
  - **Loading** copies the bytes to native memory and reads them on a helper isolate, as `Texture2D.load` does; the main isolate then makes the `Mesh`, `Texture2D` and `Material` objects, which share the model's data. `model.dispose()` frees only the parsed model: what it spawned keeps drawing.

### 3.4 Input

- `onFrame(dt, input)` gets a `FrameInput`, read in the game's frame: the pointers down and where they are, this frame's taps and drag delta, scroll, and the keys held (Flutter's `LogicalKeyboardKey`), with `pressed`/`released` for this frame. Positions are in the view's logical pixels.
- This changes stage 1's `onFrame(dt)` to `onFrame(dt, input)`. hello is updated with it; there are no other users yet.
- `FynkelView` collects the events with a `Listener` and a `Focus`, so it works with mouse, touch, trackpad and keyboard alike. Flutter's own gesture widgets around a view keep working.
- `camera.rayThrough(point)` turns a point in the view into a world-space ray, using the view's real size. `world.raycast(ray)` returns the closest hit: the object, the point and the distance. It tests bounding boxes first, then triangles, on the CPU against the meshes it already has. A faster structure comes only if a measurement asks for it.
- Raycasts read the world as it is now, not the last frame drawn, so a tap acts on what the game just set.
- *Settled in step 5:*
  - **No physics.** A raycast tests meshes the world already holds, on the CPU. Physics engines cast against colliders (simple stand-in shapes); if physics comes, its raycast sits beside this one.
  - **What a ray hits is what a view draws**, the way it draws it: objects with a `Transform` and a `MeshRenderer` with both a mesh and a material; only front faces unless the material is double-sided; a mirrored object keeps its fronts, as the mirrored pipelines do. Cut-outs are hit as if solid (the texture isn't read). A hit gives the object, the point, the distance and the face normal, turned towards the ray.
  - **Box first, in the world.** Each object's mesh box is carried into the world (centre moved, half-size by the matrix's absolute values), which costs far less than inverting the matrix; only objects whose box the ray meets are inverted, and their triangles tested in their own space. That took a cast over 10,000 cubes from 210 to 85 µs on the Mac (§6). `MeshData` keeps its box, worked out when it's made.
  - **A `Raycaster` per world** keeps the world matrices between casts (the C world holds one), so a cast in a steady world allocates nothing.
  - **`camera.rayThrough(point)`** starts on the near plane, as nothing nearer is drawn. Its `point` is a `ViewPoint`: a position in logical pixels together with the view's size, which is what makes the aspect right. `FrameInput` gives pointer positions as `ViewPoint`s, so the snippet works as written. A test casts a ray through every pixel of a rendered, non-square frame from a camera on a turned parent and checks each hits the object drawn there.
  - **`FrameInput`** (package:fynkel_flutter) has `size`, `pointers` (id → `ViewPoint`), `taps` and `tapped` (the last), `drag` and `scroll` (`Offset`s), and `keys`, `pressed`, `released`. A **tap** is Flutter's: down and up within the tap slop (18 logical pixels for touch, 1 for a mouse), mouse primary button only, with no time limit. **`drag`** follows the first pointer still down; the move that takes it past the slop counts in full, so a gesture's drags add up to the whole movement. **`scroll`** is the wheel plus the trackpad's two-finger pan (turned to scroll the way Flutter's scroll views do); the view claims the wheel, so a list around it doesn't scroll too. The trackpad pan isn't claimed (that needs a gesture recognizer, which would compete with gesture widgets around the view). Pinch isn't read yet.
  - **Keys.** The view takes focus at first (`autofocus`, on by default) and whenever it's pressed, and keeps every key while focused, so arrows and space reach the game instead of moving focus or pressing a button. Repeats aren't presses. Losing focus releases the held keys, since their key-ups won't come.
  - **hello** keeps a game file that imports only package:fynkel, so its `main.dart` adapts `onFrame(dt, input)` to the game's `onFrame(dt)`.

### 3.5 Measuring

- Load times for each texture and model, on all three devices: time on the helper isolate and time until the first frame shows it.
- GPU memory per texture and per model, and it must come back: 10 cycles of loading and disposing the scene's model leave memory flat (the stage 1 rule: no more growth than Flutter alone).
- The garden scene at the display rate over 60 s on all three devices, with the same stats as stage 1.
- *Settled in step 6:*
  - **`Material.dispose()`** is new: a model couldn't be unloaded, because the world kept every material ever used, and a texture in use by a world's material can't be disposed. It takes the material out of every world (`fy_material_remove`), refused (`inUse`) while an object uses it; a disposed material can't be used again (`StateError`). To free all a model brought: despawn what it spawned, then dispose its materials, meshes and textures, then the model. The garden does that each cycle.
  - **`FynkelView.onViewStats`** is new: the engine's counts after each frame the view posts, for measuring. "Until the first frame shows it" is read from them: when the model is spawned, frames up to `posted` don't have it. Once more frames have been drawn since than older ones were left to draw, a newer one was drawn; the next frame Flutter takes is that one or newer. The rule is never early (four tests, five mutants killed); when frames were replaced before being drawn it can be a frame or two late, and on Android, where frames go to the screen as they're drawn, up to one.
  - **package:fynkel_measure** holds stage 1's measuring, shared instead of copied: the stats line, the memory marks and the baseline inside the app, and the tool (`dart run fynkel_measure:measure`, from the app's directory) outside it. The tool takes the app's name from its `pubspec.yaml`; a lifecycle point's kind is its name without its number, so each app names its own points (hello's are now `created 3`, `resized to 0.6 2`, …). The VM service extension is `ext.fynkel.read`.
  - **The tool reads the app's reports at the end** (`ext.fynkel.reports`), as an iPhone app's output only reaches the tool once it has connected, after the loads it reports. A third mode, `bench`, runs the app's timings (the garden's are in `lib/bench.dart`) and prints them. On Android the readings now show the Native Heap row too.
  - **The Kenney files needed fixing**, in `tools/garden_assets.py`, which says why: the character's texture is a separate file and its unlit extension is *required*, and the Nature Kit stores its palette's sRGB values where glTF wants linear ones, and calls every colour metal. Read as written, the props were pale; fixed, they match Kenney's previews. Our reader is right to read them by the spec.

## 4. Steps

Each step is built, tested, and merged on its own, in this order. Each
step's tests fail first where that makes sense.

**Step 1: hierarchy.** ✅ Parents, names, world transforms in the extract,
recursive despawn, cycles as errors, the Dart API, and the 10,000-object
benchmark. *Done when* the tests and the budget pass on the Mac. Numbers
in §6.

**Step 2: vertex format and custom meshes.** ✅ UVs and tangents,
MikkTSpace, `Mesh.custom`. *Done when* the golden images match (the cube is
re-blessed once, after you look at it) and a custom mesh renders. No
golden needed re-blessing: the shapes' positions and normals didn't
change. Numbers in §6.

**Step 3: textures.** ✅ `fynkel_load` decoding, GPU mipmaps,
`Texture2D`, material slots, sRGB vs linear, the sampler, cut-outs,
freeing. *Done when* the golden images pass (including the distant
checkerboard) and loading never blocks the UI thread (a test checks
Flutter frames during a load). The UI test runs a timer on the calling
isolate rather than Flutter frames: the same thing measured more finely,
without a widget test. Numbers in §6.

**Step 4: glTF.** ✅ `Model.load`, `spawnModel`, the sample-model goldens,
warnings for what's skipped. *Done when* every listed sample renders
correctly and spawning a model twice shares its GPU data. Eight goldens
(Box, BoxTextured, NormalTangentMirrorTest, AlphaBlendModeTest,
OrientationTest, CesiumMilkTruck, Fox in its bind pose, and a generated
138-node tower with a mirrored shape), rendered through the C interface
and two of them again from Dart. A second spawn of the truck leaves the
view's mesh and texture bytes unchanged. Numbers in §6.

**Step 5: input and raycasts.** ✅ `FrameInput`, `rayThrough`, `raycast`.
*Done when* widget tests drive taps, drags and keys, and raycasts hit the
right object in unit tests, including through a hierarchy. Ten widget
tests drive taps, drags, several fingers, a mouse, the wheel, a trackpad
and keys; raycasts are tested in Rust (scale, mirror, double-sided,
hierarchy, edges), through the C interface (a block of the tower model),
and in Dart against every pixel of a rendered frame. Numbers in §6; the
phones' come with the garden in step 6.

**Step 6: `examples/garden` on macOS, then iOS, then Android.** ✅ The
scene from the snippet: a textured ground, a model, tap and drag. *Done
when* it runs at the display rate on all three, loads in a measured and
written-down time, and passes the memory cycles.
Kenney's character among twelve of the Nature Kit's props (39 placed),
tap the character to turn its head, drag to go round, scroll to zoom. It
runs at 120 fps on the Mac and the iPhone and 60 on the Note10+ with no
dropped frames; the whole scene loads in 45–73 ms on the Mac (warm),
23 ms on the iPhone and is on screen 65–153 ms and 148 ms after the load
starts. The memory cycles pass on the Mac and the iPhone. On the Note10+
the native heap grows by about 16 MB over the first 14 cycles and then
stays flat; after that the app grows as Flutter alone does. Numbers,
and the phone numbers steps 1 and 5 owed, in §6.

**Step 7: the stage 2 check.** A short report, as in stage 1. *Done when*
it's committed and you have played the scene on each device.

## 5. Decisions (2026-10-08)

1. **The garden scene uses Kenney's CC0 character and props.** The
   Khronos sample models are used in the tests.
2. **Images are decoded in Rust** (the `image` crate, PNG and JPEG), so
   the engine and its headless tests load textures without Flutter.
3. **No transparency in stage 2**, only cut-outs (MASK). Blending waits
   for the stage that needs it.

## 6. Measurements

### Step 6: the garden (Mac, iPhone, Note10+; profile builds)

`dart run fynkel_measure:measure rate|lifecycle|bench` from
`examples/garden`. The Mac is the M3 Max in a 1600 × 1200 window, under
`caffeinate -du`. The iPhone is 1320 × 2868 at 120 Hz, wireless. The
Note10+ is 1080 × 2241 at 60 Hz.

**Frame rate**, 60 s:

| | Mac | iPhone | Note10+ |
|---|---:|---:|---:|
| fresh fps, median (min) | 120.0 (119) | 120.0 (117.7) | 59.9 (57.9) |
| posted / presented | 7,257 / 7,256 | 7,319 / 7,319 | 3,573 / 3,573 |
| dropped | 0 | 0 | 0 |
| slow Flutter frames | 0 | 0 | 275 (271 raster) |
| longest raster | 3.83 ms | 1.30 ms | 31 ms |
| Dart allocated per frame | 19.1 KB | 12.7 KB | 19.5 KB |

The Note10+'s slow rasters cost no frames, as in stage 1 (28–247 a
minute there).

**Loads**: the whole scene (the grass texture, twelve prop models and the
character, read and loaded at once), then the character alone, reloaded
in the memory cycles. "On screen" is from the start of the load to the
first frame on screen that has it (`onViewStats`, §3.5).

| | Mac (warm) | iPhone | Note10+ |
|---|---:|---:|---:|
| scene loaded and spawned | 45–73 ms | 22.6 ms | — |
| scene on screen | 65–153 ms | 148 ms | 0.44–1.3 s |
| character: load | 1.1–2.5 ms | 5.6–8.7 ms | — |
| character: on screen | 18–35 ms | 24–33 ms | — |

The first launch on the Mac after a build takes about 1 s (cold
caches). The scene's on-screen time at startup includes the view's own
start: it makes its GPU device on the UI isolate in its first tick, which
takes 4–61 ms on the Mac, 7 ms on the iPhone and 0.84–0.91 s on the
Note10+, and a load's last steps wait behind it. Making the device off the
UI isolate is a follow-up.

**Memory cycles**: the character unloaded and loaded again, with the
footprint after each (Android: PSS with swap). Growth over the cycles,
against Flutter alone (`--baseline`):

| | cycles | unloaded | loaded | baseline unloaded | baseline loaded |
|---|---:|---:|---:|---:|---:|
| Mac | 20 | 3 MB (after +7 MB once, at cycle 2) | 4.3 MB | 5.0 MB | 4.6 MB |
| iPhone | 10 | 9.4 MB | 4.5 MB | 4.9 MB | 5.0 MB |
| iPhone | 30 | 20.1 MB | 19.6 MB | 17.5 MB | 17.0 MB |
| Note10+ | 10 | 16.6 MB | 15.9 MB | 9.0 MB | 8.9 MB |
| Note10+ | 30 | 30.6 MB | 28.3 MB | 17.3 MB | 16.3 MB |

- GPU memory is flat on all three: each unload frees exactly the 5.33 MB the character's texture took (Metal 170–172 MB after each unload on the iPhone, Graphics 123.3 MB on the Note10+).
- On the iPhone, 10 cycles were inconclusive (one-off steps); over 30 the garden grows as Flutter alone does, within 3 MB.
- On the Note10+ the Native Heap row grows from 40.5 to 56.6 MB over the first 14 cycles and then stays within 56.2–58.0 MB to cycle 30. After cycle 14 the app grows 0.44 MB a cycle, against Flutter's own 0.34. So it's bounded: freed memory the allocator or the driver keeps, about 16 MB, not a leak. Each load decodes a 4 MB image on a pool thread.
- The engine alone on the Mac, without Flutter (1,000 cycles headless): no growth without drawing; 57 KB a cycle with drawing, slowing.
- A model disposed without its parts keeps its textures and meshes until the garbage collector finalizes them, and the collector doesn't know their size. Telling it (`externalSize` on the finalizers) is a follow-up.

**The phone numbers steps 1 and 5 owed** (`bench`, the app's
`lib/bench.dart`, on the UI isolate through package:fynkel): `frame()`
on a headless view with 10,000 objects (the extract and the handover),
median and slowest of 300; casts are best of 200.

| | Mac | iPhone | Note10+ |
|---|---:|---:|---:|
| extract, 100 × 100 wide | 0.30 / 0.72 ms | — | 2.01 / 2.31 ms |
| extract, 100 chains 100 deep | 0.30 / 0.37 ms | — | 2.56 / 3.27 ms |
| extract, no tree | 0.16 / 0.20 ms | — | 1.21 / 3.32 ms |
| raycast, 100 × 100 cubes, hit / miss | 83 / 83 µs | — | 560 / 386 µs |
| raycast, in chains of 100, hit / miss | 193 / 207 µs | — | 1,668 / 1,669 µs |
| raycast, the garden's tap | 3 µs | — | 22 µs |

The iPhone's column is still to come: twice, Xcode timed out starting
the app over the wireless connection after a good build.

The Mac's numbers match the Rust probes' (§6, steps 1 and 5), so the
Dart layer adds nothing measurable. The Note10+ is 6–8 times slower than
the Mac: 10,000 objects in trees take 2.5 ms of a 16.7 ms frame, and a tap
in the garden 22 µs.

### Step 5: raycasts (Mac, M3 Max, release build)

`cargo run --release -p fynkel_world --example raycast`, best of 200 casts
with one `Raycaster`:

| Scene | Objects | Triangles | Hit | Miss |
|---|---:|---:|---:|---:|
| 100 × 100 cubes, no tree | 10,000 | 120,000 | 83 µs | 86 µs |
| 100 × 100 cubes, 100 chains 100 deep | 10,000 | 120,000 | 194 µs | 199 µs |
| sphere 32 × 16 | 1 | 960 | 7.0 µs | 0.0 µs |
| sphere 256 × 128 | 1 | 65,024 | 416 µs | 0.0 µs |
| tower.glb | 139 | 1,500 | 2.7 µs | 2.5 µs |
| CesiumMilkTruck.glb | 10 | 3,624 | 12.6 µs | 0.2 µs |
| Fox.glb | 27 | 576 | 4.5 µs | 0.3 µs |

- Before the world-space box (inverting every object's matrix to test its box in its own space): 210 µs and 435 µs for the two grids. The deep grid's extra time is updating 10,000 world matrices each cast.
- A big mesh costs about 6 ns a triangle once the ray is in its box. A tap or two a frame is well inside a frame at these sizes; a structure over the triangles (or the objects) waits until a scene needs it, as planned.

### Step 4: glTF (Mac, M3 Max, release build)

Reading a `.glb` (`cargo run --release -p fynkel_load --example
load_model`, best of 5), which `Model.load` does on its helper isolate:

| Model | file | nodes | meshes | image pixels | read |
|---|---|---|---|---|---|
| Box | 1 KiB | 2 | 1 | 0 | 0.0 ms |
| BoxTextured | 5 KiB | 2 | 1 | 0.1 Mpx | 0.1 ms |
| Fox | 159 KiB | 26 | 1 | 1.0 Mpx | 5.3 ms |
| CesiumMilkTruck | 361 KiB | 6 | 4 | 4.2 Mpx | 10.0 ms |
| NormalTangentMirrorTest | 1.5 MiB | 1 | 1 | 12.6 Mpx | 16.1 ms |
| AlphaBlendModeTest | 2.8 MiB | 9 | 9 | 13.6 Mpx | 99.4 ms |
| tower (generated) | 23 KiB | 138 | 1 | 0 | 0.1 ms |

Decoding the images is nearly all of it (AlphaBlendModeTest's are big
PNGs; NormalTangentMirrorTest's are JPEGs), as step 3's numbers predict.

Spawning (`cargo run --release -p fynkel_world --example spawn_model`,
best of 20): the tower's 139 objects spawn and despawn in 24 µs, and
every object's world matrix, as each frame's extract computes them, takes
1.6 µs for one tower and 15 µs for ten (1,390 objects).

Spawning the truck a second time adds no mesh or texture bytes to the
view (`ViewStats`), from Rust and from Dart. The model tests pass on the
Mac, in the iOS Simulator and in the Android emulator. On the emulator
they found a race from step 3: the render worker published a frame
before writing its GPU byte counts, so a reader woken by the frame could
see the counts from before it. The counts are now written first.

### Step 1: hierarchy (Mac, M3 Max, release build)

The extract with 10,000 drawn cubes plus the stage 1 scene, steady frames
(`extract_probe` in `crates/fynkel_ffi/src/extract/tests.rs`, 500 frames,
three runs):

| Shape | first frame | median | slowest |
|---|---|---|---|
| 100 × 100 wide | 0.60–1.08 ms | 0.30–0.32 ms | 0.40–0.65 ms |
| 100 chains 100 deep | 0.53–0.79 ms | 0.31–0.32 ms | 0.57–0.68 ms |
| no tree (as in stage 1) | 0.20 ms | 0.18 ms | 0.27–0.59 ms |

So the hierarchy adds about 0.13 ms per frame for 10,000 objects in trees,
and costs nothing for objects in none. The first frame builds the order
(parents before children); after that it's rebuilt only when a parent
changes. The world matrices alone (`cargo run --release -p fynkel_world
--example hierarchy`, best of 5):

| Shape | linking 10,000 | first update | steady update | reading all |
|---|---|---|---|---|
| 100 × 100 wide | 0.43 ms | 0.44 ms | 0.19 ms | 0.07 ms |
| 100 chains 100 deep | 5.6 ms | 0.39 ms | 0.13 ms | 0.05 ms |
| no tree | 0 | 0 | 0 | 0.04 ms |

Linking deep chains costs more because each `set_parent` checks for a
cycle by walking up (O(depth)); 5.6 ms for 10,000 links is a one-off at
load. Despawning a loose object stays at 23 ns (stage 1: 18 ns): the
subtree walk allocates only when there are children.

A steady frame still allocates nothing on the calling thread, now with
the 1,000 cubes in chains of ten (`tests/alloc.rs`). A version of the
world matrices that rebuilt the order every frame fails that test.

The phones aren't measured yet: the Note10+ wasn't connected, and a Rust
test binary can't run on the iPhone. Both are measured in step 6, when
`examples/garden` shows the frame clock's time on screen.

### Step 2: vertex format and custom meshes (Mac, M3 Max, release build)

Building meshes (`cargo run --release -p fynkel_mesh --example build`,
best of 5, two runs):

| Mesh | time | vertices |
|---|---|---|
| cube | 0.002–0.003 ms | 24 |
| sphere 48 × 24 | 0.03–0.04 ms | 1,225 |
| plane | < 0.001 ms | 4 |
| 256 × 256 grid, positions only | 2.9–3.7 ms | 66,049 |
| the same, with normals given | 0.46–0.55 ms | 66,049 |
| the same, with UVs (tangents generated) | 115–120 ms | 66,049 |

- **MikkTSpace dominates.** It costs about 0.9 µs per triangle, which matches the reference implementation; `bevy_mikktspace` is a safe port of it, byte for byte.
- **Built-in shapes.** With generated tangents the sphere took 3 ms. With its tangents worked out exactly it takes 0.03 ms, so the shapes aren't generated.
- **Big meshes.** A mesh like the grid belongs in `Mesh.inBackground`; a Dart test builds a 128 × 128 terrain that way.

Every golden image from stage 1 matches unchanged with the 48-byte
vertices, on the Mac, in the iOS Simulator and in the Android emulator.
The new golden `custom_meshes` shows a pyramid with flat faces and an
octahedron with smooth ones, both with computed normals. It's made once
through the C interface and once from Dart, and the two match.

### Step 3: textures (Mac, M3 Max, release build)

Decoding (`cargo run --release -p fynkel_load --example decode`, best of
5, two runs; photo-like images made by the example):

| Image | file | decode |
|---|---|---|
| 512² PNG | 579 KiB | 1.7 ms |
| 512² JPEG (quality 85) | 54 KiB | 1.1 ms |
| 1024² PNG | 2.3 MiB | 6.6–6.9 ms |
| 1024² JPEG | 194 KiB | 4.0–4.1 ms |
| 2048² PNG | 9.0 MiB | 26.5–26.9 ms |
| 2048² JPEG | 703 KiB | 14.4–14.5 ms |

So a 2048² texture decoded on the UI thread would drop one or two frames
at 60 Hz; `Texture2D.load` decodes on a helper isolate. Uploading and
building the mipmaps happens on the render worker, not the UI thread.

GPU memory (`crates/fynkel_render/tests/texture_memory.rs`, macOS, the
process's dirty GPU memory from `footprint`): ten 2048² textures with
their mipmaps report 213.3 MB in `texture_bytes`, and hold 216.4 MB once
they settle. While they upload it peaks at 555 MB (staging copies and
the driver's upload pool, so a game shouldn't upload many big textures
in one frame), and after they're removed 1.3 MB is left. A texture's mipmaps add a third:
a 256² texture counts 349,524 bytes.

A steady frame with textured materials, in both colour spaces, still
allocates nothing on the calling thread (`tests/alloc.rs`).

New goldens: `textured_materials` (each slot, cut-outs and tiling),
`distant_checkerboard` (mips in linear light), and `textured_scene` (the
test card on the stage 1 cube and a tiled ground, made once through the
C interface and once from Dart, from PNG and from JPEG). All texture
tests pass on the Mac, in the iOS Simulator and in the Android emulator;
`tools/test-mobile.sh` now runs them too.
