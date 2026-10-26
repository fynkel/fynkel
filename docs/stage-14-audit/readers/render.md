# Stage 14 audit — crates/fynkel_render (reader: render)

Read-only pass over renderer.rs, scene.rs, gpu.rs, assets.rs, texture.rs, shader.rs,
shadows.rs, clusters.rs, ao.rs, bloom.rs, effects.rs, sky.rs, frame_ring.rs, lib.rs and the
.wgsl files. Hardening's "raised and left" items (R8 lock while recording, R13 wrap
samplers, V10 GPU wait, Mali goldens, R7 wide spots) are not re-raised.

---

## RN1 — Outlines are treated as very bright light by bloom
- **Severity:** bug. **Size:** S.
- **Where:**
  - forward.wgsl:310-314: `fs_outline` returns `vec4(fogged(base_color.rgb, …), 1.0)`.
  - renderer.rs:2544-2552: the outline pipeline uses `blend: None` and `write_mask: ALL`.
- **How it shows:** the scene image's alpha means "how much of this pixel is light".
  - bloom.wgsl's `light()` and finish.wgsl's `fs_bloom` undo the tone mapping on rgb and weight it by alpha.
  - The sky gradient (sky.wgsl) writes picked colours with alpha 0 for exactly this reason.
  - An outline writes the colour as picked (not tone mapped) with alpha 1. Untonemapping a channel near 1.0 gives about 115 (neutral.wgsl).
  - Result: on MEDIUM/HIGH (bloom on), a white or light outline blooms like a bright lamp, and a mid-grey one glows noticeably. On LOW (no scene image) it looks right, so the outline looks different from one quality level to another.
- **First fix:** write alpha 0 from `fs_outline`, as the sky gradient does. The other option is to tone-map the colour in and keep alpha 1 (then it would no longer be "as picked").
- **Test today:**
  - Draw a white-outlined cube on a dark background at Quality.MEDIUM, then again with bloom off.
  - Diff the pixels just outside the outline: they should match, but with bloom on they will show a halo.
  - Or read the bloom chain's first mip back after the frame: it is non-zero around the outline.

## RN2 — Turning, dimming or recolouring the sky rebuilds all its light, every frame it changes
- **Severity:** slow. **Size:** M.
- **Where:**
  - renderer.rs:1591-1596: rebuilds when `e.kind != *kind`.
  - sky.rs:243-410: `Environment::new`.
  - scene.rs:717-731: the `SkyKind` comparison includes the gradient colours and the image's intensity and rotation.
- **How it shows:** any change to rotation, intensity or a gradient colour makes `Environment::new` run again. That means:
  - new cube textures,
  - about 90 render passes (radiance, 6 specular mips × 6 faces, irradiance),
  - a full re-upload of the HDR image through `image_texture`.

  Intensity and rotation are only baked into the pass uniforms (zenith.w and horizon.w). A day/night cycle or a slowly turning sky pays all of this every frame, which on the Note10+ means tens of MB of bandwidth per frame. Each Renderer (view) also keeps its own Environment, so two views pay twice. `stats.sky_bytes` counts it per view as well.
- **First fix:**
  - Apply rotation (turn the lookup direction) and intensity (scale the result) where forward.wgsl, sky.wgsl and particles sample the cubes, and leave them out of the rebuild key. Then only a new image or new gradient colours rebuild.
  - Keep the built Environment in the Kit/Assets, keyed by image, so views share it.
  - Upload the source image once per image, not once per rebuild.
- **Measurement today:** in the bench, add 0.1°/frame to `Sky.image.rotation` and compare GPU frame time and the `sky_bytes` churn against a still sky. On the Note10+, a capture would show the ~90 small passes per frame.

## RN3 — The pipeline cache is written to disk again after every pipeline made
- **Severity:** slow. **Size:** S.
- **Where:** assets.rs:130-138 (`make` calls `disk.save()` every time) and assets.rs:111-122 (`save` spawns a thread and writes the whole cache).
- **How it shows:**
  - At startup or on a new scene, N pipelines spawn N threads.
  - Each thread calls `cache.get_data()` (all bytes so far) and writes the whole file, so the bytes written grow O(N²).
  - The threads queue on the `writing` lock.
  - On Vulkan (Note10+), `vkGetPipelineCacheData` runs alongside the `create_render_pipeline` calls still happening on other threads. Whether the driver serialises the two is not traced here (uncertain), but at best it is wasted work.
  - Shader warm-up (own shaders build on threads) makes this worst exactly when the frame is busiest.
