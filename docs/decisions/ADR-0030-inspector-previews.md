# ADR-0030: Inspector previews — one preview stage for every asset, on a shared GPU

**Status:** Accepted — proposed by the assistant 2026-09-26 after the user's steer ("let's go with sharing the gpu and test"), confirmed by the user 2026-09-26 ("ok i accept it. please start")  
**Date:** 2026-09-26  
**Related:** ADR-0016 (the authoring service, edit-preview worlds), ADR-0019 (overlays), ADR-0021 (the viewport's own camera, P7 L5), ADR-0023 (built-in primitives), ADR-0028 §9 (custom asset previews, "when a real case asks for it"), `apps/cli/src/thumbnail.rs` (`fynkel thumbnail`), `docs/ui-refresh/`

## Context

Unity's Inspector ends with a preview area for the selected asset:
- a **material** on a sphere, cube, cylinder or plane. You can orbit it
  and pick the shape. You can also drop your own mesh on it.
- a **prefab** or **model**, framed, lit and orbitable;
- an **animation clip** on its model, with play, scrub and speed;
- a **texture** with RGB/R/G/B/A buttons, mip level and a line such as
  `512x512  RGBA8 sRGB  1.3 MB`;
- a **sound** with its waveform, play, loop, a playhead and a line such
  as `Vorbis, 44100 Hz, Stereo, 0:03.210`.

The user wants the same: "very good", extendable and scalable. It is one
of the areas where Fynkel falls furthest behind Unity today.

What exists (mapped 2026-09-26):

- **The Inspector** shows only a static 40 px picture for an asset
  (`AssetPreviewWidget`). The Project browser's `ThumbnailCache` is not
  used there.
- **`fynkel thumbnail`** (`apps/cli/src/thumbnail.rs`) already builds the
  right tiny scene:
  - the asset alone (its cameras, lights and volumes removed);
  - a material on the built-in Sphere;
  - framed from (1, 0.8, 1) with a 30° lens;
  - lit by the fallback sun.

  But it lives inside the CLI and renders one still picture.
- **Edit-preview worlds.** The authoring session holds one per open scene
  or prefab (`fynkel_authoring_preview_open`, `crates/fynkel_native/src/authoring.rs`).
  They follow the live document, unsaved edits included. A material is
  refused.
- **The viewport ABI** (Apple, v9) renders any world pointer
  (`fynkel_apple_viewport_render`) and has a per-viewport camera
  (`set_view`). The Game view already runs a second viewport beside the
  Scene view.
- **Each viewport makes its own GPU device.**
  `fynkel_apple_viewport_create` asks Metal for a new adapter and device
  every time (`platform/apple/src/ring.rs`). Two viewports mean two
  devices, and each device pays its own start-up. A preview that opens
  and closes with the selection would create a device on every click.
- **Sound.** Sound is decoded by `fynkel_import::audio` and cooked into
  `AudioClip`s. It plays through `fynkel_audio` once a world opens its
  device (`fynkel_world_audio_open`). No call hands samples to Dart.
- **Animation.** `Clip::sample` exists, as does the `Animator` component.
  But an edit-mode world runs no systems (`fynkel_core/src/world.rs`), so
  nothing moves in the edit-preview worlds.

## Decision

1. **A preview area at the bottom of the Inspector, as Unity's.**
   - **Layout.** A header row with the asset's name on the left and the
     provider's toolbar on the right, above a picture area. It opens and
     closes like a section. It is resized by dragging the line between it
     and the page. Its height and open state are kept per asset kind in
     the preferences.
   - **When it shows.** It shows for a selected asset whose kind has a
     provider. It also shows for a selected object in the Hierarchy that
     is a prefab instance, as Unity does.
   - **Keyboard.** The picture takes keyboard focus. The same keys work as
     in the Scene view: F frames, and the arrows orbit.

2. **Previews are providers, registered per asset kind.**
   - **The contract.** A `PreviewProvider` says:
     - which assets it takes: a browser kind, a data type or a test;
     - how to build its toolbar;
     - how to build its picture;
     - what its info line says.
   - **Built-in providers.** The editor registers texture, material,
     model, mesh, prefab, clip, blend and sound through the same registry.
     No built-in has a private path.
   - **Packages.** A package registers its own provider with
     `editor.preview(...)` in `registerEditor` (ADR-0028). This ADR adds
     "custom asset previews" to ADR-0028 §3 and removes it from §9's
     "later" list.
   - **Same rules as ADR-0028.** A failing provider fails alone: its
     picture shows the error and the Console names the package. A
     provider reads assets and never changes documents.
   - **Building blocks for providers.** A provider gets the pieces the
     built-ins use:
     - `PreviewStageView`, the orbitable 3D picture;
     - `PreviewImageView`, a zoomable image with channel buttons;
     - `PreviewWaveform`.

     So a data asset that holds a mesh and a material can show them in
     3D with a few lines of Dart.

3. **One native preview stage for every 3D preview.**
   - **What a stage is.** A small world owned by the authoring session,
     next to the edit-preview worlds. It is the counterpart of Unity's
     `PreviewRenderUtility` and its preview scene. It holds:
     - the subject;
     - the stage's own light, a neutral environment and a floor that is
       off by default;
     - nothing else from the project.
   - **Opening and closing.**
     - `fynkel_authoring_stage_open(session, json)` opens a stage and
       returns its id.
     - `fynkel_authoring_stage_world(session, id)` returns the world
       pointer for `fynkel_apple_viewport_render`.
     - `fynkel_authoring_stage_close(session, id)` frees it.
     - `fynkel_authoring_stage_call(session, id, kind, json)` changes it.
   - **The calls:**
     - `subject`: the asset, and for a material the shape: `sphere`,
       `cube`, `cylinder`, `plane`, `capsule`, or any mesh or model
       asset (drag and drop);
     - `frame`: returns the camera that fits the subject;
     - `light`: yaw and pitch, as Unity's right-drag on a preview;
     - `clip`: clip, time, playing, speed and loop;
     - `sound`: play, stop, loop and the playhead;
     - `stats`: triangles, vertices, bounds and bones, for the info line.
   - **It follows unsaved edits.** A stage is resynced from the session's
     documents like the edit-preview worlds are. A material being edited
     in the Inspector therefore shows its unsaved values on the sphere at
     once.
   - **One library, two uses.** The tiny-scene code moves out of the CLI
     into a shared library, `fynkel_preview` (a new crate). It holds:
     - the subject set-up;
     - the stand-in shapes;
     - removing the asset's own cameras and lights;
     - the framing and the lighting.

     `fynkel thumbnail` and the stage both call it, so a Project tile and
     the Inspector preview always agree.
   - **Only what the subject needs runs.** A stage is not an edit-mode
     world, but it runs only the systems its subject needs:
     - animation, for clips and blends;
     - audio, for sounds.

     It never runs behaviours, physics or scripts (ADR-0016's rule that
     the editor runs no game code in edit mode stands).

4. **All viewports share one GPU device.** This is the user's choice.
   - **Shared.** Every viewport in the process uses one wgpu instance,
     adapter, device and queue. The device is created by the first
     viewport and freed when the last one goes.
   - **Still per viewport.** The IOSurface ring, the renderer and its
     settings stay per viewport. The ring's textures are imported on the
     shared `MTLDevice`, which is what makes them valid for every
     viewport.
   - **The ABI.** `fynkel_apple_viewport_create` keeps its signature, so
     it is not a breaking change. The ABI version goes to 10 so the host
     can tell the two behaviours apart.
   - **A lost device** makes every viewport recreate against a new shared
     device. Today each would do this on its own.
   - **Proved by a measurement, not assumed.** A test in `platform/apple`
     creates the Scene view's viewport and then three preview viewports.
     It records:
     - how long each viewport takes to create;
     - the process's physical footprint after each;
     - the Scene viewport's frame time while a preview renders beside it.

     It runs in both modes: one device per viewport (as now) and one
     shared device.

     Sharing stays only if:
     - a second viewport is created in under 50 ms;
     - memory grows less than in the per-viewport mode;
     - the Scene view's frame time changes by 5% or less.

     The numbers go into `docs/perf/shared-gpu.md`. If the test fails,
     this point comes back to the user before anything else changes.
   - **Later, not now.** Sharing uploaded meshes and textures between
     renderers. Each renderer still keeps its own GPU copies. A shared
     device is what makes a shared store possible later. It will be its
     own measured step.

5. **Previews draw only when something changes.**
   - **3D previews** render a frame only when something changes: an
     orbit, a resize, an edit, a new subject, or animation playing.
   - **Budget.** Selecting an asset opens its stage. The first picture
     must show within 100 ms of the cook being ready.
   - **One stage at a time.** Only one stage is open for the Inspector.
     Changing the selection moves it to the new subject rather than
     opening a new world. A locked Inspector or a second Inspector gets
     its own stage.
   - **Nothing runs in the background.** When the preview is folded,
     scrolled away or the window is hidden, it does not render. A stage
     for an asset that is no longer selected is closed after a short
     delay.

6. **What each built-in provider shows** (the order they are built in):
   1. **Texture.** It needs no GPU work: the image is decoded by Flutter,
      and the cooked data is used for formats Flutter cannot read.
      - Buttons: RGB, R, G, B, A, and a mip slider when there are mips.
      - Zoom with the wheel; drag to pan.
      - A checkerboard behind transparency.
      - The line: `512x512  RGBA8 sRGB  1.3 MB  mips 10`.
   2. **Material, model, mesh and prefab** (a stage):
      - Orbit with a left-drag, pan with a middle-drag or Alt, zoom with
        the wheel, and turn the light with a right-drag. The same
        `EditorCamera` maths as the Scene view is used.
      - A material has a shape menu, and any mesh can be dropped in. The
        choice is remembered per material in the editor's state, never in
        the file.
      - The line: triangles, vertices, sub-meshes, bounds, and bones for
        a skinned model.
   3. **Sound:**
      - The waveform. It is peaks computed once from the cooked clip by a
        new native call, `fynkel_authoring_stage_call(…, "peaks", {width})`,
        so no raw samples cross into Dart.
      - Play and stop, loop, and a playhead. Clicking the waveform starts
        playback from that point. Playback goes through `fynkel_audio` on
        the stage world.
      - The line: `Vorbis, 44100 Hz, Stereo, 0:03.210, 212 KB`.
   4. **Clip and blend:**
      - The clip on its model, or on a model the user drops in.
      - Play, pause, a scrub bar, speed (0.25× to 2×), loop, and the
        current time and frame.
      - Root motion is shown in place, with an option to let it move.

      This leans on the Animator work (roadmap Phase 5). If that work is
      not ready, the stage calls `Clip::sample` itself, and the provider
      stays the same.

7. **Tests.**
   - **Widget tests** for each provider's toolbar and info line, and for
     the preview area's fold, resize and saved height.
   - **Rust tests** for `fynkel_preview`: the framing, and removing the
     asset's cameras and lights. `fynkel thumbnail`'s existing tests must
     still pass unchanged.
   - **Stage tests:** open, subject, frame, resync after a material edit,
     and close.
   - **Pictures.** Screenshots of every preview are added to the shots
     test and placed beside Unity's in `docs/ui-refresh/`.
   - **The shared-GPU measurement** from point 4.

## Alternatives considered

- **Use the Project browser's thumbnails in the Inspector.** Rejected.
  A thumbnail is a still picture from the CLI: no orbit, no unsaved
  edits, no animation, no sound.
- **Reuse the edit-preview worlds for materials.** Rejected. Those worlds
  are the document's own scene. A material has no scene to open, and
  bending that path would mix "the document" with "a stage to look at
  it".
- **One device per viewport, as now.** Rejected by the user. A device's
  start-up would be paid on every selection, twice the device memory
  would be used, and a later shared GPU asset store would be impossible.
- **Draw previews in Flutter (a Dart 3D painter).** Rejected. It would be
  a second renderer that shows materials differently from the game.
- **Render previews into images offscreen and hand the pixels to Flutter**
  (the thumbnail path, every frame). Rejected. It copies each frame
  through the CPU and is slow at Inspector sizes. The IOSurface ring is
  already the zero-copy path.

## Consequences

- **Speed and memory.** The Inspector gets Unity's previews, and adding a
  viewport becomes cheap.
- **Packages** can show their own data in 3D.
- **Sharing the device.** The viewport ABI goes to version 10. The Swift
  plugin changes little; the change is inside `platform/apple`. On a lost
  device every viewport is affected, not one.
- **A new crate,** `fynkel_preview`, and new authoring calls. These are
  shared-crate touches, so they go in their own commits.
- **Thumbnails.** `fynkel thumbnail` now comes from the shared library.
  Its output must not change, and its tests check this.
- **Other platforms.** Windows, Linux and the web viewports will need the
  same shared device when they arrive. This ADR states it as the rule for
  every host.

## Order

1. **The preview area** in the Inspector, and the provider registry
   including `editor.preview`.
2. **The texture preview.** No native work.
3. **The shared GPU device** and the measurement. It is a separate commit
   in `platform/apple`, and its numbers are brought to the user.
4. **`fynkel_preview`,** lifted out of `fynkel thumbnail`. Thumbnails stay
   exactly the same.
5. **Stages** in the authoring session, then the material, model, mesh
   and prefab previews.
6. **The sound preview.**
7. **The clip and blend preview,** with the Phase 5 Animator or
   `Clip::sample`.
8. **Pictures beside Unity's** in `docs/ui-refresh/`, and the user's check
   in the real app.
