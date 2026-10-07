# ADR-0001: Fynkel 2, a code-first framework

- Status: accepted
- Date: 2026-10-07
- Supersedes: every Fynkel 1 ADR (0001–0062, archived at `fynkel-1-final`)

## Context

Fynkel 1 grew into a Unity-style engine with about 265k lines. It had an
editor, a JSON document model (scenes, prefabs, materials, data assets),
an import and cook pipeline, physics, audio, animation with IK, and a
renderer with shadows, AO, bloom and grading. It worked, but every
feature cost twice: once in the engine, once in the editor and
serialization.

On 2026-10-07 we compared it with Bevy embedded in Flutter. On an iPhone,
our wgpu renderer matched or beat Bevy (Sponza: 106 fps / 9.2 ms GPU vs
60 fps / 15.1 ms). The renderer and the Flutter presentation path (an
IOSurface ring plus a frame queue) are worth keeping. The weight around
them is not.

## Decision

Start again as a **code-first framework**. A game is a Dart program that
builds and drives a world through one public API. We keep our own
renderer on wgpu and our own ECS on hecs, and we do not use Bevy.

Removed, and only brought back if a real game needs it: the editor, CLI,
authoring/document/preview/cook crates, all JSON and schemas, codegen,
animation and IK, physics, audio, glTF import, textures and compression,
shadows, AO, bloom, grading, volumes, shader assets, and all examples,
benchmarks and spikes.

### The quality rule

Nothing is copied from Fynkel 1 as it is. We bring each piece back on
purpose and make sure it is:

1. **understood**, meaning it is small enough to read and every part has
   a reason;
2. **improved**, fixing what we learned was wrong;
3. **tested**, with a test that fails first where that makes sense;
4. **measured** if it is on the frame path.

### Rules that keep an editor possible later

An editor may come years from now. To keep that door open, the core
follows four rules from the start:

1. Every component describes its own fields: name, type, default and
   range.
2. Everything can be built through one public API. An editor would be
   just another caller.
3. Objects have stable IDs, not only memory handles.
4. Saving is an optional layer on top. The core never needs it.

### Flutter first, with an engine that stands alone

Fynkel is made for Flutter and Dart developers only. Other hosts, such
as React Native, are out of scope. The Rust engine is still kept
self-contained, so it can be tested and reasoned about without Flutter:

1. **No Flutter or Dart in the Rust crates.** The C interface (`fy_*`)
   uses plain C types only.
2. **Flutter provides the frame clock, and Rust does the work on its own
   threads.** The engine never owns a main loop or a window. Flutter's
   ticker calls `fy_frame()` once per display refresh. That call copies
   the world into a render list and returns, and Rust's render thread
   draws the frame. One clock keeps frames in step with the display: no
   wasted frames, no drift and judder. Work that must not wait for frames
   (rendering now; audio and loading later) runs on Rust's own threads.
3. **Game logic lives in Dart, above the C interface.** The engine is a
   library the game calls. It never calls back into Dart to run game
   code.
4. **The engine runs without Flutter in tests.** Pure Rust tests render
   to PNG in CI, so any Flutter dependency leaking into the core breaks a
   test.

## Stage 1 scope

Stage 1 is a bare engine that renders something:

- `crates/fynkel_render`: wgpu, one forward pass, a PBR colour material,
  one sun plus ambient, and a camera.
- `crates/fynkel_world`: hecs with Transform, MeshRenderer, Camera and
  Light.
- `crates/fynkel_ffi`: the C ABI that Dart calls.
- The platform layer: an IOSurface ring plus a frame queue on Apple, and
  a SurfaceProducer-backed surface on Android. Both sit behind one
  interface.
- `packages/fynkel`: the Dart API, with `World`, `spawn`,
  `Mesh.cube/sphere/plane`, `Material`, `Camera` and `Light`.
- `packages/fynkel_flutter`: the `FynkelView` widget and the texture
  plugin.
- `examples/hello`: a lit spinning cube written in Dart.
- A headless render-to-PNG test in CI.

**Platforms:** macOS, iOS and Android are all in stage 1, built in that
order. Stage 1 is done when the cube runs at the display rate on all
three. Windows and Linux come soon after.

## Getting old code back

- Read a file: `git show fynkel-1-final:<path>`
- Restore a folder to look at: `git checkout fynkel-1-final -- <dir>`
  (then unstage it)
- Work on the old engine: `git checkout fynkel-1`

Local-only files (the Mixamo hero, test copies, comparison assets) are
backed up outside the repository, in `../fynkel-1-local/`.
