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

final grass = await Texture.load(await bytesOf('assets/grass.png'));
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
  fynkel_world/     Parent and Name components, world transforms, recursive despawn; Texture and Model resources
  fynkel_render/    vertex format with UVs and tangents, custom meshes, textures and samplers, textured PBR, alpha mask
  fynkel_load/      NEW: PNG/JPEG decoding + mipmaps, and .glb parsing into plain data. No GPU, no world, no threads of its own.
  fynkel_ffi/       loading entry points, resources handed to the render worker, raycasts
packages/
  fynkel/           Texture, Model, Mesh.custom, parent/children/find, raycast, Ray
  fynkel_flutter/   FrameInput (pointers, taps, drags, keys) passed to onFrame
examples/
  garden/           the stage 2 acceptance app
```

`fynkel_load` turns bytes into plain values (`ImageData`, `ModelData`)
and is tested without a GPU. Dependencies still only point down;
`fynkel_load` doesn't know the world or the renderer.

## 3. Design decisions

### 3.1 Hierarchy

- `Parent(ObjectId)` is a component. `Transform` stays the local transform, relative to the parent. An object without one is a root.
- `object.parent = other` (or `null`), `object.children`, `object.worldTransform` (read only). Setting a parent that would make a cycle is an error (`FY_STATUS_INVALID_VALUE`), never a hang.
- `object.despawn()` despawns its children too. That's what a game almost always wants, and the opposite (orphans left in place) is easy to write by hand.
- `Name` is a component (optional, not unique). `object.find('Head')` searches its descendants, depth first. glTF nodes keep their names.
- `object.isPartOf(other)` is true for `other` itself and everything below it.
- World transforms are computed during the extract, in one pass over objects sorted parents first. The order is kept between frames and rebuilt only when a parent changes. Stage 1's test that a steady frame allocates nothing on the Rust heap must still pass.
- The budget, checked by a benchmark on the Mac and measured on both phones: 10,000 objects (one flat level, 100 × 100, and a chain 100 deep) extract in under 1 ms on the iPhone.

### 3.2 Meshes and textures

- Vertices gain UVs and tangents (position, normal, tangent with handedness, UV: 48 bytes). Built-in shapes get both. When a glTF mesh has no tangents, they're generated with MikkTSpace, as glTF requires.
- `Mesh.custom(positions:, normals:, uvs:, indices:)` makes a mesh from code (rule 2: everything a loader can do, code can do too). Normals and tangents are computed when left out.
- `Texture.load(bytes)` decodes PNG or JPEG and builds the mipmaps on a helper isolate (in Rust, through FFI), then hands the pixels to the world. The render worker uploads them; the Dart UI thread never waits for decoding or for the GPU. `Texture.rgba(width, height, pixels)` makes one from code.
- Colour textures (base colour, emissive) are sRGB; data textures (normal, metallic-roughness, occlusion) are linear. The material slot decides, so the same image can't be read the wrong way.
- Mipmaps are built on the CPU in linear light, so a texture doesn't darken as it shrinks. A golden test checks a distant checkerboard is mid-grey, not darker.
- `Material` gains `baseColorTexture`, `normalTexture`, `metallicRoughnessTexture`, `occlusionTexture`, `emissive`, `emissiveTexture`, `uvScale`, and `alphaCutoff` for cut-out leaves and fences (glTF's MASK). One sampler: repeat, trilinear, anisotropic 8× where supported.
- Textures and meshes are freed when disposed, or when the world is, and the GPU memory is checked to come back.

### 3.3 Models (glTF)

- `Model.load(bytes)` takes a `.glb` (one file, with everything inside). `.gltf` with separate files would need a way to fetch them; it comes later if wanted. Parsing runs on a helper isolate, like textures.
- A `Model` is a resource: its meshes, materials and textures are uploaded once. `world.spawnModel(model, transform)` makes one object per glTF node, with `Parent`, `Name`, `Transform` and `MeshRenderer`, and returns the root. Spawning the same model twice shares the meshes and textures.
- Supported: meshes (triangles; several primitives per mesh), the metallic-roughness material with all five textures, MASK alpha, node transforms and names, `KHR_texture_transform` and `KHR_materials_emissive_strength`. Everything else (skins, animations, morph targets, cameras, lights, BLEND) is skipped with one readable warning per kind, never an error.
- Tests: Khronos's glTF sample models load and render to golden images (Box, BoxTextured, NormalTangentMirrorTest, AlphaBlendModeTest's MASK part, a model with 100+ nodes), and broken files give readable errors.

### 3.4 Input

- `onFrame(dt, input)` gets a `FrameInput`, read in the game's frame: the pointers down and where they are, this frame's taps and drag delta, scroll, and the keys held (Flutter's `LogicalKeyboardKey`), with `pressed`/`released` for this frame. Positions are in the view's logical pixels.
- This changes stage 1's `onFrame(dt)` to `onFrame(dt, input)`. hello is updated with it; there are no other users yet.
- `FynkelView` collects the events with a `Listener` and a `Focus`, so it works with mouse, touch, trackpad and keyboard alike. Flutter's own gesture widgets around a view keep working.
- `camera.rayThrough(point)` turns a point in the view into a world-space ray, using the view's real size. `world.raycast(ray)` returns the closest hit: the object, the point and the distance. It tests bounding boxes first, then triangles, on the CPU against the meshes it already has. A faster structure comes only if a measurement asks for it.
- Raycasts read the world as it is now, not the last frame drawn, so a tap acts on what the game just set.

### 3.5 Measuring

- Load times for each texture and model, on all three devices: time on the helper isolate and time until the first frame shows it.
- GPU memory per texture and per model, and it must come back: 10 cycles of loading and disposing the scene's model leave memory flat (the stage 1 rule: no more growth than Flutter alone).
- The garden scene at the display rate over 60 s on all three devices, with the same stats as stage 1.

## 4. Steps

Each step is built, tested, and merged on its own, in this order. Each
step's tests fail first where that makes sense.

**Step 1: hierarchy.** `Parent`, `Name`, world transforms in the extract,
recursive despawn, cycles as errors, the Dart API, and the 10,000-object
benchmark. *Done when* the tests and the budget pass on the Mac.

**Step 2: vertex format and custom meshes.** UVs and tangents, MikkTSpace,
`Mesh.custom`. *Done when* the golden images match (the cube is
re-blessed once, after you look at it) and a custom mesh renders.

**Step 3: textures.** `fynkel_load` decoding with mipmaps, `Texture`,
material slots, sRGB vs linear, the sampler, freeing. *Done when* the
golden images pass (including the distant checkerboard) and loading
never blocks the UI thread (a test checks Flutter frames during a load).

**Step 4: glTF.** `Model.load`, `spawnModel`, the sample-model goldens,
warnings for what's skipped. *Done when* every listed sample renders
correctly and spawning a model twice shares its GPU data.

**Step 5: input and raycasts.** `FrameInput`, `rayThrough`, `raycast`.
*Done when* widget tests drive taps, drags and keys, and raycasts hit the
right object in unit tests, including through a hierarchy.

**Step 6: `examples/garden` on macOS, then iOS, then Android.** The
scene from the snippet: a textured ground, a model, tap and drag. *Done
when* it runs at the display rate on all three, loads in a measured and
written-down time, and passes the memory cycles.

**Step 7: the stage 2 check.** A short report, as in stage 1. *Done when*
it's committed and you have played the scene on each device.

## 5. Decisions (2026-10-08)

1. **The garden scene uses Kenney's CC0 character and props.** The
   Khronos sample models are used in the tests.
2. **Images are decoded in Rust** (the `image` crate, PNG and JPEG), so
   the engine and its headless tests load textures without Flutter.
3. **No transparency in stage 2**, only cut-outs (MASK). Blending waits
   for the stage that needs it.