- **First fix:** mark the cache dirty in `make`, and save once after a quiet period (or at most every few seconds, and on drop/pause).
- **Measurement today:** count `save` threads and bytes written during the lab's first scene. Log it in `save`, or use `fs_usage`/strace on the cache file. Expect one full write per pipeline.

## RN4 — Transparent particles are sorted back to front on the render thread every frame
- **Severity:** slow. **Size:** S.
- **Where:** renderer.rs:3379-3382 (`particle_batch`): `sort_unstable_by` on depth for each transparent, non-trail system.
- **How it shows:**
  - The sort is O(n log n) per system per frame, after the sim, inside `prepare` (the render thread's CPU half).
  - The depth closure recomputes a `Vec3` dot product inside each comparison, so about 2·n·log n dot products instead of n.
  - At 10k particles this is roughly 280k comparisons each frame. Stage 12's Note10+ step time (0.33 ms) measured the sim, not this sort.
  - Uncertain how much it costs on device, because no stage-12 number isolates `prepare`.
- **First fix:**
  - Compute depths into a scratch `Vec<(f32, u32)>` once, sort the keys (or radix sort on the f32 bits), then gather.
  - Skip the sort when the camera and particles have not moved.
  - Additive systems already skip it; consider letting `Transparent` with a low alpha opt out too, as Unity's sort mode "None" does.
- **Measurement today:** time `particle_batch` in the lab's 10k transparent system on the Note10+ with `--profile`, sorted against an additive system of the same size.

## RN5 — Soft particles take the whole "what's behind" path when they only need depth
- **Severity:** slow. **Size:** M.
- **Where:** renderer.rs:1265 (`self.behind |= reads_behind || (softness > 0 && soft_particles)`), and encode's behind branch: copy depth, resolve colour, restore.
- **How it shows:**
  - On MEDIUM/HIGH (`soft_particles` on), one soft system in view turns on the full two-pass split: an MSAA colour resolve to a copy, a depth copy, then a restore pass that loads the colour back.
  - Soft particles read only `fy_scene_depth`.
  - On the Note10+ tile GPU this costs a full-screen resolve, store and reload of the colour image every frame a soft emitter is visible. Smoke or dust in view becomes a bandwidth cliff.
  - Possibly accepted as the price of stage 12 phase 3; marking it so it is weighed.
- **First fix:** split the flag into `needs_depth` and `needs_color`. For depth-only, keep depth (store instead of discard) and copy it, and skip the colour resolve/restore. Soft particles and effect depth then take the cheaper path.
- **Measurement today:** in the lab "Particles and the world", compare the Note10+ GPU frame time with `softness` 0 against `softness` > 0, both at MEDIUM.

## RN6 — A particle module with its own shader is not checked when the shader is made
- **Severity:** awkward. **Size:** S.
- **Where:** renderer.rs:2858 (`format!("{}\n{}", shader.wgsl, particles.wgsl)` passed straight to `create_shader_module`). Compare shader.rs, where `CustomShader::new` checks the forward glue with naga.
- **How it shows:**
  - The own shader is validated only in its forward/effect form.
  - The particle composition (shader.wgsl + particles.wgsl) is first compiled when the first frame draws a particle system with that material.
  - Any clash (a name both files define, or a function particles.wgsl expects that the shader lacks) shows up as a wgpu validation error or panic at draw time, not as a Dart error at `Shader(...)`.
  - Uncertain whether wgpu's error scope catches it cleanly here; not traced.
- **First fix:** validate the particle composition with naga in `CustomShader::new` (as is done for the forward glue), and report it like the others.
- **Test today:** make a shader whose WGSL defines a helper named like one in particles.wgsl, then use it on a ParticleSystem. Expect a failure at draw time rather than at creation.

## RN7 — Per-object data goes to the GPU four different ways
- **Severity:** awkward (seam). **Size:** M.
- **Where:**
  - `Instance`, 148 B: renderer.rs:221.
  - Shadow `Caster`, 96 B, duplicated for every map an object falls in: shadows.rs:232.
  - Own-shader casters push extra Instances instead: renderer.rs:1506-1508.
  - `ParticleInstance`, 128 B, plus joints and shader values sharing one storage block.
  - Dynamic meshes use `queue.write_buffer`: assets.rs:396-404.
  - The effect uniform uses `queue.write_buffer` too: effects.rs.
- **How it shows:**
  - frame_ring.rs:3-11 says per-frame `queue.write_buffer` blits cost Apple 128 MB of dirty memory, which is why the ring exists. Stage 13's dynamic meshes (and the sweep road) went back to `queue.write_buffer` each frame they change.
  - Casters copy the model matrix into a second layout instead of indexing the Instance array.
  - Each new feature (particles, own shaders, skinning) added its own layout and its own upload path.
- **First fix:**
  - Have one per-object record in the ring, and let shadow and depth passes index it (instance index → record).
  - Put the vertex/index data of changing meshes in the frame ring, or use a mapped staging ring.
- **Measurement today:** on iPhone, Instruments' VM tracker during the Splines lab with a road being re-swept every frame. Watch IOSurface/dirty memory rise against a still road.

## RN8 — renderer.rs is a 3.5k-line file whose `encode` records every pass inline
- **Severity:** rewrite. **Size:** L.
- **Where:**
  - renderer.rs:1527-2157 (`encode`, about 630 lines) and 934-1375 (`prepare`, about 440 lines).
  - Pipeline makers at 2417-3187: about ten `make_*` functions that each repeat the vertex-buffer layouts, the depth-stencil state and `cache: None` before `PipelineMaker::make` replaces it.
- **How it shows:**
  - Every stage added a branch inside `encode`: behind, AO prepass, spot maps, sky, particles, trails, lines, effects, bloom and finish.
  - Load/store choices and which images live depend on flags set far away in `prepare` (`behind`, `behind_kept`, `bloom`, `effects`).
  - Fixing RN5 (depth-only behind) means touching the scene pass's attachment setup, the behind branch and the restore, all in the same function.
  - Adding a pass (for example decals or SSR) means another flag threaded through both halves.
- **First fix:** make the frame a short list of pass structs (shadow, ao, scene, behind, particles, effects, bloom/finish). Each owns its targets and declares what it reads and writes. `encode` walks the list, and load/store follows from what later passes read. Move the pipeline makers into the modules for each feature, and build vertex layouts in one shared helper.
- **Measurement:** not a runtime issue. The test is how many places RN5's fix has to touch.

## RN9 — Small leaks and stale state
- **Severity:** awkward. **Size:** S.
- **renderer.rs:970-973:** with no camera, `prepare` returns early. `self.behind` and `behind_kept` keep last frame's values, so the next encode may set up the behind targets for a frame that has no scene. Uncertain whether encode also checks for the camera; harmless if it does.
- **frame_ring.rs:253-256:** `write` grows a slot's buffer to `next_power_of_two` and never shrinks it. One frame with a spike (an explosion of 100k particles) keeps 3× the peak buffer for the life of the view.
- **renderer.rs:1597-1600 and bloom.rs:** the bloom chain is made when bloom is on and kept when bloom is turned off. It is freed only when the scene image is recreated.
- **texture.rs:** the `bind_groups` and `own_bind_groups` caches drop entries only when a texture changes. Materials or texture sets that are made and dropped (generated per level) stay pinned.
- **First fix:**
  - Reset the behind flags before the early return.
  - Shrink a ring slot after N frames under ¼ of its size.
  - Drop `image.bloom` when `self.bloom` is None.
  - Evict bind groups when their texture handle is freed.
- **Test today:** a frame-ring unit test that writes one large frame and then small ones, then checks the buffer size. For the leaks, load and unload a level 20 times in the soak and watch the footprint check.

---

## Looked at and fine
- Instance and ParticleInstance attribute offsets against the struct layouts (148 B / 128 B).
- Cluster maths: log slices, cell_of in forward.wgsl against clusters.rs's build, conservative tile bounds, and 64-bit masks with no cap.
- Cascades: splits, texel snapping, pancaking, MAX_STRETCH, blending between cascades, the spot fov clamp (R7 holds).
- Shadow caster batching, bind-group order, and MapArray sharing between views (ordered by queue submission).
- MSAA load/store: transient MSAA colour discarded, depth discarded unless kept for behind.
- Frame ring: three slots, the map/unmap flow, and alignment of sections.
- Premultiplied transparent blending (`fs_transparent`), additive, and cutout.
- Fog on particles and lines.
- Trail draw ranges (one strip per draw).
- `frustum_planes` for both depth conventions; reversed-Z depth compare everywhere.
- Effects ping-pong in the view's format; the sRGB view assertion.
- AO parameters taken after the pixel-look (render scale) size.
- Variant indexing over all seven axes: no missing pipeline combination found.
