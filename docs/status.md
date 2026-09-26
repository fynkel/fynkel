# Engine status — 2026-09-25 (M4 gate: GO; M3: GO with two named exceptions; §6)

The one-page, plain-words answer to "what do we have, is it solid, what is
missing". Rewritten at every milestone gate and updated whenever a row below
changes. Numbers come from the gate reports (`m0-architecture-report.md`,
`m1-gate-report.md`, `m2-gate-report.md`, `m3-gate-report.md`) and the
captures under `benchmarks/`; nothing here is estimated.

## 1. What exists

| Layer | What it is | State | Proof |
| --- | --- | --- | --- |
| `fynkel_contracts` | Shared types: ids, schema metadata, `AssetRef`/`Uuid`, the §14.2 diagnostic record and code namespaces, wire header | **Solid** | unit tests; every other crate builds on it |
| `fynkel_ecs` + `fynkel_scene` | `hecs` behind a thin facade (ADR-0003); transforms, hierarchy, propagation | **Solid** | unit tests; deterministic checksum in the headless host; `hecs` provably hidden |
| `fynkel_core` | The world: command batches in, snapshot/events/diagnostics out, fixed §5.1 step order, `load_scene` from a typed description; the input contract (`input.rs`, `docs/input.md`): the binding grammar, actions compiled from `inputActions`, raw events captured per frame, held state every tick and edges consumed by exactly one; the physics slot (`fynkel_physics` stepped, rejections → diagnostics, contacts on the physics lane); the native catalogue (`world/catalogue.rs`, `docs/catalogue.md`): `Rotator` (14), `Oscillate` (15), `LifetimeDespawn` (18), `Emitter` (19) before physics, `Follow` (16) and `LookAtCamera` (17) after it, `Behaviour`-free, with every phase of the step timed (`SystemTime` per `SystemKind`, `FYNKEL_QUERY_SYSTEMS`) (M3-10); runtime scenes (`load.rs`, `docs/scenes.md`): `load_scene_with(scene, LoadMode::{Replace, Additive})` / `unload_scene` / `loaded_scenes` / `scene_of` over a scene registry — replace unloads everything and destroys the rest (runtime spawns included), additive stacks, unload takes a scene's still-alive entities only — `SceneReady { scene }` / `SceneUnloaded { scene, entities }`, and `TriggerZone` (20) turning a sensor's trigger contacts into the general-lane `ZoneEntered`/`ZoneExited { zone, other, event }` (`once` removes itself) (M3-12); world labels (`world/projection.rs`, `docs/hud.md`): `project(width, height, anchors, out)` from the frame's camera and interpolated poses through the renderer's own `view_matrix`/`projection_matrix`, `PROJECTED_*` flags, opt-in occlusion by collider mask (a raycast), `MAX_PROJECTION_ANCHORS` 64 with `TOO_MANY_ANCHORS` (`FYN-R0012`) beyond (M3-13); animation (`animation.rs`, `world/animation.rs`, `docs/animation.md`): the one clip core — channels over named nodes, step/linear/cubic-spline sampling into poses, pose blending, skins with inverse bind matrices —, `Animator` (21) ticked first among the pre-tick systems (fresh-tick sampling, looping or one-shot with `ANIMATION_ENDED`, crossfades with the outgoing clip on its own clock, `CLIP_UNAVAILABLE` once), `Skin` (22) resolving its joints by name under its skeleton root (`SKIN_UNAVAILABLE` once) and the frame's matrix palettes from the joints' interpolated poses in the extract, the `Animate` command, the `AnimatorState` query, the root-motion policy (none by default; clips in place) (M3-19); animation depth (M3-19c): a blend space played like a clip — the two points around the parameter blended by the fraction between them, phase-synced (the shared phase times each clip's duration, the clock rescaled on a parameter change), the dominant clip's markers, `parameter` reported as set —, animators run ancestors first so a second `Animator` on a child entity is a layer (`weight` blends, `additive` adds the clip's delta from its first frame), clip markers crossed on the unwrapped clock (loops, backwards) as `ANIMATION_MARKER`, opt-in root motion (`root_motion`: the root's planar delta and yaw taken out of the pose and handed to a `CharacterController` or the transform), `SetWeight`/`SetParameter` (the parameter drives the animator's blend space) | **Solid** | 3 000-tick deterministic checksum (step p50 2 µs at 25 entities), 25 tests + the input tests (a press across three catch-up ticks, the zero-tick frame, composition, reset), `tests/catalogue.rs` (8: spaces under a tilted parent, moved rest, snap/smooth/look-at, billboard priority, subtree despawn, rate/max_alive/velocity/`PREFAB_UNAVAILABLE` once), `tests/scenes.rs` (5: replace/additive/unload/zone/validation), `tests/animation.rs` (the sampler, blending, the clock, one-shots, crossfades, skins and palettes on the rig fixture; layers, markers, root motion with and without a controller), headless host; 5 000 stress movers at 0.15 ms a tick together |
| `fynkel_physics` | `RigidBody` (10) / `Collider` (11) / `CharacterController` (12) and the Rapier adapter (`docs/physics.md`): stepped by `World::step` at the §5.1 slot only while something has a body, collider or character, sync of changed entries only, dynamic poses written back (`PhysicsOwned`), body commands (`fynkel.cmd.Body`, `Move`/`Jump` for characters), contacts with point/normal/impulse (stay for subscribed pairs), raycast/shape-cast/overlap queries, project gravity and named layers, bounded mesh colliders from the cache; characters over Rapier's kinematic controller (capsule, slopes, steps, gravity, own grounded probe, `FYNKEL_QUERY_CHARACTER` → grounded/sliding/velocity) driven either by `Move`/`Jump` or by the actions named on the component, and `ThirdPersonCamera` (13, in `fynkel_core`) placed after the step behind its named target with a sphere-cast pull-in (M3-09); `add_root_motion` summing an animator's extracted travel into the tick's `Move` (M3-19c) | **Solid** | 10 crate tests + core/native/headless physics tests; the fixture's Crate B lands and sleeps under the checksum, the Playground's Player walks, jumps and lands on the Platform from `test/platform.input.json` under the checksum; 500 stress bodies at p50 0.64 ms/tick, checksum repeated |
| `fynkel_audio` | `AudioSource` (23) / `AudioListener` (24), the voice engine and the mixer (`docs/audio.md`, M3-11 part 1): the engine on the world's thread — clip registry over cooked `CLIP` chunks, 32 voices with priority (steal the lowest at or below, oldest among equals; refuse otherwise), the tick clock that decides every position and end, bus/mute/listener gains — and the mixer inside the device callback (fixed tables, resampling × pitch, gain smoothing, nothing allocated or freed there; two `rtrb` rings between them); backends: null (the headless host, every test) and `cpal` 0.18.2 behind the `device` feature (`fynkel_native`, `hosts/standalone`). The audio phase runs last in the tick (`SystemKind::Audio`): sources reconciled with their voices, `PLAYBACK_ENDED` (11) for the non-looping ends; commands `Audio` (10) / `PlaySound` (11) / `AudioBus` (12), queries `AUDIO` (12) / `AUDIO_STATS` (13), `fynkel_world_audio_open/suspend/close`. Streamed music (M3-11 part 2): a sidecar's `stream: true` cooks a `STRM` chunk of the source bytes, a decoder thread per streamed voice (`symphonia`, a 1 s `rtrb` ring, loop by reopening, `finished` on every exit) feeds the mixer through a `StreamTap`; the tick's clock still decides the end; `AudioStats.streams`/`starved`. | **Solid (2D)** | `crates/fynkel_audio` unit tests (the stream's start/loop/stop/bad source among them) + `tests/allocation.rs` (zero allocations in a render, a streamed voice included) + `tests/engine.rs` (a streamed clip ended by the clock, looped, stopped, respawned across a device close); `crates/fynkel_core/tests/audio.rs` (start/end tick, loop/pause/volume/pitch/stop, validation, both lanes, the listener, the voice limit); the ABI test; the fixture's Robot tone headlessly; the device opened by the ABI test, the standalone host and the Flutter test on this Mac |
| `fynkel_wire` + `fynkel_schema_derive` + `fynkel codegen` (a) | Components and document records written once in Rust → JSON Schema (components, `material.schema.json`), Dart codecs (with `copyWith` and the `componentCodecs`/`componentIds` table), Rust codecs (with the `ComponentValue` wire dispatch), fingerprint `0x97fd26db36af3421` | **Solid** — the strongest piece | `codegen --check` in CI; wire fixtures shared by Rust and Dart |
| `fynkel_native` + `package:fynkel` | C ABI v1, `ffigen` bindings, the Dart SDK: `World`, `GameObject`, allocation-free `TransformWriter`, `loadProject(dir)`/`loadScene(sceneId, {cacheDir, mode})` from the cooked cache only → `LoadReport` (scene key, residency, `mode`, `scenes`), `package:fynkel/testing.dart` (`cookProject`), `Behaviour` with the §3.2 lifecycle driven by `World.frame` at the §5.1 points, `GameContext` services with installable test contexts (`Input` over `TestInput`), `BehaviourRegistry`, `World.frame(dt, input:)` with `WorldInput` read in place, recordings (`InputRecording`, `loadInputRecording`); typed native component access — `GameObject.get<T>()`/`has`/`set`/`add`/`setPresentation`/`remove`/`components` (and on `Behaviour`) over `fynkel.cmd.SetComponent`/`RemoveComponent` and `FYNKEL_QUERY_COMPONENTS`, one query per object per tick, read-your-writes, no mirror (M3-07a); physics — body commands, `bodyState()`, contact callbacks on `Behaviour`, `World.raycast`/`shapeCast`/`overlap`, `layerMask` (M3-08); `World.find`/`findAll`, `GameObject.findChild`/`findDescendant` over one `FYNKEL_QUERY_ENTITIES` per tick, spawn-then-configure in one tick through temp references, `World.instantiate(prefab)` over `fynkel_world_instantiate` → `InstantiateReport` (M3-08b; an imported model by `AssetRef(source, sub: World.importedPrefabSub)` since M3-19a — no ABI change, the world keys the prefab by the source); runtime scenes — `world.scenes` (`SceneManager`: `state`/`current`/`loaded`, `load`/`unload`/`transition(to, {mode})` performed at the start of the next frame, `cancel`, listeners) over `fynkel_world_load_scene_with`/`unload_scene`, `unloadScene` → `UnloadReport`, `bindActions` → rebinds re-applied after every load, `Behaviour.onZoneEnter/onZoneExit` + `world.zones`, `SettingsDocument`/`SettingsStore` (memory or a file with tmp+rename saves) behind `GameContext.settings` (M3-12); HUD models and world labels — `GameModel`/`ModelValue<T>` (notify on `!=`) and `World.projection` (`ProjectionView`/`ScreenAnchor`: one `fynkel_world_project` call per frame for every anchor, viewport in the caller's pixels, `visible`/`occluded`/`missing`/`dropped`, the `FYN-R0012` diagnostic once per overflow, informational), a `GameObject` attached this tick reads its transform through the component query until its first snapshot (M3-13); animation — `GameObject.animation` (`play`/`crossfade`/`stop`/`pause`/`resume`/`speed`/`time`, `state` → `AnimatorState`) over `fynkel.cmd.Animate` and `FYNKEL_QUERY_ANIMATOR`, `Behaviour.onAnimationEnded`, `AnimationEnded` events, `LoadReport.animation`, clips a behaviour names registered from the cache on demand in `fynkel_native` (M3-19); native-assets build hook | **Solid** | 1 000 entities/tick: 191 µs encode+submit, 169 µs step, 47 µs read; 5.1 KB/frame Dart allocation on the 25-entity scene (0.7–1.2 KB of it SDK), 2 GCs in 3 000 frames; leak tests at the ABI and viewport |
| `package:fynkel_generator` + `fynkel codegen` (b) | `build_runner` aggregate builder: `@FynkelBehaviour`/`@Expose` classes → `behaviours.g.dart` (factories, field get/set, `registerBehaviours`), `behaviours.g.json` (Inspector metadata) and `schemas/behaviours/<id>.schema.json`; `fynkel codegen --project [--check]` runs it and splits the schemas | **Solid** | authored `game.Spinner { speed: 2.5 }` → validated → instantiated → rotates (M2 gate); emit tests; `--check` in CI for the example and the starter project; 1 s warm |
| `fynkel_document` | Strict JSON with spans, envelope, JSON Schema 2020-12, canonical writer that preserves unknown data; scene, prefab, material (formatVersion 2: the `PbrMaterial` fields flat on the document, custom shaders through their manifest), shader, `.meta.json`, project and lock documents; prefab expansion (UUID v5 ids, overrides, nested refused); asset database (scan, resolve, reference check); migrations with diff + `.bak` (`material/1-2/pbr-fields` is the first real step); scene → runtime description (`describe`, project-aware; the cooker's input); whole-project validation; imported hierarchies (M3-19a): a scene's or prefab's `prefab.asset` accepts `{ asset: <source>, sub: "prefab" }` — the `PrefabLibrary` keys imported prefabs by `(source, sub)` and fills them through the `SourcePrefabs` provider (`fynkel_import::ImportedPrefabs` runs the importer in memory; `NoSourcePrefabs` for the bare crate), overrides address the derived `node/<i>` ids, nested inside authored prefabs, `FYN-D0401` for a source without a hierarchy or a wrong sub; rig documents (M3-19b): `.rig.json` — one skeleton source, clips by name from any model source, `markers` per clip (M3-19c) — as an asset with its own sub-assets (`prefab`, `anim/<name>`; uuids v5 of the rig), validated against the database (`D0500`–`D0502`), its `prefab` filled by adopting the skeleton's imported hierarchy under the rig's id (`Rig::adopt_prefab`, entity ids the source's); blend documents (M3-19c): `.blend.json` — clips along one parameter, phase-synced — validated against the database (`D0600`–`D0602`), cooked as a document chunk with the clips' carriers as dependencies; scene/prefab formatVersion 3 with `scene|prefab/2-3/animator-layers` (M3-19c); authoring commands and transactions (`Editor`: revision = blake3, 13 §11.1 commands + `unsetField` as reversible edits, conflicts, rollback to the byte, byte-exact undo/redo) | **Solid** | 64 integration tests over fixture corpora with 30 exact code/pointer/line/column expectations (the fixture project: 11 documents, 16 assets — `results.scene.json`, `coin.prefab.json` with a sensor `TriggerZone`, `column.rig.json` over `rig.glb` with `wave`/`bob`, `column.blend.json` between them along `energy`, the Rig scene's `Imported` entity instancing the rig's `prefab` with a material override; `H0009` a zone without a sensor; the crate depends on `fynkel_import` for tests only so the fixture validates with the real provider); every command undone to the original text; validate/load ≈20–25 µs per entity, linear to 10 000 entities |
| `fynkel_import` | glTF/GLB, FBX, PNG/JPEG/HDR, WAV/Ogg Vorbis → deterministic `FYNI` intermediates: meshes in the one 72-byte vertex layout, `fynkel.pbr@1` materials in the model's document form with all five texture slots, RGBA8 textures with a declared colour space (a glTF image used as both colour and data is imported twice), f32 PCM clips, node hierarchy as a prefab; rigged sources (M3-19, `fynkel.gltf` v4): `JOINTS_0`/`WEIGHTS_0` as a trailing skin section of the `MESH` payload, glTF skins as `skin/<s>` (joints by node name, inverse bind matrices, rest pose, skeleton root) and animations as `anim/<a>` (translation/rotation/scale channels by node name, step/linear/cubic-spline), the prefab's `fynkel.Skin`; sub-asset ids from the sidecar or derived; external files denied by default; limits; `ImportedPrefabs`, the document crate's provider for `<source>#prefab` (M3-19a); FBX (M3-19b, `fynkel.fbx` v1 over `ufbx`): the glTF importer's twin through one shared module (`graph.rs`) — meshes per material part welded back to shared vertices, `fynkel.pbr@1` materials from the PBR view of Phong/Lambert or real PBR materials, embedded PNG/JPEG textures (external ones only with `allowExternalFiles`, and then cache-key inputs), skins in cluster order with the clusters' bind matrices, clips baked at the file's rate and named after their stacks, the hierarchy as `prefab`; units and axes converted at import; everything else a named skip or refusal; the rig cook (M3-19b): a rig document cooks to its `prefab` chunk and one `ANIM` per clip matched to the skeleton by bone name (`retarget`: `A0106` for a bone the skeleton lacks, `A0105` for a dropped prop channel), runtime dependencies the skeleton only while the cache key covers every clip file; clip markers (M3-19c, `fynkel.gltf` v5, cache v5, FYNI v4): a glTF animation's `extras.markers` kept on the clip, a rig's `markers` merged at cook (one past the end dropped with `D0502`); HDR environments (E1 part 1, FYNI v6, cache v6): a Radiance `.hdr` → linear `f32` RGB radiance, cooked as an RGBA16F `TEXR` (format 3) plus an `ENVM` chunk — `fynkel_import::environment` prefilters it into a 128² six-level GGX specular cube and a 32² irradiance cube (E/π) with the mean irradiance, deterministic, the mapping and face order the shaders' — read back through `Assets::environment` | **Solid for static and skinned meshes, TRS clips, 8-bit and HDR textures and PCM/Vorbis clips** — no morphs, Draco, FLAC/MP3; FBX blend shapes, cameras, lights, NURBS, layered textures skipped | 51 tests + the rig fixture (`fixtures/rig.py` → `rig.glb`: a skinned column on three joints, wave and bob clips; duplicate joint names refused; `fixtures/blender_fbx.py` → `rig.fbx`/`robot.fbx`/`robot_external.fbx`, the same content through Blender's FBX exporter, compared number for number with the glTF path — M3-19b) + the Khronos sample rigs (`fixtures/khronos/`: Fox 24 joints / 3 clips, CesiumMan and RiggedFigure 19 joints / 1 clip — every vertex skinned, clip durations to the centisecond, deterministic; M3-19a); 3 committed expected intermediates; the Column rig cooked, invalidated per file and refused on a robot skeleton (`tests/cook.rs`); a 2 MiB / 79 k-triangle GLB imports in 16 ms |
| `fynkel_import::cache` + `::cook` + `fynkel_cooked` | Content-addressed cache (blake3 over source, settings, importer/format versions, dependency keys, profile; `objects/<key>.fyna`, `index.json` with the project's name, startup scene and the build's fingerprint, `--prune`) and the cooker (`cook_project`: every scene validated and expanded — prefabs applied, ids derived — into its object's `JSON` chunk, a cooked chunk per prefab; sources cooked first so a scene instancing `<source>#prefab` reads the hierarchy from the source's own `prefab` chunk — M3-19a); `FYNC` v1 container with CRC-checked header and chunk table, bounded reads, dependency-free reader (`MESH`/`TEXR`/`CLIP`/`JSON`/`MATL`/`BLOB`; `MATL` is the `PbrMaterial` wire record the renderer consumes) | **Solid for what is imported** — no compression or per-profile transforms (full mip chains since M3-08b); `CLIP` has no consumer; no GC beyond `--prune` | editing a texture re-cooks exactly its dependents (4 of 6 on the fixture); cook miss 20 ms / hit 12 ms / partial 13 ms; flipped byte caught on read-back |
| `fynkel_assets` | The runtime store over the cache: `Assets::open` refuses a cache from another build, `scene(id)` → the expanded description + behaviours + asset closure, `prefab(id)` → an authored prefab's chunk or an imported source's `prefab` chunk (M3-19a; `FYN-R0006` for a source without a hierarchy), meshes/textures/materials by `AssetRef`, `register_animation` putting a scene's clips, blend spaces (a space once, its points' clips with it; M3-19c) and skins into the world, `retain(closure)` → `Residency` token, drop evicts what no scene holds; `FYN-R0001`–`R0009` name the asset id, never fall back to source | **Solid** | 5 store tests; the render gate renders from a cache whose source tree was deleted; ABI tests load a copied cache and refuse a tampered one |
| `fynkel_render` | wgpu renderer over the extract (`docs/renderer.md`): Cook-Torrance (GGX, Smith, Schlick) in physical units — every `fynkel.pbr@1` factor and map (base colour, normal via cook-generated tangents, metallic-roughness, occlusion, emissive), `alpha_mode: mask`, double-sided — into a linear HDR `Rgba16Float` target exposed by the camera's EV100 (`Camera.exposure`, or metered from the sun) and ACES tone mapped; one sun, constant ambient or image-based lighting from the sky + optional equirectangular sky (`fynkel.Environment`), point/spot lights (`fynkel.SpotLight`) culled per object — a frame pool of 64 that the camera frustum filters, the 8 that reach an object shading it (E1) —, perspective or orthographic camera; the `shadow → depth prepass → ambient occlusion → AO blur → opaque → transparent → bloom → post → presentation` chain: the sun's 3×3 PCF shadow from four texel-snapped cascades in a 2×2 atlas of one 2048² map, each fitted to its slice of the view and tightened to the casters' bounds, cross-faded at the seams (R1; `cast_shadows`/`receive_shadows` per mesh), screen-space ambient occlusion from a camera depth prepass — 12 hemisphere samples at 0.6 m, blurred 4×4 bilaterally, multiplied into the ambient and IBL term only (R2; off until a volume gives it an intensity since 2026-09-24, as in Unity; `ambientOcclusion` per quality profile, and with it off the chain and the picture are exactly what they were), post-processing volumes (R3a: `fynkel.Volume` global/box/sphere with blend distance, weight and priority naming a `.volume.json` profile of optional overrides, blended once a frame at the camera — the AO radius and exponent are its first fields, the quality profile the ceiling), bloom (R3b: the profile's `bloom` block — a Karis-weighted soft-knee prefilter, a 13-tap downsample and a scatter-blended tent upsample over up to six half-resolution mips, added before ACES; off until a volume gives it an intensity, `bloom` per quality profile), colour grading (R3c: the profile's `colorGrading` block — white balance, contrast, colour filter, hue shift, saturation, the tone mapper ACES/Neutral/None, lift/gamma/gain — arithmetic in the post pass, skipped while neutral so an ungraded frame is unchanged to the bit), `blend` materials back to front in the transparent stage, MSAA 1/4 resolved through the post pass's coverage, render scale with a presentation pass, the project's `quality` profiles read from the cache and switched at runtime (renderer, Apple ABI, `FynkelViewportController.quality`, `fynkel render --quality`), debug lines/AABBs; one instance buffer per frame with one draw per distinct `(mesh, material)` pair, every instance frustum-culled against the camera (and, for casters, the sun's box) on its cooked bounding sphere before upload, `draws`/`instances`/`culled` counted; the skinned path (M3-19): a second vertex buffer of joint indices and weights, the frame's joint palettes in a storage buffer at bind group 2, `vs_skinned` in the mesh and shadow shaders (`model = instance · Σ wᵢ · palette[jᵢ]`), skinned instances batched with the rest, a skinned mesh without a resolved skin drawn in its bind pose, `skinned`/`joints` in the stats, `fynkel render --steps`; thirty fixed pipelines, bind groups cached per material/texture, evicted with the store; image-based lighting (E1 part 2) when the sky is an `.hdr`: the cook's prefiltered specular and irradiance cubes plus a renderer-built BRDF LUT sampled split-sum in `mesh.wgsl`, `rotate_sky` shared with the sky pass, metered into the auto exposure, the constant ambient kept as the fallback with no new pipeline variant; placeholder cubes, magenta for a missing mesh; the overlay (item 9f, ADR-0019): lines as anti-aliased screen-space quads with a width, flat triangles and an analytic ground grid, drawn after post into the host's target (never exposed), faded to a quarter behind the scene unless on top, `DebugDraw` its client at 1.5 px | **V1 (c) + skinning + IBL + light culling + cascades + AO + volumes + bloom + grading** — the same casters drawn into every cascade, ambient occlusion is screen-space (what is off-screen or behind does not occlude, and blend instances take the occlusion of what is behind them), no light clustering (the per-object assignment is a CPU loop over the pool), normals skew under non-uniform scale | 27 per-feature pixel tests (shadow, the cascades over a sixty-metre corridor, the ambient occlusion in a cube's contact corner and the open floor it leaves alone, volumes shaping it by where the camera stands — off at intensity 0 exactly as the profile's off, capped by the profile, a wider radius, priority, a box's fade —, bloom spreading past a bright cube's edges — off exactly as the profile's off, the threshold keeping the rest, tint and scatter —, colour grading moving an orange cube's colour as each control says — a neutral grade identical to none, the clear colour never graded —, per-mesh opt-outs, blend order, MSAA edges, render scale, profile switching, debug lines, culling, the overlay's width, anti-aliasing, occluded fade, exposure and MSAA independence, triangles and grid, the Rig scene's posed column against its bind pose, an HDR sky lighting a cube through its cooked environment, two cubes each lit only by the lights that reach it among them) + the fixture project (rusted robot with swatch, normal and metallic-roughness maps under its `Sun`, `low`/`medium`/`high` profiles) rendered from its own camera from the cache alone; exposure, shadow fit, quality clamp, culling and debug unit tests; `fynkel render` writes a PNG with `ev100`/light/shadow/draw/culled counts; runs in every host; M0 capture `2026-09-20-12859e1` unchanged at MSAA 4 + shadow pass (122/120 fps, engine CPU ≈ 0.1 ms); stress capture `benchmarks/m3/results/2026-09-20-cf2b3eb`: 5 804 entities, 5 343 instances in 15 draws, 458 culled, 122 fps standalone / 120 Flutter; its many-lights variant at `2026-09-22-64221f7` (256 lights over the field): 121 fps, prepare 2.10 ms p95 against 1.64 without them; R1's capture `2026-09-22-5abaac3` (four cascades): 18 checks green, 122 fps, shadow pass 0.55 ms p95 for four times the draws, scene pass +0.7 ms; R2's `2026-09-22-94490ad` (ambient occlusion): 18 checks green, the frame rate unchanged, and the stress scene's GPU frame **down** to 3.07 ms p95 from 3.56 — at 5 530 instances the depth prepass buys more in early-Z than the AO stage's 1.81 ms costs — while the 53-instance reference game, whose GPU idles between passes, went 4.01 → 6.11 ms |
| `platform/apple` + `package:fynkel_flutter` + `hosts/flutter_apple` | IOSurface ring → Flutter `Texture` (Path A), 0 copies, GPU-completion hand-off; macOS + iOS. `package:fynkel_flutter` is the Flutter plugin a game depends on: Swift `FynkelFlutterPlugin` (texture registration, multi-viewport by `textureId`, ordered teardown), `FynkelViewportController` (create/resize/rebind, adopts the world's asset store, placeholders, camera, stats), `FynkelViewport` widget (layout × DPR, focus, pointer in physical px), `FynkelGameView` (Ticker loop: `World.frame` then render, lifecycle mute, per-second `FynkelFrameStats`, overlay), `FynkelInputCapture` (keys, mouse, pointer capture; reset on focus loss/pause), Swift `FynkelInputPlugin` (gamepads via `GCController` on both targets, captured-mouse deltas on macOS) and `TouchOverlay` (sticks and buttons for `touch:overlay/*`); HUD bindings `ModelBinding`/`ModelBuilder`/`ModelValue.listenable` and world labels `ProjectionLayer` (owns its children's anchors, viewport from layout, a `Flow` repainting on the view's listener — no rebuilds per frame, unpainted and unhittable when not visible) with `ProjectedAnchor`/`WorldLabel`, the view updated by `FynkelGameView` after every frame (M3-13, `docs/hud.md`). ``DartHeapProbe` (M3-16, C3: the Dart heap, bytes allocated and GC per window from the app's own VM service; `FynkelGameView(profileDartHeap:)`) and `FynkelFrameStats.summary()` (the `[stats]` capture line). `hosts/flutter_apple` is the measurement harness as a thin app on it; `FYNKEL_PROJECT=<dir>` shows a project's startup scene from its cooked cache | **Solid** | 120 Hz, lag 0, raster p95 0.96 ms after the port (M0 capture `2026-09-19-b35f603`); iOS simulator 60 Hz, 0 drops; package tests create → render → resize → detach against the real Rust viewport under `flutter test`; 60-lifetime leak test |
| `fynkel_authoring` + `fynkel_native` feature `authoring` + `package:fynkel_authoring` (M4-01, branch `editor`) | The editor's session over `fynkel_document` (ADR-0016): one open project (file, lock → read-only when newer, embedded + behaviour schemas, asset database with every reference indexed, committed behaviour metadata) and one authoring `Editor` per open scene, prefab or material behind an integer handle; `apply`/`undo`/`redo`/`save`/`reload`/`rescan` and fourteen read-only queries (`document`, `inspect`, `validate`, `validateProject`, `assets`, `asset`, `dependents`, `schemas`, `schema`, `metadata`, `migrations`, …), all JSON; the `fynkel_authoring_*` C ABI (`include/fynkel_authoring.h`, NULL-safe, panics caught and the session poisoned, one result buffer per call) compiled only with the cargo feature `authoring` — `package:fynkel`'s hook builds every runtime `--no-default-features` and adds the feature only for a pubspec that asks (`hooks.user_defines.fynkel.features`), so a game's runtime has no editor code; `package:fynkel_authoring` is the typed Dart surface (ffigen bindings, `AuthoringSession`/`DocumentHandle`/`Command`, a `Stream` of events). D5 and D7 closed (`docs/editor.md`, `docs/documents.md` § The authoring session) | **Solid** | 9 session tests + 4 ABI tests (Rust) and 5 Dart tests driving a complete session over the ABI on a copy of the fixture project (all seventeen commands, undo to the original bytes, save and save-as, the stale-revision conflict, a material's `base_color`, `duplicateObject` of the rig); CI builds the runtime without the feature and fails on any `fynkel_authoring_*` symbol; 16 000 entities / 8 000 warnings read + validate in 99 ms release (was 726 ms) |
| `apps/editor` — the editor shell (M4-02, branch `editor`) | The Flutter macOS app on `package:docking` (spike: seven criteria, ADR-0016): launcher (New over `fynkel create --json`, Open, Recent, doctor checks for CLI/checkout/Flutter/ABI/IDE), the §22.2 workspace saved per user with Reset layout, a native menu bar + command registry + palette (23 commands with stable ids, each reachable by menu, shortcut and palette), the Console over the session's diagnostics (exact `file:line:column`, filters, collapse, copy, open in the IDE), read-only banner with the lock's reason (+ `FYN-P0011`), a migration preview over the session's new `migrations` query (steps, diff, the command; writes nothing), preferences (`~/Library/Application Support/Fynkel Editor/`). The editor runs no game code and shells out only to `fynkel create` so far (`docs/editor.md` § The shell) | **Built; the gate is the user's to drive** | 45 tests in `apps/editor` (docking spike 7, commands 6, palette 3, console 4, launcher 11, workspace 7, real session 7: the fixture project and the reference game open and validate, a misspelt field is listed at `main.scene.json:75:13`, a lock naming runtime 9.0.0 opens read-only with that reason, a v0 scene previews as `0 → 2`, the watcher revalidates); `fynkel_authoring` 10 Rust + 6 Dart tests cover `migrations`; `flutter build macos --debug` succeeds |
| `apps/editor` — document tabs and the Hierarchy (M4-03, branch `editor`) | Tabs over the session's documents: Open Document (⇧⌘O) and Console → tab, dirty dot, ⌘S / Save As (never over an existing file) / Save All / Revert / Close with the save prompt, undo and redo per tab with a counted label (text fields keep their own), a file watcher that reloads a clean tab silently and turns an external write under a dirty tab into a conflict dialog with three revisions and a unified diff (Reload / Save As / Keep editing — the `FYN-E0001` path, never an overwrite); the Hierarchy as a virtualised tree built once and patched from `affectedIds` through the `document` query's `ids` form, with select / multi / range, rename in place, Duplicate, Delete, Create empty / with a native component (defaults from the new `defaults` query), drag-and-drop reparent with an explicit index and keep-world-transform `setField`s in the same transaction, search over name / id / component type, filters for component, prefab state and has-error, prefab and error badges. Three read-only queries added to `fynkel_authoring` (`document` with `ids`, `defaults`, `revision`); no shared crate touched (`docs/editor.md` § Document tabs and the Hierarchy) | **Solid** (driven by the user on a copy of the fixture project and the reference game; the gate found and fixed a Save As path bug, the tab label after Save As and a placeholder overflow) | 86 tests in `apps/editor` (+41: documents 18, hierarchy 16, line diff 4, real session 3 — every Hierarchy operation one transaction on the fixture's Main scene, saved and inspected and reopened; the conflict path through the real watcher with save-as and the rig scene's silent reload; a generated 10 000-entity scene opens in 262 ms and edits in 307 ms without a rebuild); `fynkel_authoring` 11 Rust + 6 Dart tests (`defaults_cover_every_authored_component`) |
| `apps/editor` — the Inspector (M4-04, branch `editor`) | The selection's components as cards, generated from the schemas (native) and the committed metadata + schema (behaviours) through one parser with the ADR-0018 hints — `label`/`tooltip`/`min`/`max`/`step`/`group`/`order`/`header`/`unit`/`kind`/`readOnly`/`hidden`/`showIf`, `icon`/`category` — now emitted by `package:fynkel_generator` from `@Expose`/`@FynkelBehaviour` (an unhinted class emits what it did before) and, for the built-ins, supplied by an editor table until the Rust attributes exist; one widget per kind (number with label drag = one transaction, integer, bool, string/multiline, enum, vec2/3/4 by pointer, Euler degrees over the quaternion, colour picker linear↔sRGB, asset picker by kind + drop target, entity names, layer mask from `physics.layers`, arrays, objects, JSON fallback); multi-object editing with mixed `—` and one transaction over all selected; precheck (bounds, enum, integrality, unknown asset `FYN-A0004`) plus the session's validation, the value back and the message kept until fixed; Add Component (searchable, native defaults / schema defaults; a type the entity holds is greyed out — one per type, `FYN-D0204`) and Remove; a rejection shows the crate's `related` reasons, not the `FYN-E0005` summary, and clears with the selection, the next accepted edit or ×; prefab override list with Revert / Revert all; Open script in the IDE; the codegen banner from committed drift and `fynkel codegen --check --project <dir> --json` (new `--json` in `apps/cli`, run once at open and on request). ADR-0018 accepted with the native-table note (`docs/editor.md` § The Inspector) | **Solid** (driven by the user in two rounds on copies of the fixture project and the reference game; round one found a second Transform offered, a bare `FYN-E0005` summary and a message that never cleared — fixed with tests, blueprint item 4) | 116 tests in `apps/editor` (+31, two shell tests updated: inspector 22 — every native schema parses with a real kind, the hint table names only real fields, slots/mixed/one transaction, precheck and the document's refusal, add/remove/reset/revert, drift, the CLI check's arguments and report, Euler and colour maths, JSON int-ness; widgets: a drag is one transaction, ↩ commits an expression, a `min` violation shows and reverts, checkbox/enum/showIf/mixed, the asset picker by kind, Add Component and the banner, layer mask and the colour picker — and real session 4: every field of the fixture's Main and Playground and the reference game's playground set to itself saves byte-identically, `speed < 0` refused by the precheck and by the crate with the document untouched, an unknown asset refused, Add/Remove/revert on the real document); `fynkel_generator` 6 tests, its example regenerated; `fynkel_cli` check tests 16 |
| `apps/editor` — the Project browser (M4-05, branch `editor`) | Folders and a searchable flat list with kind filters (live counts) and sort; decoded texture previews, material swatches, icons; the **Asset** panel (item 2's Import placeholder): sidecar import settings as controls (`colorSpace`, `stream`, `allowExternalFiles`) plus raw JSON, **Apply and Reimport** with the importer's summary or refusal, sub-assets draggable, dependents that open the referencing scene at the entity; New Scene / Prefab / Material / Folder, inline rename, move by drag onto a folder or a picker, delete to `.fynkel/trash` refused while referenced (dependents listed, **Delete anyway**), Reveal in Finder, Copy id; every change through the session's new `operate` (`newDocument`, `renameAsset` with the sidecar so the id survives and open editors relocated, `deleteAsset`, `setImportSettings`, `reimport`, `undoAsset`/`redoAsset` — an **asset history** of its own, ⌥⌘Z) and `assetDetail`/`assetHistory` queries (`fynkel_authoring`, ABI **2** with `fynkel_authoring_operate`); `AssetDatabase::update` re-reads only the touched paths (a 2 000-asset tree: full scan tens of ms, one change well under 1 ms); a prefab or `robot.glb#prefab` dropped onto the Hierarchy is one `instantiatePrefab` transaction; an image is a whole `texture` (`AssetEntry.wholeKind`, which also lets the Inspector's picker offer textures); 15 `assets.*` commands | **Solid** (gate driven by the user on copies of the fixture project and the reference game, 2026-09-21) | 140 tests in `apps/editor` (+24: the model — views, search, filters, sort, selection, previews and drag payloads, every operation and its refusal, read-only, revision refresh; the Project panel — tree, list, search, status, double-click, New ▾ + inline rename + ⎋, the refused-delete dialog and Delete anyway, the error bar, a row dragged onto a tree folder; the Asset panel — colour space → Apply and Reimport, dependents click, a refused reimport, the material swatch, Open; the Hierarchy drop of a prefab and an imported `#prefab`, refused kinds and prefab-instance parents; the shell's panels and `assets.*` commands; and 6 on the real session over a copy of the fixture: rename/move keep the id and validate clean, refused → forced → undone delete with the trash checked, new documents, settings to the sidecar + reimport + a refused setting, folders); `fynkel_authoring` ops tests incl. the 2 000-asset timing; `fynkel_native` ABI 2 test |
| `apps/editor` — the Scene view (M4-06, branch `editor`) + the edit-preview world | A `FynkelViewport` per scene or prefab tab over the session's **edit-preview world** — a `fynkel_core::World` in `WorldConfig::edit_mode` (no system ever runs; `crates/fynkel_authoring/src/preview.rs`, owned by `fynkel_native`'s `PreviewSlot`) holding the live document with prefab instances expanded and an anchor map, cooked in process by the first preview, kept current by an **incremental diff sync** after every session call (fallback reload when a round makes no progress; `Preview::dump` proves the synced world equals a fresh load after every transaction kind) — authoring ABI **3** (`fynkel_authoring_preview_open/_close/_world/_call`: camera, hidden, pick, pickRect, lines, worldMatrix, setTransforms, restoreTransforms, resync, reload, recook, dump, stats; `"previews"` reports) and `ScenePreview` in `package:fynkel_authoring`; the editor camera (orbit / pan / dolly / fly, perspective ↔ orthographic, frame, look through / write back the scene camera in one transaction) saved per scene in `.fynkel/editor.json`; grid, axes, light / camera / collider icons and the selection outline as renderer **debug lines** and **Save Frame** as a PNG through the viewport ABI **4** (`fynkel_apple_viewport_debug_lines`, `fynkel_apple_viewport_read_frame`; `FynkelViewportController.renderHandle`/`debugLines`/`readFrame`); click picking through the world (stacked-hit cycling, ⌘/⇧), marquee; move / rotate / scale gizmos in world or local axes with snapping, previewed in the world and committed as **one transaction** on release (multi-selection about the lead, locked and Transform-less entities explained in the status line); editor-only hidden and locked flags; prefab drop at the hit point; stats overlay; Reload and Rebuild cache; 19 `scene.*`/`view.*` commands The panel draws on demand, so an idle Scene view costs nothing — and a **window or splitter drag** no longer blinks it: `FynkelViewport.resizeDebounce` (new, `Duration.zero` for a game) holds the last frame scaled for 120 ms and resizes the ring once when the drag settles, and a frame the ring refused mid-resize is asked for again instead of being dropped (found by the user 2026-09-22, fixed with M4-08; `docs/editor.md` § Resizing the panel). | **Solid** (gate driven by the user on the fixture project and the reference game's Playground, 2026-09-22) | 194 tests in `apps/editor` (+29: scene_view 25 over fakes — previews per tab, picking, cycling, marquee, every gizmo drag one transaction with the world equal to the committed matrix, snapping, cancel, multi-selection, hidden/locked, camera commands, drop, the panel's hotkeys and toolbar, the shell's commands; scene_session 4 over the real world — every entity of every fixture scene picked by a ray through its centre, a drag one undo step with the incremental world equal to a reload, hidden never in the document, nothing ticks, the scene camera aligned and written back, a real Metal frame read back and equal to itself); `fynkel_authoring` `tests/preview.rs` (sync == fresh load after every transaction kind, picks, outlines, hidden, anchors); `fynkel_native` ABI 3 tests; `platform/apple` debug-line and read-back tests; `fynkel_flutter` controller tests; the editor's clean 320×200 frame of the fixture's Main through its camera equals `fynkel render --size 320x200` — 0 differing pixels |
| `apps/editor` — Prefab mode (M4-07, branch `editor`) | **Since ADR-0022 (2026-09-24) a prefab has one root and the instance row *is* that root** (its components shown and edited as overrides on the root, placement a `setField` on the root's Transform, `World.instantiate` returns the root; formatVersion 5 with the cross-document migration); what follows was written for the two-level model it replaced and reads with "anchor" = the instance. A prefab instance opens in the Hierarchy to its **derived rows** (the session's new `instance` query — the expansion of one anchor with every `setField`/`unsetField` field and every added component or object marked, live against an open prefab tab's unsaved edits; italic inherited, bold with a count when overridden, `+` when added); every edit on a derived entity is a `setPrefabOverride` transaction on the anchor (rename of added entities, delete as `removeObject` or the revert of the adding `addObject`, create as `addObject`, field edits and the Scene view's gizmo drag as whole-key `setField`, Add / Remove Component); the Inspector shows overridden fields **bold** with per-field **Revert** and **Apply**, the anchor's section lists the overrides with Revert / Apply each and **Apply to prefab** (one transaction on the prefab document, then one `revertPrefabOverride` on the scene — two undo steps in two tabs), **Revert all**, **Unpack** (`unpackPrefab`, one transaction, the derived ids kept so the preview world needs no reload), **Open prefab**, **Select prefab asset**; imported hierarchies (`<glb>#prefab`, `<rig>#prefab`) open read-only in their own tab (`openDocument("<id>#prefab")`, `state.sub`) under the derived `node/<i>` ids and take overrides; a missing or broken prefab (`D0401`/`D0403`/`D0404`/`D0407`) is shown with its diagnostics, keeps the instance's data and is never rewritten; five `prefab.*` commands (menu, palette, shortcut, context menu); shared touches: `unpackPrefab` + `Editor::apply_with_prefabs` in `fynkel_document`, `fynkel scene|prefab apply --prefabs` in `apps/cli` | **Solid** (gate driven by the user on copies of the fixture project and the reference game's Playground, 2026-09-22; the one finding — no gizmo on a derived row — fixed the same day) | 219 tests in `apps/editor` (+25: prefab_mode 19 over fakes — expansion caching, the missing prefab, every derived-row guard, delete / create / rename derived, arrows, `revealAndSelect` into an unopened instance, derived slots and override ops, add / remove component, apply as two undo steps with the prefab-change broadcast, per-field apply and the refused prefab, unpack, the imported read-only tab, live prefab-tab edits, the five commands, the context menu, the read-only banner, the panel's keys; scene_view: a gizmo drag of a derived entity is one `setPrefabOverride` on the anchor with the prefab untouched, and scene_session repeats it through the real world (incremental sync, one undo); prefab_session 5 over the real session on a copy of the fixture — Crate A's override reverted and re-added and the saved scene reloaded clean, the coin instanced / overridden / applied to `coin.prefab.json` / undone per document / saved / unpacked / reloaded, the Imported rig overridden and unpacked with Apply refused, a broken reference opened and shown with the file bytes unchanged, `rig.glb#prefab` opened read-only and a texture's refused — every override list checked against the raw document and `inspect`'s count); `fynkel_authoring` `tests/prefab_mode.rs` (the query live against an open prefab, apply as two transactions, `unpackPrefab` == `expand`, imported hierarchies, the broken cases left alone); `fynkel_document` `tests/authoring.rs` (`unpackPrefab` round trip, refusals); `package:fynkel_authoring` `session_test.dart` (`instance`, `#prefab`, `unpackPrefab`); `apps/cli` `tests/check.rs` (`apply --prefabs`) |
| `apps/editor` — Play / Stop and the game Console (M4-08, branch `editor`; since ADR-0021 P6 this is **Play → Build and Run**, ⌘B) + `package:fynkel`'s editor channel | ▶ (⌘P) saves what is dirty (prompt) and runs `fynkel run --project <dir>` — the third and last CLI command the editor shells out to — with `FYNKEL_EDITOR_CHANNEL=stdio` added to the environment; `package:fynkel` then writes **one-directional JSON lines on the game's own stdout** (`EditorChannel`, ADR-0016): `log` (`Diagnostics.log`, with the default print hook quiet so nothing is shown twice), `diagnostic` (every step and load diagnostic as its §14.2 record, with the authored entity id when the load report knew one), `stats` (`FynkelGameView`'s window, whether or not the game passes `onStats`) and `state` (`started` with the pid, `loaded` with the scene and counts, `loadFailed`) — and nothing at all when the variable is unset. The Console gains a **Game** tab beside Project (tick, source, code, message, place; the same filters, search, collapsing and ⌘C, plus Output and Stats switches; a row opens its document with the entity selected, or reveals the asset); Play names the desktop (`-d macos`), so `flutter run` never picks a phone, a simulator or Chrome and never refuses to choose; a run that dies before the game starts (a failed build, no device) says *Did not start* and the Console turns its Output switch on by itself, because the reason is in the process's own lines. The toolbar shows ▶ / ■ and the run's state, ■ (⌘.) kills the **process tree** (`ps` read before the first signal, SIGTERM deepest-first, three seconds, then SIGKILL), a project with errors refuses to Play and the Console comes up on Project, and an edit during a run is marked ("changes apply on the next Play", whose **Play again** stops the run that is up before starting the next one — ▶ itself refuses while a game runs; the banner and the suffix go the moment the game does, and a run Stop ended reads *Stopped* rather than its signal's code); four `play.*` commands (`docs/editor.md` § Play and the game Console, `docs/hosts.md` § The editor channel) | **Done** (gate driven by the user 2026-09-22 on a copy of the reference game; five findings fixed the same day — the device `flutter run` could not pick, a row whose document never opened, the Scene view blinking on resize, *Play again* doing nothing, and a banner that outlived its run) | 242 tests in `apps/editor` (+23: play 21 — the records behind a `flutter run` prefix, the process tree from a canned `ps` table with the polite and the stubborn case, a whole run over a fake process (environment, the named device, `starting → running → exited`, a crash's code, a run that dies before the game starts with the Console showing the output that says why, dropped rows, no CLI), the Game tab's filters and collapse, and the shell — Play refused on a project with errors, the Game tab's rows, a diagnostic row that opens the scene with its entity selected, Stop, the banner and its Play again, the save prompt's three answers; play_process 2 against a **real** process — Stop takes a grandchild with it, a `kill -9` ends in the exit code with the records it wrote still shown, and both leave every document byte-identical); `package:fynkel` `editor_channel_test.dart` 5 (the records, the sequence, the silence when the variable is unset) |
| `apps/editor` — Play in the editor (ADR-0021 P2–P6) | **The project editor:** `fynkel editor <dir>` generates `<project>/.fynkel/editor_host`, a Flutter app that is the editor (`package:fynkel_editor`, `runFynkelEditor`) with the project's game compiled in, and supervises it over `flutter run --machine`: hot reload of `lib/` on save, a **Restart to Apply** for a change a hot reload cannot apply (saves first, then the open tabs, the one in front and each selection come back — they come back on every open), `status.json` for the editor to follow. **Fast open:** content fingerprints of the native and the Dart inputs (`build/fynkel_launch.json`); nothing native changed → the built app is launched directly and `flutter attach`ed (a project editor up in ≈0.3 s instead of 11–20 s), older Dart code caught up by one hot restart behind a *Loading code changes…* cover, and the app rebuilt after it quits. **Handover:** opening a game project from the launcher (the plain editor, Unity's Hub) starts its project editor detached, in the launcher's window frame, and the launcher quits; an editor started on a project says *Opening …* until it is open. **The Game view:** Play (⌘P) cooks — in process, from what the editor holds, unsaved edits included and nothing written (P5) — and mounts the game in the Game panel beside Scene, playing the open scene, with Pause / Step in the title bar, the keyboard given to the game, its Quit ending Play, its channel records in the Console; between runs the panel shows the active camera. **Build and Run** (⌘B) keeps the separate process for release-like speed or an isolated crash; in the plain editor Play is Build and Run. Not done: live Play mode — the Hierarchy and Inspector showing the running world, changes discarded on Stop (P7, its design note first) | **Solid** (driven by the user 2026-09-23 on copies of the reference game: Play in the Game view, the handover, fast open; their findings — a handover lost to the catch-up restart, the launcher page flashing while the project opened, the window jumping to another place — fixed the same day) | 361 tests in `apps/editor` (the Game view and Build and Run over fakes, P5's live cook, the handover and its status file, Restart to Apply and the session restore, the *Loading code changes…* cover, the *Opening* screen); `apps/cli` `editor_launch` fingerprints and the handover status file; live checks of the marker file, the rebuild after quit and the window frame through `CGWindowListCopyWindowInfo` |
| `apps/editor` — the look, the title bar, one Inspector, one picker, panel copies (items 9a–9e) | **The look (9a):** design tokens as a `ThemeExtension` (surfaces, one hairline, three inks, one accent for selection / focus / "on", diagnostics' own hues; dark and light as two sets) and an explicit `ThemeData` in `lib/src/theme/`, the Density preference (18 / 20 / 24 px rows), the docking tabs and splitters from the tokens, Lucide icons (`lucide_icons_flutter`), a keyboard focus ring drawn by one layer (`:focus-visible` rules), notices in the status bar instead of snackbars, nothing animated but hover. **The chrome (9b):** one 38 px title bar that is the macOS title bar (transparent, an empty unified-compact `NSToolbar`, `isMovable = false`, drag and double-click over the `fynkel.editor/window` channel) holding the project menu with recent projects, the document tabs, Play centred, the codegen state and the command search; the Scene view's toolbar floating over the picture in frosted strips; the Inspector read left to right with units; the accent following focus; the Project list in columns. **One Inspector (9c):** `EditorSelection` — entities, an asset, a part of one, a folder — with the Asset panel's content as the Inspector's asset / part / folder pages, import settings applied or reverted from a pinned bar, a lock, document kinds' sections registered by kind (`AssetDocumentSections`); the Asset panel retired from saved layouts. **One picker (9d):** `showPicker` for asset and entity slots, Open Document, Add Component, Create Entity with Component and the palette — search, groups, None, a preview, ↑ ↓ ⏎ esc. **Panels (9e):** tabs with the × on hover and a right-click menu, trackpad zoom, the orientation gizmo (click an axis to look from it, the caption names the view), copies of the Hierarchy, Scene view, Inspector and Project browser — each with its own state and padlock, explicit requests going to an unlocked copy (`docs/editor.md` § The look … § Panel tabs, copies and the orientation gizmo) | **9a done** (gate driven by the user 2026-09-22 on a copy of the reference game; its bug — the project opened before the toolchain was detected — fixed with a test, its judgements became 9b–9d); **9b–9e done 2026-09-23** (gate driven by the user 2026-09-23 on a copy of the reference game — passed; its one finding, trackpad scroll and pinch not reaching the live Scene view, fixed in `package:fynkel_flutter` (`FynkelViewport` now forwards the pan-zoom gesture)) | 293 tests in `apps/editor` (+19 for 9e: `panels_test.dart` 10 — the workspace's copies, the tab chrome and menu, two Inspectors, two browsers and a locked Hierarchy through the shell and a restart; two Scene views drawing one world from two cameras on real Metal; the gizmo; the copies' memory in `editor_state_test`, `editor_selection_test`, `project_browser_test`, `hierarchy_test`; `lookFrom` in `scene_math_test`); before that 274 (+32 since M4-08: `look_test.dart` 7 — the ring, the notices, no animated popup anywhere in the sources; `title_bar_test.dart` 4 — the bar, its menu, recent projects, the codegen state, the macOS channel mocked; `editor_selection_test.dart` 7; `picker_test.dart` 5; the asset pages and the kind registry in `project_browser_test.dart`; Create With through the picker in `documents_test.dart`; the startup order) |
| `hosts/headless` + `hosts/standalone` | Rust hosts: deterministic runs, cooked scenes from the cache (headless through `fynkel test --headless` `--project`/`--scene <path\|id>`/`--cache`, `--input <file.input.json>` replaying a recorded stream against the project's actions; standalone `--cache <dir> [--scene <id>] [--quality <profile>]`, never cooking), state dump by authoring id, the prefabs a scene's emitters name registered, per-system time accumulated over the run (`systems`, mean + worst tick; M3-10), the animation counts (`animation`: skinned meshes, joints, animators, clips and spaces, markers crossed; M3-19/19c); both hosts register gravity, mesh colliders, prefabs, clips and skins before the first step (the standalone drew every rig unskinned until M3-19c part 2); winit window with the renderer's counts — skinned instances included — on its stats line | **Solid** | CLI tests; the stress capture runs the standalone host on the cooked stress scene (200 rigs drawn skinned at 122 frames/s) and reads the per-system and animation tables from the headless one |
| `apps/cli` | `fynkel codegen [--project]`, `schema`, `test --headless`, `state dump`, `document`, `scene`, `prefab`, `material`, `shader`, `asset`, `migrate`, `import [--prefab] [--rig <name>]` (the hierarchy written as an authored prefab document with a fresh id, M3-19a; glTF/GLB and FBX, and `--rig` writing `<name>.rig.json` from the files — the first the skeleton, every clip named and matched by bone —, M3-19b), `rig inspect\|validate`, `cook`, `cooked inspect`, `project describe\|lock`, `validate`, `render`, `scene\|prefab apply --patch`, `create`, `run` — every command with `--json`, exit codes 0/1/2 fixed (ADR-0017); works outside the checkout on the embedded schemas; document commands know the enclosing project's behaviour types; `test --headless`, `state dump`, `render` and `run` cook first and load from the cache (`--no-cook`) | **Solid** | 18 CLI tests incl. a fresh `fynkel create` project through validate, headless, state dump and `codegen --check`, and the Khronos Fox assembled through `import --rig` → `rig inspect\|validate` → a scene on `<rig>#prefab` → cook, headless and state dump; every command under 30 ms on the fixture project except `codegen --project` (1 s warm) |
| `examples/showcase` | The renderer's example scene: an ordinary created game authored to exercise the sun's shadow (with per-mesh opt-outs), a spot and two point lights, an `Environment` whose generated sky is an HDR image that lights the scene (E1: `tools/sky.py` → `assets/textures/sky.hdr`), textured PBR, gold/chrome that reads as metal, three overlapping blend panels, a mask leaf card, the fixture robot as a hierarchy, `game.Orbit` circling the camera, three quality profiles, the reference game's rigged runner looping its run clip in place — instanced from `character.glb#prefab` with the suit material overridden, 35 authored entities, 48 live (M3-19, M3-19a); the HUD prints the viewport's frame counts (blended, lights, casters/map, MSAA, render size, GPU/CPU ms — appended to `FynkelAppleViewportStats`); `renders/<item>.png` committed per renderer item | **Solid** | CI validates, runs headless, renders (`--steps 10`), compares the runner's GLB with the reference game's and `flutter test`s it; `renders/m3-05.png` … `m3-19.png`, `e1.png`, `r1.png`; launched on macOS (34 entities, 27 meshes, 5 blended, 18 casters in a 2048² map, MSAA 4, GPU ≈ 2 ms at 1600×1200) |
| `examples/reference_game` | The first reference game (M3-14/M3-15, blueprint items 14 and 15; `README.md`/`AGENTS.md` are the tour): a Tier 1 collect-'em-all — three generated scenes, the Menu (the startup scene: a podium, ring coins and the camera on `Rotator`s, 29 entities), the Playground and the Results (the podium, a still camera, a `Jingle` source playing `win.ogg` from the load; 13) — the Playground generated by stdlib Python in `tools/` (`kit.py` → `assets/models/kit.glb`, `character.py` → `assets/models/character.glb` — the rigged player: eleven boxes on eleven joints, idle/walk/run/jump/wave clips, the walk's and the run's footstep markers —, `playground.py` → materials, prefabs, `assets/models/player.blend.json` (idle, walk, run along `speed`), `playground.scene.json` with the joint entities from `character.py`'s table, `route.py` → `test/collect_all.input.json`; deterministic, CI regenerates and diffs) — 169 entities, 113 static meshes with colliders, 22 rigid bodies, 10 coin prefab instances, an `Oscillate` blade and an `Emitter`-launched ball as `hazard` zones, a block, a switch and a gate; `game.Player` on a `CharacterController` + `ThirdPersonCamera` (sprint, jump, deaths, restart) wearing the skinned character on the locomotion blend space (the controller's planar speed is the parameter, the jump played once over it, the `wave` action playing an additive layer on the spine, `step_l`/`step_r` markers counted into the HUD — M3-19c; `tools/playground.py --hero <file.rig.json>` swaps in a downloaded character as a `Rig` child instancing `<rig>#prefab` with its clips by name, opt-in and never committed — M3-19a/19b), `game.Collectible` (a runtime-spawned burst prefab), `game.Switch`/`game.Gate`, a `GameState` model behind the HUD counter, banners and a projected prompt; the sound (M3-11 part 2): `assets/audio/` — Kenney's footsteps, pickup and hit, Juhani Junkala's chiptune (CC0, credited) —, a listener on the Camera, the `Music` entity's looping, streamed source on the music bus, a footstep per marker and the hit from `game.Player`, the pickup from `game.Collectible`, `audio: true` in the app; the flow (M3-15): `Game.play()`/`finish()`/`toMenu()` over `scenes.transition`, `GameState.screen` set from the `SceneManager` listener and switched on by the app's `ModelBuilder<Screen>`, `lastRun` snapshotted when `game.Player` finishes the run 1.5 s after the last coin, `lib/ui/` — `MenuScreen`, `PauseMenu` (Esc/`pause`, an on-screen button on touch), `ResultsScreen`, `SettingsPanel` (buses, mute, quality, rebinding captured in `onKeyEvent`, reset; `settings.json` under app support, applied after every load), the HUD under `SafeArea` with the hint hidden on touch —, every button clicking `click.ogg` on the presentation lane, `ExcludeFocus` around the overlay | **Solid** | CI regenerates, validates, runs the Menu, the Results and the Playground headless, replays the route natively (`--scene … --steps 1400`), renders, `codegen --check`, `flutter analyze --fatal-infos` and `flutter test` (8: the Menu loads and Play transitions, the route collects all ten with no deaths, the character is posed from the controller — the blend space's duration following the speed, footsteps counted, a held jump crossfading back, the wave layer raising the arm —, rig-agnostic so the `--hero` character passes too, the sounds — the streamed music from the load, one one-shot per marker, the pickup, the hit, `starved` 0 —, the sweeper kills and the fall respawns, restart resets, the flow — the route to the Results after ≈90 ticks, `lastRun` 10/10, the jingle once, the click, Play again, Give up, Menu —, the settings — defaults, `bindingsOf`, a rebind written and applied, reset, a relaunch reading the file); `renders/playground.png`; played end to end on macOS through `fynkel run` and on the iPhone simulator (the overlay, the pause button; the real-iPhone qualification stays M6) |
| `fynkel create` template (`apps/cli/templates/create/`) | A new game: Flutter app (`flutter create --empty` runners, sandbox off) on `package:fynkel` + `package:fynkel_flutter`, `lib/main.dart` = `FynkelGameView` over the project's cooked cache with a HUD overlay, `lib/game/game.dart` (`Game.load`), `game.Spinner` and `game.Mover` (walks the cube with `Input.action2D('move')`, hops on `jump`) with committed generated files and schemas, cube/material/scene with fresh ids and real import sidecars, `test/game_test.dart` + `test/walk.input.json` (a recorded stream replayed through the Mover), a kinematic cube over a static Floor and a dynamic Crate that lands on it and gets pushed (`physics` in the project file, the test replays a walk and counts landings), `FynkelInputCapture` + `TouchOverlay.fromActions` in `main.dart`, a `HudModel` (`lib/game/hud_model.dart`) installed as a context service and written by `Mover.update`, bound in the HUD with `ModelBuilder` (`MoverReadout`, `PauseBanner`), `WorldLabel`s on the Cube and the Crate in a `ProjectionLayer` (M3-13), `AGENTS.md`, `README.md`; `fynkel run` validates, cooks and runs it; every cook stages the cache into `fynkel_cache/` (bundled as Flutter assets, `pubspec.yaml`) and `main.dart` unpacks it through `unpackBundledCache()` where `FYNKEL_PROJECT` is not on the device — an iPhone, a shipped build (M3-06a) | **Solid** — the created app shows its lit, spinning cube (M3-01 gate) | CI creates one and runs `validate`, `test --headless`, `codegen --check`, `flutter analyze`, `flutter test`; built and launched on macOS by hand (1 mesh drawn, 60 ticks/s, 120 Hz) |

Size: ≈47 k lines (Rust 37.2 k, Dart + templates 8.8 k, Swift/ObjC 0.9 k,
Python 0.5 k) plus 5.5 k lines of docs, over 45 commits.

## 2. What is proven end to end

- **Dart → Rust → GPU → Flutter, every frame, zero copies.** A Flutter app on
  macOS and on an iPhone drives `step()` from the frame callback and shows the
  world in a `Texture` with widgets composited over it. (M0 + M1 gates.)
- **One source of truth for components.** Adding a Rust field regenerates the
  schema, the Dart codec and the Rust codec; drift fails the build. Adding an
  `@Expose` field to a behaviour regenerates its registration and schema the
  same way. (M1, M2-16.)
- **File → world → file.** A `.scene.json` is parsed, validated with
  positioned diagnostics, expanded, loaded into the world and dumped back by
  authoring id; unknown data survives a rewrite byte for byte; every §11.1
  authoring command applies as a transaction and undoes to the original
  bytes. (M2 gate.)
- **Source → cache → screen.** The fixture project (scene with a `Camera`, a
  `MeshRenderer` over `robot.glb` + `rusted.material.json` with the
  `swatch.png` base colour, a prefab instance with an override) is cooked by
  `fynkel cook` (the scene expanded, ids derived), loaded by
  `World.loadProject` from the cache alone and shown in the Flutter macOS
  host from the scene's own camera — 5 entities, 2 cooked meshes, 0 missing,
  120 Hz — and by `fynkel render` to a PNG headlessly; the same path is a
  pixel test that deletes the source tree after cooking and renders the
  same bytes. (Spike after M2-13; cache-only since M3-03.)
- **Authored data → compiled Dart behaviour.** A `@FynkelBehaviour` class
  becomes a schema; a scene's `game.Spinner { speed: 2.5 }` validates against
  it and comes back from `World.loadProject` as a `Spinner` with
  `speed == 2.5` that rotates its entity every tick. (M2-16.)
- **Empty folder → game project → green, on a clean CI runner.** `fynkel
  create my_game` → `validate` → `test --headless` → `codegen --check` →
  `flutter analyze` → `flutter test`. (M2-17.)
- **The M2 gate itself:** all seven criteria with the test that proves each
  (`m2-gate-report.md` §1), a clean-clone run of every suite, and a timing
  capture of every tool. (M2-18.)
- **A created project shows its scene in its own app.** `fynkel create` →
  `fynkel run` (validate, cook, `flutter run`): the starter's lit, spinning
  cube in `FynkelGameView` with the HUD over it on macOS; the same package
  runs the measurement host on macOS (numbers unchanged) and the iOS
  simulator. (M3-01.) The same app runs where the project directory does
  not exist: the cook stages the cache into `fynkel_cache/`, the app
  bundles it and unpacks it at start-up (`unpackBundledCache()`); the
  showcase loads from the bundle on macOS with `FYNKEL_PROJECT` pointing
  nowhere (22 objects retained from the temp cache) and on the iPhone by
  hand (the iOS simulator had hidden this by sharing the Mac's filesystem,
  M1-09). (M3-06a.) The stress scene runs there too: `benchmarks/stress/
  generate.py --app` wraps it in the starter app (`fynkel run -d
  <iphone>`), the HUD shows the frame rate.
- **Actions from the project file drive the game.** `inputActions` bind
  when the project loads; WASD/Space, a gamepad or the touch overlay feed
  the same `move`/`jump` the starter's `game.Mover` reads; a press during
  a 50 ms frame jumps once; `test/walk.input.json` replays through
  `flutter test` (the cube ends where the arithmetic says) and through
  `fynkel test --headless --input`; the overlay walked and jumped the cube
  on the iOS simulator. (M3-07.)
- **A behaviour reaches every native component on its object.** `get<
  MeshRenderer>()`, `set(mesh.copyWith(visible: false))`, `add(const
  PointLight(…))`, `remove<SpotLight>()` — the same commands the loader
  uses, validated the same way, read back before the tick applies them;
  a scratch starter's `Mover` spawned a twin cube with the mesh read off
  its own object and both rendered, the twin blinking through `visible`.
  (M3-07a.)
- **Bodies fall, collide and answer queries, deterministically.** A scene
  with `fynkel.RigidBody`/`fynkel.Collider` steps on Rapier inside the
  fixed tick; the fixture's Crate B lands on its Floor and sleeps under
  the headless checksum, the starter's crate lands and the kinematic cube
  the Mover walks pushes it (`flutter test`), a behaviour's
  `onCollisionEnter` sees the impulse and `applyImpulse` hops the crate,
  a sensor reports one enter and one exit, `raycast` from above hits the
  crate then the floor; the stress scene's 500 bodies settle at p50
  0.64 ms per tick with the checksum repeated. (M3-08; `docs/physics.md`.)
- **Gameplay code finds, spawns and instantiates without workarounds.**
  `world.find('Robot')!.findChild('Arm/Hand')` reads the hierarchy from
  one entity query per tick; a crate spawned in a `fixedUpdate` gets its
  body, collider and impulse in the same tick and moves; the fixture's
  Crate prefab instantiates under a named anchor, parented, with the mesh
  drawn at the next tick; a prefab containing another prefab expands
  through the cook and a cyclic pair is refused once each; every cooked
  texture carries its full mip chain and the renderer samples it (the
  showcase render `renders/m3-08b.png` is the before/after). (M3-08b.)
- **A player walks, jumps and lands.** The fixture's Playground scene has
  a `fynkel.CharacterController` Player driven by the `move`/`jump`
  actions and a `fynkel.ThirdPersonCamera` behind it; the headless replay
  of `test/platform.input.json` (W held, Space once, W released) ends with
  the Player grounded on the 0.8 m Platform, the camera 5 m behind it
  looking at the pivot, the checksum repeated; a Dart `PlayerController`
  (`packages/fynkel/example/player_controller.dart`) does the same from a
  behaviour with sprint and a double jump; the jump edge is consumed by
  exactly one tick and a resting character does not oscillate (found and
  fixed: Rapier's zero-translation depenetration shoved a resting capsule
  0.45 m into the floor). (M3-09; `docs/physics.md`.)
- **Thousands of things move with no Dart tick.** The stress scene's
  5 000 statics carry `Rotator`/`Oscillate`/`LookAtCamera`, a lantern
  `Follow`s the first body and an `Emitter` keeps 40 sparks alive with a
  `LifetimeDespawn` each; the catalogue's passes cost 0.15 ms a tick
  together (physics 2.0 ms because every mover keeps its collider —
  `physics.md`), the settled step is p50 2.34 ms, the standalone holds
  121 frames/s at p99 9.03 ms, and the `--app` build with 300 `Wobble`
  behaviours on top holds 120 fps at simulate 3.02 ms; the fixture's
  Catalogue scene runs every component headless under a stable checksum
  and from Dart with no behaviour attached; per-system time is in the
  headless report, the CLI line, `World.systemTimes()` and the starter
  HUD. (M3-10; `docs/catalogue.md`,
  `benchmarks/m3/results/2026-09-20-1263dd8.md`.)
- **A game moves between scenes at runtime, from Dart, without leaving
  the tick.** The fixture's Playground loads, a coin prefab is
  instantiated at runtime, the character walks into its sensor, the
  `TriggerZone` fires `ZoneEntered "collect"` to the `Collector`
  behaviour, which destroys the coin and asks `world.scenes.transition`
  for `Results` from inside `fixedUpdate`; the world performs the replace
  load at the start of the next frame (`[ready, pending, ready]`, 10–30
  frames end to end), the Playground's 7 entities are gone, `Results`'
  3 are there, and a `SettingsStore` on disk survives a reopen with its
  jump rebind applied on the next load. Additive loads stack, unload
  takes only what the scene still owns, and the same passes through the
  C ABI. Found on the way: Rapier's default active collision types do not
  include kinematic-vs-static, so a character never overlapped a static
  sensor — sensor colliders now opt in. (M3-12; `docs/scenes.md`.)
* **A label follows a scene object on screen and leaves when culled,
  without rebuilding a widget.** `packages/fynkel_flutter/test/
  projection_test.dart`: a `WorldLabel` in a `ProjectionLayer` over the
  fixture's Main scene sits at Crate A's projected point (computed by
  hand from the camera at (0, 2, 6), fov 1 rad, in a 400×300 layer) and
  stays on it while a scripted camera pans a metre per frame — the
  child built once, positions are paint transforms from one
  `fynkel_world_project` call per frame — and is neither painted nor
  hit-testable once the crate is off the viewport or behind the camera;
  a clamped marker stays on the edge; a label with an occlusion mask
  hides under the floor while the unmasked one shows. The same numbers
  hold in `package:fynkel` alone and in the core against the extract's
  matrices. HUD state goes the other way: the starter's `Mover.update`
  writes a `HudModel` whose fields notify on change only (one
  notification per jump, none on an idle frame) and `ModelBuilder`
  rebuilds one `Text` per changed field. (M3-13; `docs/hud.md`.)
- **A game plays end to end on the stack.** `examples/reference_game`'s
  recorded route (`test/collect_all.input.json`, 1 398 frames) walks the
  `CharacterController` through the Playground under `flutter test`:
  ten `collect` zones score through `game.Collectible`, each coin
  spawns the burst prefab at runtime and destroys its own instance,
  the `Oscillate` blade and the launched balls are passed with no
  death, the switch's hint appears, `E` opens the gate, the last coin
  wins — 23.3 s of game time in under a second, deterministic; the same
  recording on the Rust host (`fynkel test --headless --input`) walks the
  controller from its named actions to the gate (ten trigger enters).
  Restart is `scenes.transition` from inside a tick with the model
  reset; a scripted teleport into the blade respawns the player. On
  macOS the game plays with keyboard + captured mouse and a gamepad
  (M3-14).
- **A rigged character walks, and its skin follows.** The reference
  game's player is `tools/character.py`'s glTF — a skinned body on an
  eleven-joint skeleton with idle, walk, run and jump — imported like
  any model, its joints authored as entities under the Player, a
  `fynkel.Skin` on the body, a `fynkel.Animator` on idle. `game.Player`
  crossfades walk/run/idle from the stick and sprint and plays the jump
  once on the press: `test/game_test.dart` reads the animator's clip and
  duration switch, a thigh rotated mid-stride while the controller — not
  the clip — moved the entity, the jump held in the air and idle back on
  landing; `packages/fynkel/test/animation_test.dart` reads the fixture
  Rig's joint poses to the number at tick 31 (hips 0.125 up, spine 45°,
  head 90°, the head's world height), the one-shot's single
  `AnimationEnded` at tick 122, pause/resume/rewind/seek/crossfade/stop
  and every rejection as a diagnostic; `crates/fynkel_render/tests/
  project.rs` draws the posed column hundreds of pixels from its bind
  pose. The showcase's runner (`renders/m3-19.png`) is the same
  character mid-stride, casting its posed shadow. (M3-19;
  `docs/animation.md`.)
- **An imported model is a prefab.** A scene names `{ asset: <glb>,
  sub: "prefab" }` and the file's own hierarchy — joints, skinned body,
  `fynkel.Skin` — expands under the entity like an authored prefab, with
  overrides on the derived node ids; `World.instantiate(AssetRef(glb,
  sub: 'prefab'))` does the same at runtime; `fynkel import --prefab`
  writes the hierarchy out as a prefab document when authored ids are
  wanted. Proven on content nobody here made: the three Khronos sample
  rigs (Fox, CesiumMan, RiggedFigure) import clean, validate, cook, run
  headless with their joints moving between ticks 1 and 60 and render
  posed (`apps/cli/tests/check.rs`
  `khronos_rigs_instance_from_the_source_alone`); a Mixamo X Bot with
  four clips replays the Playground's route through `playground.py
  --hero` (237 entities, 0 diagnostics) and passes the rig-agnostic pose
  test; the showcase's Runner is now the instanced `character.glb` and
  renders pixel-identical to its hand-authored twin. (M3-19a.)
- **An FBX download imports as it is.** `fynkel.fbx` reads a Mixamo
  file straight from the download — units and axes converted, the
  skin's 65 clusters as joints, the take as a clip, the hierarchy as
  `prefab` — through the same intermediate, cook, world and renderer as
  glTF; the importer is checked against the glTF path on the same rig
  and robot exported both ways from Blender, and the user's four Mixamo
  files (Idle, Walking, Fast Run, Jumping) each validated, cooked, ran
  headless and rendered posed from a scene that names only the source.
  (M3-19b part 1.) Four downloads are one character: `fynkel import
  Idle.fbx Walking.fbx "Fast Run.fbx" Jumping.fbx --rig hero` writes
  `hero.rig.json` — the first file the skeleton, every file's clip named
  and matched to it by bone name — and a scene names `<rig>#prefab` and
  `anim/walk`; the Khronos Fox goes through the same path in a CLI test
  and `RiggedFigure` + `CesiumMan` are refused with `A0106`; the user's
  rig ran the reference game's route and its Dart tests and rendered in
  the Playground through `playground.py --hero <rig.json>`. (M3-19b
  part 2.)
- **A clip can layer, mark and travel.** A second `Animator` on a
  child entity is a layer over its ancestors' pose (`weight`,
  `additive`); a clip's markers — authored in the rig document or a
  glTF animation's `extras` — reach `onAnimationMarker` once per
  crossing, loops and backwards included, and are counted headlessly;
  `root_motion` takes a travelling clip's planar delta and yaw out of
  the pose and hands it to the entity's `CharacterController` (or its
  transform). The Khronos Fox's `run` with rig markers went through
  `rig inspect`, cook and headless in a CLI test; the fixture Column's
  `wave` markers reached a Dart behaviour. (M3-19c part 1.)
- **A character blends by a number.** A blend document places clips
  along one parameter; an `Animator` plays it like a clip and the world
  mixes the two clips around the parameter, phase-synced, so the stride
  follows the speed with no clip switching and the feet never jump;
  `animation.setParameter` drives it from Dart. The Khronos Fox's
  Survey/Walk/Run space went through validate, cook and headless and
  posed every joint at the midpoint of walk and run in a CLI test; the
  reference game's Player runs on one (the jump over it, the wave layer
  on the spine, the footsteps in the HUD), the user's Mixamo hero on the
  same one locally; 200 rigs pose in 0.34 ms a tick and draw skinned at
  122 frames/s in the standalone host. (M3-19c part 2.)
- **A game is heard, and a test hears the same.** The starter's cube
  plays Kenney's `jump.ogg` from its own `fynkel.AudioSource` on take-off
  and `land.ogg` through `world.audio.play` on landing, the camera
  carries the listener, `FynkelGameView(audio: true)` opens the Mac's
  output device; the same scene on the null backend (`flutter test`,
  `fynkel test --headless`) counts the same voices started and ended and
  the same `PlaybackEnded` ticks, because the world's tick — not the
  device — decides every voice's clock. 32 voices with priorities, three
  buses fed by the saved settings, and a render that allocates nothing
  (counted). (M3-11 part 1.)
- **Music streams; the game has a sound.** The reference game's
  `music.ogg` (Juhani Junkala's "Level 1", CC0, 74 s) cooks as a `STRM`
  chunk of 1.4 MB (its sidecar's `stream: true`) instead of 26 MB of
  floats and is decoded on its own thread a second ahead of the mixer,
  looping without a gap; the tick's clock still owns the end, so
  `fynkel test --headless` reports `1 sounds (1 streamed), 1 voices
  started` under the route's checksum. Kenney's footsteps play per
  `step_l`/`step_r` marker from `game.Player`, the pickup from
  `game.Collectible`, the hit on a death — one-shots by asset id,
  counted against the markers in `flutter test` with `starved == 0`.
  Content the implementer did not make, committed with credits.
  (M3-11 part 2.)
- **The game has a front, a pause and an end.** The reference game
  starts in a generated Menu scene (the camera on a `Rotator` pivot over
  a podium of coins, the music streaming), `Play` transitions to the
  Playground, the last coin sends the run to a Results scene whose
  `Jingle` source plays Kenney's `win.ogg` from the load, and every
  button clicks on the presentation lane. Esc (or the `pause` action, an
  on-screen button on touch) pauses the ticker and opens the pause menu;
  the settings panel edits `settings.json` — three buses and mute, the
  quality profile, a key rebound per action by pressing it — through
  `SettingsDocument.copyWith`/`rebind` and `SettingsService.update`,
  and the store re-binds after every scene load. `GameState.screen` is
  one `ModelValue` the app's `ModelBuilder<Screen>` switches on; the
  HUD sits under `SafeArea` with the keyboard hint hidden on touch
  platforms. Played end to end on macOS and on the iPhone simulator.
  (M3-15.)
- **Every §15.4 number is measured on its real path**
  (`docs/instrumentation.md`, M3-16 part 1): `World.frame` splits itself
  into the Dart behaviours, the native ticks and the presentation flush
  (`world.frameTimes`, allocation free), the game view's window stats
  carry that split, the engine's per-system times, Flutter's build and
  raster timings and a `totalWork` (simulate + render call) for the
  12 ms p95 target; the render call is timed as extract / prepare /
  encode / submit and the GPU's own pass times come from Metal
  timestamps (two alternating query sets, the previous frame's set
  resolved at the next draw because Apple's passes overlap, mapped
  without a wait, three or four frames late) with the renderer's GPU
  memory estimate next to them; a counting allocator is the global
  allocator of every final binary (`native_bytes`, its peak, the
  process's resident set from `task_info`, the resident assets' bytes
  in `FynkelWorldStats`, the headless report and the standalone `[1s]`
  line); every load report carries its wall time; and the subscription
  budget (C5) is a check — `world.snapshotBudget` raises
  `SNAPSHOT_BUDGET` (20, `FYN-R0013`) once per crossing with the
  snapshot still whole. Viewport ABI 3. (M3-16 part 1.) **Part 2:**
  the C3 Dart heap probe (`DartHeapProbe`: the app's own VM service,
  the `GC` timeline events' before/after heap per collection and
  `getMemoryUsage` — exact bytes allocated per window at ~20 KB of
  traffic, where `getAllocationProfile` cost 5.7 MB a call and
  allocated more than it measured; `FynkelGameView(profileDartHeap:)`,
  `FynkelFrameStats.dartHeap`, the host's and the reference game's
  `dart heap` lines, `benchmarks/m0/capture.py`'s `alloc_aot` from the
  host's `--profile` bundle), `FynkelFrameStats.summary()` (one
  `[stats]` line per window; the reference game prints it under
  `FYNKEL_CAPTURE=1` and starts in the Playground), and
  `benchmarks/m3/capture.py` recording **both scenes against the
  manifest** — the stress scene in the standalone and Flutter hosts,
  the reference game rendered, replayed headlessly on its route, in the
  standalone host and in its own app — with the **12 ms p95 total-work
  target checked**: `benchmarks/m3/results/2026-09-22-0427213`, stress
  **7.59 ms p95** (8 406 entities, 5 530 drawn, 540 bodies, 200 rigs),
  reference game **1.62 ms p95**; GPU timestamps read back in both
  apps; the headless route report with step/extract/frame/memory.

## 3. What is *not* proven yet (honest list)

| Gap | Why it matters | Owner |
| --- | --- | --- |
| **SDK packages come from the checkout.** `package:fynkel` and `package:fynkel_flutter` are path dependencies into the repo; the hook's `fynkel.lock.json` is a different shape from the project lock. | A game cannot be built without a Fynkel checkout and a Rust toolchain. | M3 item 17 / C6 (published artifacts) |
| **The renderer's lights are culled per object, not clustered.** E1 closed the carry-over: an `.hdr` sky cooks to its prefiltered `ENVM` and `mesh.wgsl` samples it split-sum (a scene whose sky is an 8-bit image, or has none, still gets the constant ambient, so metals read dark there), and each object is now shaded by the eight lights that reach its bounds out of a 64-light frame pool the camera frustum filters (`docs/renderer.md` § Lights per object). What is left: the assignment is a CPU loop over the pool per instance, and a view wanting more than 64 lights at once still drops the farthest. Shadows are four cascades to 60 m since R1, but the same caster set is drawn into every one of them (no per-cascade caster culling); ambient occlusion (R2) is screen-space, so what is off-screen or hidden behind a nearer surface does not occlude, and a blend instance reads the occlusion of whatever is behind it; instance culling is per instance on the CPU (1.4 ms of prepare at 5 800 records — fine at 120 Hz, not free). | A view with hundreds of lights loses the farthest. The loop's cost is now measured and bounded (`capture.py`'s many-lights variant: +0.45 ms of prepare for 5 571 instances against a full pool, the ceiling since the pool is capped), so what is left is a picture limit, not a performance one. | A cluster grid the day the pool itself has to grow (a scene that wants more than 64 lights in one view) |
| **Audio is 2D, three buses; `targets` unread.** No panning or distance attenuation (the listener is a gain; M5), no effects or sends, no one-shot handle, a streamed clip decodes once per voice (two voices on one track are two decoders), the ring length is fixed at 1 s, the Android device path untested; `targets` is validated and unused (`fynkel run -d` picks the device by hand). | Enough for the reference game: footsteps, pickups, a hit and a streamed loop; a game wanting the same track on two sources at once pays twice. | M5 (spatial); D3's last field |
| **Animation V1 is clips, crossfades, 1D blend spaces, entity layers, markers, opt-in root motion and skins.** No state machine, no timeline, no 2D blend spaces, no per-bone masks (a layer masks by the entity it sits on), no IK or retargeting, no morph targets. | Enough for the reference game's character; the state machine and the sequencer are the next items on the same core (blueprint §9). | after M3 |
| **Physics V1 is bodies, a character and queries.** One collider per entity, no joints, mesh colliders static only, collider outlines (`Physics::debug_lines`) not drawn by any host yet; the character does not push dynamic bodies or ride moving platforms (M3-14 decided the relative look delta: consumed by one tick as a rate, `docs/input.md` rule 5; and no camera smoothing — the rig snaps). | Enough for the reference game's player, crates and hazards; ragdolls, vehicles, hinged doors and platform riding are post-1.0. | after M3 |
| **Input has no Android or web path and no rebinding UI.** `FynkelInputPlugin` is Apple-only (GameController); the touch overlay is fixed to bottom corners; rebinds persist in the settings file (M3-12) but nothing edits them on screen. | Fine for the Apple targets M3 ships; a settings screen needs a runtime rebind API. | after M3 |
| **Native components' Inspector hints live in an editor table, not in Rust attributes.** ADR-0018 asks for `#[fynkel(...)]` field attributes on the component structs; M4-04 shipped the behaviour half (`@Expose` hints through the generator) and a table in `apps/editor/lib/src/inspector/native_hints.dart` for the 23 built-ins (audio included), read underneath the schema's `x-fynkel`. | Editing a native component's hint means editing the editor, not the component; one code path either way. | M3 stream, when `fynkel_schema_derive` is next touched |
| **A Flutter frame allocates ≈ 12 KB on the Dart heap before Fynkel runs** (profile build, a running `Ticker`; a `CircularProgressIndicator` +7 KB/frame, a `Text` rebuilt every frame +16). The 40–60 KB/frame the first captures read was attributed on 2026-09-22 (docs/instrumentation.md § What a Flutter frame costs): 39 KB of the game's 57 were the SDK's read path — `find` re-decoding the entity table and `get<T>()` decoding every component each tick, the label layer repainting every frame — and were fixed (the game reads 17.6 KB/frame idle at the spawn, behaviours ≈ 2 KB; the m3 capture's 12 s window 26.3 where it read 68.8; the host's 35–45 is the framework plus its own two progress indicators, Fynkel's share 2–3 KB). | The framework's floor is not Fynkel's to move; what a game adds is its HUD. On a phone the scavenges (1–2 a second now) are still where a p99 stutter would come from. | recorded; re-read with every capture |
| **Everything runs on the Flutter UI thread** (step, extract, render). | Fine at current sizes; ADR-0007 allows a render thread later, so extraction must stay cheap. | M5 |
| **Single machine, single profile.** Every number is from one M3 Max and one iPhone; no second display / DPI change exercised. | Regressions elsewhere would not be seen. | first release |

## 4. Compromises we knowingly made

Not bugs — things done the simple way with a note to revisit.

- `material`, `shader`, `meta`, `project` and `lock` schemas are
  **hand-written**; every component and behaviour schema is generated.
  Acceptable while these models are only validated; when renderer V1 needs a
  Rust `Material`, that becomes the source of truth (D6).
- Schema-vs-semantic diagnostic de-duplication works by **matching JSON
  pointers** (`scene.rs`, `material.rs`, `runtime.rs`). Simple; a message can
  vanish if one side changes the pointer it anchors to. The fixture corpora
  catch that.
- Document numbers are `f64` all the way; the runtime is `f32`. Narrowing
  happens in the generated codecs without a diagnostic for precision loss.
- `Document::new` embeds a minimal body per kind instead of reading defaults
  from the schema.
- Dart tests run through Flutter's `dart` only (`hooks`/`code_assets` are
  still experimental on stable Dart). Pinned; fallback documented in the M1
  gate report.
- The glTF importer writes zero tangents when the file has none (the cook
  generates them — `MESH` info `tangents: generated`) and drops
  non-default samplers and `TEXCOORD_1` bindings with a warning (the PBR
  model has one sampler and one UV set).
- The cache has no garbage collection beyond `fynkel cook --prune`; stale
  objects accumulate until then, and `rm -rf .fynkel/cache` is always safe.
  Cooked textures are RGBA8 with one mip level for every profile (D8).
- The renderer's normals are transformed by the model matrix and
  renormalised (no inverse transpose), so non-uniform scale skews them;
  emissive is relative to the exposure's white rather than in nits; the
  sky is a texture reference on `fynkel.Environment`, not the blueprint's
  `fynkel.sky@1` material (`docs/renderer.md` § Deviations). A scene that
  authors no lighting at all still gets the host's fallback sun (the M1
  demo scene). A `.shader` material's texture is read in its image's
  colour space (the sidecar's `colorSpace`); the property's `srgb`/`linear`
  is not a view switch, so a normal map must be marked `linear` on its
  image (`docs/shaders.md` § Textures).
- The runtime store keeps every retained payload decoded in memory
  (`ResidencyStats.bytes`); eviction happens only when the last `Residency`
  of a scene drops. No streaming, no budget — fine at fixture sizes.
- `fynkel test --headless`, `state dump`, `render` and `run` cook the whole
  project before loading (a cache hit is ≈12 ms); `--no-cook` trusts the
  cache. A stale cache is `FYN-R0002`, never silently re-read.
- Migrations: `material/1-2/pbr-fields`, `scene/1-2/camera-exposure` and
  `prefab/1-2/camera-exposure` are the real steps so far, each with its
  fixture pair; adding a required field to a component always costs one.
- `FYNKEL_STATUS_LOAD_FAILED` (8) and the Apple viewport ABI v2 were added
  without a deprecation window (v3 and v4 were additive); both ABIs are
  pre-release.
- ADR-0005 (license) is still open; nothing is public yet. Deferred with
  the rest of the paperwork to a "public readiness (b)" item when the
  repository goes public (blueprint §19.2d item 17, thin on 2026-09-22).

## 5. Verification (what "green" means)

Run from a clean clone — the same steps as `.github/workflows/ci.yml`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p fynkel_cli -- codegen --check
cd packages/fynkel && dart pub get && dart analyze --fatal-infos && dart test   # Flutter's dart
cd packages/fynkel_authoring && dart pub get && dart analyze --fatal-infos && dart test   # M4-01: the hook builds the runtime with --features authoring
cd apps/editor && flutter pub get && flutter analyze --fatal-infos && FYNKEL_CLI=$PWD/../../target/debug/fynkel flutter test   # M4-02…08 and 9a–9e: the editor shell, tabs, Hierarchy, Inspector, Project browser, Scene view, Prefab mode, the look, the title bar, one Inspector, one picker and the panel copies (`-j 4` while another stream compiles); project_test.dart, documents_session_test.dart, inspector_session_test.dart, project_browser_session_test.dart, scene_session_test.dart and prefab_session_test.dart drive the real session (scene_session renders through the real Metal viewport); M4-08's play_process_test.dart drives a real process
cargo test -p fynkel_authoring --test preview && cargo test -p fynkel_native --test authoring   # M4-06: the edit-preview world (sync == fresh load) and authoring ABI 3
cargo test -p fynkel_authoring --test prefab_mode && cargo test -p fynkel_document --test authoring   # M4-07: the instance query, apply as two transactions, unpackPrefab, imported hierarchies read-only, broken prefabs left alone
cargo check --target wasm32-unknown-unknown -p fynkel_core -p fynkel_scene -p fynkel_physics -p fynkel_render   # blueprint §12.1: a compile check, not a product
cargo build -p fynkel_native -p fynkel_platform_apple --no-default-features && ! nm -gU target/debug/libfynkel_platform_apple.dylib | grep -q fynkel_authoring_   # a game's runtime has no editor code
cd packages/fynkel_flutter && flutter pub get && flutter analyze --fatal-infos && flutter test
cd hosts/flutter_apple && flutter pub get && flutter analyze --fatal-infos
cd packages/fynkel_generator && dart pub get && dart analyze --fatal-infos && dart test
cd packages/fynkel_generator/example/spinner_game && dart pub get && dart analyze --fatal-infos && dart test
cargo run -p fynkel_cli -- codegen --check --project packages/fynkel_generator/example/spinner_game
cargo run -p fynkel_cli -- create /tmp/starter_game && cd /tmp/starter_game
fynkel validate && fynkel test --headless && fynkel test --headless --input test/walk.input.json && fynkel codegen --check --project . && flutter analyze && flutter test
cd examples/showcase && fynkel validate && fynkel test --headless && fynkel render --out /tmp/showcase.png && fynkel codegen --check --project . && flutter analyze --fatal-infos && flutter test
```

(`cargo test -p fynkel_cli` runs the `codegen --check` lines itself when a
Dart SDK is on `PATH`, and a `--no-flutter` create through validate and the
headless host always.) Timings: `python3 benchmarks/m0/capture.py` (frame
loop, bridge, allocation, leaks), `python3 benchmarks/m2/capture.py`
(tools) and `caffeinate -du python3 benchmarks/m3/capture.py` (the stress
scene, its many-lights variant and the reference game with the item
6/8/10/19c/16 and E1 gates; **run it alone and with the display awake**,
after `cargo build --release --workspace` and `flutter build macos
--profile` in `hosts/flutter_apple` and `examples/reference_game`, or
without `--skip-build`); results land under
`benchmarks/*/results/<date>-<commit>.md`.

Last clean-clone run: **2026-09-19, commit `68e9a72` + the M2-18 changes**
(M2 gate): fmt clean, clippy clean, 189 Rust tests passed (2 doc-tests
ignored by design), both `codegen --check` pipelines up to date, `dart
analyze --fatal-infos` clean, 58 Dart tests passed (49 `fynkel`, 5
`fynkel_generator`, 4 `spinner_game`), starter project `flutter analyze`
clean and `flutter test` 1 passed; ≈5 min wall clock (Flutter 3.47.2 / Dart
3.13.2, Rust 1.93.0, M3 Max). M3-01 added 7 `fynkel_flutter` tests (65 Dart
tests total) and the host/package analyze steps; the starter was built and
run on macOS and the host on the iOS simulator by hand. `platform/apple`'s
`presenter_wait_yields_lag_zero` is load-sensitive — it fails while another
build or a simulator app is using the GPU and passes on a quiet machine.
M3-02 (2026-09-19): fmt/clippy clean, `cargo test --workspace` green,
both `codegen --check` pipelines up to date (fingerprint
`0x45bf9f2f1cc8d124`), 66 Dart tests (50 `fynkel`, 5 `fynkel_generator`,
4 `spinner_game`, 7 `fynkel_flutter`), starter create → validate →
headless → cook → render → codegen → analyze → test clean; the robot's
cooked `MATL` chunk binds all five texture slots.
M3-03 (2026-09-20): fmt/clippy clean, 198 Rust tests green, both
`codegen --check` pipelines up to date, 66 Dart tests (the SDK, spinner and
starter suites now cook into a temporary cache first), starter create →
validate → headless (3 assets retained) → render (1 mesh) → codegen →
analyze → test clean; the render gate deletes the fixture's source tree
after cooking and renders identical pixels from the cache.
M3-04 (2026-09-20): fmt/clippy clean, `cargo test --workspace` green (the
fixture project now has 5 entities and 7 assets), both `codegen --check`
pipelines up to date (fingerprint `0x408b089693b6c6b6`), Dart suites
green, starter create → validate → headless → render → codegen → analyze
→ test clean; 11 shading pixel tests (exposure, point/spot/budget,
ambient, orthographic, maps, mask, double-sided, emissive, sky) and the
fixture render with its normal + metallic-roughness maps.
M3-08 (2026-09-20): fmt/clippy clean, `cargo test --workspace` green
(260 tests: `fynkel_physics` unit tests, `fynkel_core/tests/physics.rs`,
the native ABI `physics_bodies_commands_queries_and_contacts` test —
components, body commands, rejections on both lanes, one collision enter
with impulse, raycast through the wire, a malformed query buffer — and
the headless fixture run: Crate B lands on its Floor and sleeps under the
checksum), both `codegen --check` pipelines up to date (fingerprint
`0x08abdf27cdf70ef0` — two components, a command, four records; wire
fixtures rolled, both locks refreshed), Dart suites green (`packages/
fynkel` 79 tests with `physics_test.dart`: fall/contacts/sleep/
`PHYSICS_OWNED`/teleport, every body command and its rejections, a sensor's
one enter and one exit, raycast/shape cast/overlap/batched queries, the
fixture's gravity/layers/masks, a pending object's `StateError`;
`fynkel_flutter` 15, `spinner_game` 4 — its fixture now has 7 entities),
starter create → validate → headless (`2 bodies, 3 colliders, 1 enter`) →
`--input` → codegen → analyze → test (4 tests: the crate lands and the
cube pushes it) → render clean, showcase validate → headless → render →
codegen → analyze → test clean; `benchmarks/m3/capture.py`
(`results/2026-09-20-4b74301`): all seven gate checks green — 500 bodies
over 5 501 colliders, p50 tick 0.68 ms falling / 0.61 ms settled (max
5.8 ms, the first tick that builds the colliders), 1 430 enters / 657
exits, checksum `fdce91f4c8a52bfc` repeated, standalone 122 frames/s with
p99 interval 9.13 ms and 0 skipped, Flutter 120 ticks/s. The standalone
p99 check is load-sensitive: the first capture, run right after the
release build, saw 304 skipped frames and a 59 ms p99, the second a
10.8 ms p99 with 0 skipped; interleaved runs with and without physics
show the same occasional 14–16 ms frame, so the physics tick (≈0.4 ms
over the no-physics 0.28 ms) is not what misses the vsync.
M3-08b (2026-09-20): fmt/clippy clean, `cargo test --workspace` green
(266 tests: `fynkel_core` `instantiate_places_a_description_under_an_anchor`
and `temp_references_build_an_object_in_the_tick_of_its_spawn`, the
native `prefabs_instantiate_through_the_abi` and `QUERY_ENTITIES` in
`hierarchy_queries`, `fynkel_assets` `prefabs_describe_from_their_chunk`,
`fynkel_document` `nested_prefabs_flatten_and_cycles_are_refused`
with the new `cycle_a`/`cycle_b` fixtures, the cook's 4×3 swatch as a
3-level chain whose sRGB and linear copies differ only past the base),
both `codegen --check` pipelines up to date (fingerprint
`0x2b369001633c9953` — one record; wire fixtures rolled, the prefab
schema now admits `prefab` on entities, cache version 4, locks
refreshed), Dart suites green (`packages/fynkel` 84 tests with
`prefab_test.dart`, `findTests`, the same-tick body test; `fynkel_flutter`
15, `spinner_game` 4), starter create → validate → headless → `--input`
→ codegen → analyze → test (4) clean, showcase validate → headless →
render → codegen → analyze → test clean. Found on the way: the SDK only
sent transform subscriptions when the frame had events, so a handle
attached in a quiet frame had no snapshot until something happened
(`_flushSubscriptions` now runs on every drain).
M3-12 (2026-09-20): fmt/clippy clean, `cargo test --workspace` green
(`fynkel_core/tests/scenes.rs` 5 tests — replace destroys runtime spawns
and orders Destroyed/SceneUnloaded/SceneReady, additive refuses a loaded
id and a broken scene loads nothing, unload takes the still-alive
entities only, a zone fires enter/exit and `once` removes itself, an
empty event is `INVALID_PAYLOAD`; the native ABI
`scenes_stack_unload_replace_and_zones_fire_through_the_abi`: `R0010`
before a project, additive Playground to 12 live, `R0011` on the repeat,
an unknown flag, a coin instantiated at the origin firing one
`ZONE_ENTERED` from the Player, unload Main → 7 Destroyed +
`SceneUnloaded`, `bind_actions` jump → J kept across a replace to
Results; the `physics_rules` corpus `H0009`; the fixture project's 8
documents / 12 assets through document, import, assets and CLI tests),
both `codegen --check` pipelines up to date (fingerprint
`0x13943882aa51a5a0` — one component, one record, one event record, the
settings schema; wire fixtures rolled, both locks refreshed, header +
ffigen; the Dart `==` parameter is `that` when a field is named `other`),
Dart suites green (`packages/fynkel` 107 tests with `scenes_test.dart`:
the gate, additive/unload/replace/event order/transition failure/cancel,
the settings document and store; `fynkel_flutter` 15, `spinner_game` 4),
starter create → validate → headless → `--input` → codegen → analyze →
test clean (the starter's `main.dart` now opens `SettingsStore` and
persists Q's quality), showcase validate → headless → render → codegen →
analyze → test clean.
M3-10 (2026-09-20): fmt/clippy clean, `cargo test --workspace` green
(`fynkel_core/tests/catalogue.rs` 8 tests, the `physics_rules` corpus
`H0001`–`H0008`, the native ABI `catalogue_components_and_the_systems_query`,
the headless Catalogue scene: 10 entities, 1 prefab, 32 live after 300
ticks, the beacon at 7.5 rad, the puff and its core gone, 13 phases,
checksum repeated), both `codegen --check` pipelines up to date
(fingerprint `0xe2310e060567e753` — six components, one enum, one record,
one query; wire fixtures rolled, both locks refreshed, header + ffigen),
Dart suites green (`packages/fynkel` 100 tests with `catalogue_test.dart`;
`fynkel_flutter` 15, `spinner_game` 4), the M3 capture with all nine
checks green (the first run overlapped the test suites and hitched —
386 skipped standalone frames, p99 16.5 ms — a rerun on an idle machine
gave p99 9.03 ms; the capture is load-sensitive, run it alone), the
`--app` build launched from its bundle with 300 `Wobble` behaviours (the
starter template's `main.dart` was missing the `SystemKind`/`SystemTime`
import — found by that build, fixed), starter create → validate →
headless → `--input` → codegen → analyze → test clean, showcase validate →
headless → render → codegen → analyze → test clean.
M3-09 (2026-09-20): fmt/clippy clean, `cargo test --workspace` green
(`fynkel_physics` 10 unit tests — a character walks/jumps/lands, climbs a
step, slides down a steep ramp, rests without oscillating —,
`fynkel_core/tests/physics.rs` with the driver in the camera's frame, the
rig's pull-in and the value/missing-component checks, the native ABI
`character_commands_and_query`, the `physics_rules` scene corpus
`H0001`–`H0007`, the headless Playground replay: 120 frames, 1 jump, 75
held ticks, the Player at y 0.82 on the Platform, camera at pivot + dir·5,
checksum repeated), both `codegen --check` pipelines up to date
(fingerprint `0x28dfe79b866c1efa` — two components, two body ops, one
record, one query; wire fixtures rolled, both locks refreshed, header +
ffigen), Dart suites green (`packages/fynkel` 90 tests with
`character_test.dart`: the recorded walk on the Playground, `move`/`jump`/
`characterState()` and their rejections, the `PlayerController` example;
`fynkel_flutter` 15, `spinner_game` 4), starter create → validate →
headless → `--input` → codegen → analyze → test clean, showcase validate →
headless → render → codegen → analyze → test clean.
M3-07a (2026-09-20): fmt/clippy clean, `cargo test --workspace` green
(the native ABI `component_commands_and_query` test: set/replace/query/
remove through the wire, the presentation lane, the rejections at the
tick), both `codegen --check` pipelines up to date (fingerprint
`0x27ff561a8b4bc084` — two commands and a record; wire fixtures rolled,
both locks refreshed), Dart suites green (`packages/fynkel`
`components_test.dart`: typed reads with one query per object per tick,
a behaviour hiding its mesh and adding a light across ticks, remove/
rename/rejection fallback, spawn-then-add, the presentation lane, the
codec table), starter create → validate → headless → `--input` → codegen
→ analyze → test (its new component test) clean, showcase suite clean; a
scratch starter whose `Mover.start` spawned a twin cube from Dart built
for macOS and launched from its bundle: `meshes 2 drawn in 1 draws`, the
twin blinking every second through `visible` (HUD `1 drawn`/`2 drawn`).
M3-07 (2026-09-20): fmt/clippy clean, `cargo test --workspace` green
(core input tests, the native ABI capture/view test, the headless replay
of the fixture's `test/walk.input.json`, `FYN-P0009` document test), both
`codegen --check` pipelines up to date (fingerprint `0xe75eb93baf7a72bc`
— two new `fynkel.rec` records), Dart suites green (`packages/fynkel`
`input_test.dart`: bind on load, the catch-up-tick log, replay, recorder,
`TestInput`; `packages/fynkel_flutter` `input_test.dart`: keys, mouse,
recorder, channel batches, a game view with the overlay driving a real
world, the overlay hidden on macOS), starter create → validate → headless
→ headless `--input test/walk.input.json` (70 frames, 11 events: move
held 11 ticks, jump pressed 2) → codegen → analyze → test (the cube ends at
`-5·2/60` in z and `+4·2/60` in x, 2 jumps, landed) clean, showcase
validate → headless → codegen → analyze → test clean; the starter built
for macOS (the Swift input plugin compiles) and launched from its bundle,
built for the iOS simulator and launched: the touch overlay's stick walked
the cube and its button jumped it (HUD `mover (-1.03, 0.41, -3.30)  2
jumps (airborne)`).
M3-06 (2026-09-20): fmt/clippy clean, `cargo test --workspace` green
(19 shading pixel tests; `presenter_wait_yields_lag_zero` in
`platform/apple` flaked once under the full parallel run and passed 3/3
alone — a GPU timing test), both `codegen --check` pipelines up to date,
Dart suites green (the controller test asserts the appended draw/culled
counts), starter create → validate → headless → render (1 instance in 1
draw, 0 culled) → codegen → analyze → test clean, showcase validate →
headless → render (27 instances in 16 draws, 0 culled, 18 casters in 8
shadow draws; `renders/m3-06.png` byte-identical to `m3-05.png`) →
codegen → analyze → test clean; stress capture
`benchmarks/m3/results/2026-09-20-cf2b3eb` all four gate checks PASS
(5 343 instances in 15 draws ≤ 15 pairs, 458 culled, standalone 122 fps
with prepare + encode + submit 1.41 / 1.59 / 1.78 ms, Flutter 120 ticks/s,
`render()` 2.6 ms, GPU 3.2 ms); M0 capture `2026-09-20-cf2b3eb` within
noise (122/120 fps, engine CPU 0.12 / 0.20 / 0.26 ms; GPU p50 1.05 ms vs
0.74 with the p95 unchanged at 1.08 and Flutter's own raster thread up by
the same 0.3 ms in both of two runs — the GPU's clock state, the cube
scene's draws are the same).
M3-06a (2026-09-20): fmt/clippy clean, `cargo test --workspace` green,
both `codegen --check` pipelines up to date, Dart suites green (9
`fynkel_flutter` tests: `bundled_cache_test.dart` unpacks a fake
`AssetBundle` and prunes stale objects), starter create (`fynkel_cache/
README.md` keep file) → validate → headless (staged 4 objects) → render →
codegen → analyze → test clean, showcase validate → headless → codegen →
analyze → test clean and the showcase app launched on macOS from the
bundled cache alone (`FYNKEL_PROJECT=/nonexistent`: 22 retained from
`$TMPDIR/fynkel_cache`, 27 drawn in 16 draws); confirmed by the user on
the iPhone. Follow-up (same day): `FynkelFrameStats.frames/fps` and an
`fps` HUD line (showcase + starter), `generate.py --app` wraps the stress
scene in the starter app with an orbiting camera — built and launched on
macOS from its bundle (≈4 700 drawn in 15 draws, 120 fps, `render()` 2.7 ms).
M3-05a (2026-09-20): `examples/showcase` validate → headless (34 entities,
22 assets) → render (`medium`: 27 meshes, 5 blended, 3 lights, 18 casters
in a 2048² map; `low` 960×540 no shadow; `high` 1920×1080, 4096² map) →
codegen → analyze → test clean; the app launched on macOS with the orbit
running and the HUD's frame counts live; fynkel_flutter's controller test
checks the appended stats.
M3-05 (2026-09-20): fmt/clippy clean, `cargo test --workspace` green
(18 shading pixel tests, `PIPELINE_VARIANTS` 9), both `codegen --check`
pipelines up to date, Dart suites green (the viewport controller test
switches to `low` and back against the real viewport), starter create →
validate → headless → render (`medium`: MSAA 4, 1 caster in a 1024² map)
→ render `--quality low` (MSAA 1, no shadow, 960×540) → codegen → analyze
→ test clean; M0 capture `2026-09-20-12859e1` — standalone 122 fps,
Flutter Path A 120 fps, engine CPU 0.11 / 0.19 / 0.29 ms, GPU 0.74 /
1.08 / 2.21 ms, present lag 0 — within noise of `2026-09-19-f958917`
with the cube scene now drawn at MSAA 4 behind a shadow pass.

M3-13 (2026-09-20): fmt/clippy clean, `cargo test --workspace` green
(`fynkel_core/tests/projection.rs` 5 tests — the centre pixel and one
metre up at depth 5 match the extract's matrices by hand, the priority
camera wins and a scaled camera does not zoom, an interpolated pose
halfway through a frame, a destroyed entity is `MISSING`, occlusion by
mask with the anchor's own collider excluded, 66 anchors → 64 + 2
dropped with one `FYN-R0012` cleared by the next step; the native ABI
`projection_of_world_anchors`: no camera, centre/offset/occluded/missing
through the wire, 70 anchors with the diagnostic visible before the next
step, a truncated and a wrong-kind buffer are `Protocol` with an empty
view and the world stays usable), both `codegen --check` pipelines up to
date (fingerprint `0x5b5e4210411a1a74` — two records; wire fixtures
rolled, both locks refreshed, header + ffigen), Dart suites green
(`packages/fynkel` 117 tests with `projection_test.dart` — the fixture
crate's pixel against the camera by hand, moves one tick behind like the
render, off the viewport and behind, the viewport scale, the floor
occluding by mask only, pending/destroyed/bogus handles and `prune`, 65
anchors → one informational diagnostic per episode, no camera and no
anchors → no call — and `model_test.dart`; `fynkel_flutter` 23 with the
gate `projection_test.dart` — the label at the crate's point across a
scripted camera pan with zero rebuilds, unpainted and unhittable once
culled or behind, a clamped marker on the edge, children diffing keeps
anchors, `FynkelGameView` updates the view every frame — and
`model_binding_test.dart`; `spinner_game` 4), starter create → validate
→ headless → `--input` → codegen → analyze → test (5, with 'the mover
writes the HUD model; a label follows the cube') clean, showcase
validate → headless → render → codegen → analyze → test clean. Found on
the way: a `GameObject` attached this tick read identity before its
first snapshot, so the starter's `transform.rotation = …` at the first
tick sank the Cube from its authored y 0.5 to 0 — the SDK now reads the
transform through the component query until the snapshot arrives, and
the Mover writes nothing while idle.

M3-14 (2026-09-21): fmt/clippy clean, `cargo test --workspace` green
(`fynkel_core` input tests rewritten for the one-tick rate semantics of
relative controls plus `a_binding_scale_turns_pixels_into_units` —
`*<factor>` on a delta, a trigger and a stick, `BadScale` for zero,
negative, non-numeric and non-axis bindings), both `codegen --check`
pipelines up to date (fingerprint `0x97fd26db36af3421` — `frame_x`/
`frame_y` on `fynkel.rec.InputAction`; wire fixtures rolled, the four
locks refreshed; no ABI header change), Dart suites green
(`packages/fynkel` 118 with the mouse-delta test reading the rate in a
tick and the motion in `update` across a 50 ms frame and a zero-tick
frame; `fynkel_flutter` 23; `spinner_game` 4), starter create →
validate → headless → `--input` → codegen → analyze → test clean,
showcase validate → headless → render → codegen → analyze → test clean,
reference game generate (deterministic: the three generators rewrite
byte-identical files) → validate → headless → `--input … --steps 1400`
(1 398 ticks, 10 trigger enters, checksum stable) → render → codegen →
`flutter analyze --fatal-infos` → `flutter test` (4) clean. Built and
played on macOS through `fynkel run` (keyboard first — the mouse did
nothing: `World::step` consumed the tick's input right after the
character driver, so the camera rigs, placed late in the tick, read a
zeroed `mouse:delta` while a held stick still turned them; `end_tick`
now closes the tick after every native system, and the core rig test
turns by a mouse delta once across a two-tick frame). Tuned on the way:
the blade's `Oscillate`
phase (−1.217 rad) and a 1.9 s wait at coin 6 so the route crosses the
corridor and the ball lane clear; gold coins read as gold with a touch
of emissive since the scene has no sky to reflect.

M3-19 (2026-09-21): fmt/clippy clean, `cargo test --workspace` green
(320 tests: `fynkel_core` animation — sampling, the loop's `rem_euclid`,
the one-shot's clamp and its single `AnimationEnded` at tick 122,
crossfades on two clocks, the same-clip and zero-second fades as plain
plays, `SetTime` rejections, targets by name under the animator, the
skin palette from interpolated matrices; `fynkel_import` glTF v4 with
skins and animations, FYNI v3, cache v4 and the fixture's Rig scene;
`fynkel_render` 20 pixel tests over fourteen pipelines with the skinned
column; the native ABI's animation commands, query and event; the
headless report's `animation:` line), both `codegen --check` pipelines
and the reference game's up to date (fingerprint `0x18acc498ae005854` —
`fynkel.Animator` 21, `fynkel.Skin` 22, `fynkel.cmd.Animate` 9,
`fynkel.ev.AnimationEnded` 9, `fynkel.rec.AnimatorState`,
`FYNKEL_QUERY_ANIMATOR` 11; wire fixtures rolled, the locks refreshed,
the header + ffigen rerun), Dart suites green (`packages/fynkel` 126
with `animation_test.dart` — the Rig scene's joints and Prop at 0.5 s,
the end event once and replayed at 4×, pause/resume/negative
speed/seek/crossfade/stop from a behaviour, a spawned host driving a
child by name, a missing animator/negative fade/NaN speed as
diagnostics, an unknown clip as `CLIP_UNAVAILABLE` once, a pending
object's play in one tick; `fynkel_flutter` 23; `fynkel_generator` 5;
`spinner_game` 4), `hosts/flutter_apple` analyze clean, starter create
(the template's registry lists the two components) → validate → headless
(checksum `8408fa8489f76bff`) → `--input` (72 ticks, `f47d67405a604f2d`)
→ codegen → analyze → test (5) clean, showcase validate → headless (47
entities, `cc6543ae0b202b81`) → render (`--steps 10`: 28 instances in 17
draws, 1 skinned over 11 joints) → codegen → analyze → test clean,
reference game generate (`character.py` deterministic: the GLB's md5
stable across reruns) → validate (20 documents, 304 references) →
headless (`animation: 2 animators + skins, 1 clips, 1 skins, 1 skinned
meshes (11 joints)`, checksum `da05e9536bcad71c`) → `--input …
--steps 1400` (`aa675a1b9fc65ee1`) → render (`--steps 40`) → codegen →
`flutter analyze --fatal-infos` → `flutter test` (5, with 'the character
is posed from the controller') clean — walk, run and jump are never
named by the scene, so their clips reach the world through
`register_animation_assets` on demand from the cache (the load report
says `animation.clips == 1`, the test plays all four). Found on the way:
a stale `.fynkel/cache` under the document fixture made
`cook_project_expands_scenes_and_records_the_project` see `cached`
where it expects `cooked` — the fixture's cache is never committed and
is removed before every gate.

M3-19a (2026-09-21): fmt/clippy clean, `cargo test --workspace` green
(`fynkel_document` 61 — `imported_hierarchies_instantiate_like_prefabs`:
the bare crate reports `FYN-D0401` for `rig.glb#prefab`, the provider
expands the Rig scene to 15 entities with the `Imported` twin's Prop
material overridden, a wrong sub and a texture's `#prefab` are
diagnostics, a rig nested inside the crate prefab expands clean;
`fynkel_import` 40 — `khronos_sample_rigs_import_clean` over Fox,
CesiumMan and RiggedFigure; `fynkel_assets` 5 — `rig.glb` described
from its `prefab` chunk, the swatch texture refused with `FYN-R0006`;
`apps/cli` 17 — `import --prefab` writes the robot's 9-entity prefab
once and refuses to overwrite it, a texture has no hierarchy;
`khronos_rigs_instance_from_the_source_alone` cooks each rig into a
scene that names only the GLB, runs 60 headless ticks with `animation
(2, 1, 1, 1, joints, 0)` and finds `b_Neck_04` /
`Skeleton_arm_joint_L__3_` / `arm_joint_L_3` moved in the state dump;
the render and headless suites count the doubled rig), both `codegen
--check` pipelines up to date (no wire change), `packages/fynkel` 127
(`animation_test.dart`: the rig instanced by `AssetRef(rig, sub:
'prefab')` reports the source's id and path, six entities, the head's
world position, then waves to the same joint numbers as the authored
column; `mesh/0` is an `ArgumentError`, a texture a
`FynkelInstantiateError`), `fynkel_flutter` 23, `fynkel_generator` 5,
`spinner_game` 4, `hosts/flutter_apple` analyze clean, starter create →
validate → headless → `--input` → codegen → analyze → test (5) clean,
showcase validate → headless (48 entities) → render (`--steps 10`,
pixel-identical to the hand-authored Runner at ticks 1 and 40) →
codegen → analyze → test clean, reference game regenerated byte-for-byte
→ validate → headless → `--input … --steps 1400` → render → codegen →
analyze → test (5) clean; locally the Mixamo X Bot through
`playground.py --hero` — 237 entities, 130 joints, checksum
`cf5865855fd56211`, 0 diagnostics, the five Dart tests green — and the
Khronos rigs rendered side by side. The outside content is the point:
from 19a on an item's acceptance runs on files its implementer did not
make.

M3-19b part 1 (2026-09-21): fmt/clippy clean, `cargo test --workspace`
green (`fynkel_import` 51 — `tests/fbx.rs`: `rig.fbx` against `rig.glb`
(16 vertices with the same normals and weights per position, the three
joints' parents, bind matrices and rest poses — the root's in the
armature's space —, `bob` 2.0 s / `wave` 1.0 s with the spine at 45° Z
at 0.5 s, the head at 90° Y, the hips lifted 0.25, the prefab's six
entities with `Skin` on the Body; deterministic and round-tripping),
`robot.fbx` against `robot.glb` (three parts, 24 cube corners with the
same UVs after the V flip, the quad's colours, Glass/DefaultMaterial/
Painted with the embedded `paint` texture decoded to the same pixels,
nine prefab entities, the clamped-wrap warning), external textures
refused then read with `allowExternalFiles` and listed as cache inputs,
a GLB as `.fbx` is `A0100`, `max_joints`/`max_vertices`/`max_node_depth`
are `A0101`; `tests/cook.rs`: `rig.fbx` in the corpus cooks to
`META MESH MESH SKIN ANIM ANIM JSON` and hits the cache; the glTF
suite's `robot.expected.fyni` unchanged through the `graph.rs`
refactor), every project lock refreshed (`importers` gains
`fynkel.fbx: 1`), `fynkel_platform_apple` built for `aarch64-apple-ios`
with `ufbx`'s C in the workspace, both `codegen --check` pipelines
(no wire change), `packages/fynkel` 127, `fynkel_flutter` 23,
`fynkel_generator` 5, `spinner_game` 4, `hosts/flutter_apple` analyze
clean, starter and the two examples through their pipelines; locally
the four Mixamo files through a throwaway `fynkel create` project:
`validate` 0/0, `test --headless --steps 120` (74 entities, 3 animators,
2 skins over 129 joints, 0 diagnostics), `render` upright, skinned and
posed for each.

M3-19b part 2 (2026-09-21): fmt/clippy clean, `cargo test --workspace`
green (`fynkel_document` `tests/project.rs` 15 — the fixture's Column
rig: sub-assets, resolve by sub and by sub uuid, `D0500`/`D0501`/
`D0502` with their pointers, the Rig scene's `Imported` entity through
the rig's adopted prefab; `fynkel_assets` `tests/store.rs` — `prefab`
and `clip` read off the rig object, closure `[rig, skeleton]`, the
source's numbered clip refused on the rig; `fynkel_import`
`tests/cook.rs` — the rig cooks to `JSON prefab ANIM ANIM` with the
FBX `bob` moving the Prop, a clip file's sidecar edit recooks the file
and the rig only, a rig edit the rig only, the robot skeleton refuses
the column's wave with `A0106` naming `head`; `apps/cli` 18 — the
Khronos Fox through `import --rig`, dry run keeping the id, `rig
inspect` text/JSON, `rig validate`, a scene on `<rig>#prefab` +
`anim/run` cooked, headless (24 joints) and `b_Neck_04` moving, then
`RiggedFigure` + `CesiumMan` written with `A0106` at `/clips/cesiumman`
and refused by `rig validate` and `cook`), every project lock refreshed
(`documents` gains `rig: 1`; no wire change, both `codegen --check`
pipelines), `packages/fynkel` 128 (`animation_test.dart`: the Column
rig instantiated from Dart by `{ rig, prefab }` — the source's entities
and skin — and played by `anim/wave`/`anim/bob`), `fynkel_flutter` 23,
`fynkel_generator` 5, `spinner_game` 4, `hosts/flutter_apple` analyze
clean, starter and the two examples through their pipelines (the
reference game's `game.Player` gains `idleAnim` … `jumpAnim`, the
scene's clip names, so its schema, registry and scene changed);
locally the four Mixamo files as `hero.rig.json` with one command (65
joints, 67 entities, 156/156 channels per clip, `rig validate` ok),
`playground.py --hero assets/models/hero/hero.rig.json` → `validate`
0/0, the route replayed (233 entities, 2 skins over 129 joints,
checksum `cdd39bcc7d0a7b9a`, 0 diagnostics), the five Dart tests
green, the Playground rendered with the X Bot idle.

M3-19c part 1 (2026-09-21): fmt/clippy clean, `cargo test --workspace`
green (343 tests: `fynkel_core` `tests/animation.rs` 10 — a child
animator replacing and lerping its parent's pose by weight, an additive
wave over a walk, markers across a loop and backwards, root motion
moving the transform and, with a `CharacterController`, feeding the
tick's `Move` (velocity z ≈ −2, the yaw reported); `fynkel_cooked` the
`ANIM` marker section round trip and its three refusals;
`fynkel_document` `tests/migrate.rs` `scene|prefab/2-3/animator-layers`
with `v2_dancer.scene.json` migrated exactly, `tests/project.rs` the
rig's `markers` validated (orphaned, malformed, non-array, a clean
list); `fynkel_native` `tests/abi.rs` a marked clip's two markers over
a looping second, `weight`/`parameter` read back, `SetWeight 2.0`
refused; `apps/cli` the Khronos Fox's `run` with rig-document markers
through `rig inspect` (3), cook (the late one dropped, `FYN-D0502`) and
headless (2 `ANIMATION_MARKER`)), both `codegen --check` pipelines up
to date (fingerprint `0x68267f95a40bac58`: `fynkel.Animator`
`weight`/`additive`/`root_motion`, `fynkel.cmd.Animate` 64 B with
`value`/`name` and the ops `setWeight`/`setParameter`,
`fynkel.rec.AnimatorState` 80 B, `fynkel.ev.AnimationMarker` 10; glTF
importer 5, FYNI 4, cache 5, scene/prefab formatVersion 3; wire
fixtures rolled, every lock refreshed, the header + ffigen rerun, the
committed intermediates regenerated), Dart suites green
(`packages/fynkel` 130 with `animation_test.dart`'s marker test — the
Column rig's `wave` markers `peak`/`trough` reaching `onAnimationMarker`
across a loop and on the way back at −1×, `weight`/`setParameter` read
back and rejected out of range, root motion on a vertical-only clip
taking nothing out —, `fynkel_flutter` 23, `fynkel_generator` 5,
`spinner_game` 4, `hosts/flutter_apple` analyze clean), a fresh
starter through validate, headless, the walk replay, `codegen --check`,
analyze and its 5 tests, the showcase (48 entities, checksum
`8c186fe3e17251df`) and the reference game (route replayed, checksum
`aa675a1b9fc65ee1`, 0 markers yet — the hero's footsteps are part 2)
through validate, headless, render, analyze and tests. Outside content:
the Khronos Fox's markers (CLI test); the fixture Column's are mine —
the user's Mixamo hero gets its footsteps in part 2.

M3-19c part 2 (2026-09-21): fmt/clippy clean, `cargo test --workspace`
green (348 tests: `fynkel_core` a blend space's two neighbours mixed
and phase-synced, the clock rescaled on a parameter change, the
dominant clip's markers, root motion lerped; `fynkel_document`
`blend.rs` — the order rule, the clip rule, the malformed entry —, the
fixture `column.blend.json` in the corpus (11 documents, 16 assets);
`fynkel_import` the blend cooked as a document chunk with its clips'
carriers as dependencies; `fynkel_assets` a space registered once with
its clips; `apps/cli` the Khronos Fox's `fox.blend.json` (Survey 0 /
Walk 1 / Run 2 by `speed`) through `validate`, `cook`, `test
--headless` (the space counted once) and, in process, `SetParameter`
1.5 posing all 23 `b_*` joints at the lerp/slerp midpoint of Walk and
Run at the same phase), both `codegen --check` pipelines up to date (no
wire change: fingerprint `0x68267f95a40bac58`; `blend: 1` in every
lock's `documents`), Dart suites green (`packages/fynkel` 131 with
`animation_test.dart`'s blend test — duration 2 → 1.5 → 1 along the
parameter with the phase kept, the hips halfway, the Prop following the
one clip that keys it, an out-of-range parameter reported raw with the
duration clamped, the dominant clip's markers —, `fynkel_flutter` 23,
`fynkel_generator` 5, `spinner_game` 4, `hosts/flutter_apple` analyze
clean), a fresh starter through validate, headless, the walk replay,
`codegen --check`, analyze and its 5 tests, the showcase (48 entities,
checksum `8c186fe3e17251df`) and the reference game regenerated (168
entities, `player.blend.json`, the `wave` action; route replayed,
checksum `aa675a1b9fc65ee1`; the render unchanged at 40 ticks) through
validate, headless, render, analyze and its 5 tests (the box character
and, locally, the Mixamo hero through `--hero`: footsteps counted, the
wave layer on `Rig`). The M3 capture (`benchmarks/m3/results/
2026-09-21-570c9c1`, the stress scene at 8 406 entities with 200 rigs):
450 animators + skins in 0.34 ms a tick, 10 975 markers crossed over
1 800 ticks, p50 tick 3.89 ms falling / 3.54 ms settled (the physics
step is 2.1 ms of it and predates 19c — the item 8 gate reads 3.2–3.9
ms run to run, close to its 4 ms line), the standalone host at 122
frames/s with 183 skinned instances in view — eleven gate lines green.
Outside content: the Fox's blend (CLI test), the user's hero on the
reference game's blend space (local); the Column's and the box
character's spaces are mine.

M3-11 part 1 (2026-09-21): fmt/clippy clean, `cargo test --workspace`
green (381 tests: `fynkel_audio` the engine — register/replace/unregister,
play/steal/refuse by priority, pause/resume/volume/pitch/loop, the clock's
ends, bus × master × listener × mute gains, device open/close on the null
build —, the mixer — resampling, smoothing, a full ring — and
`tests/allocation.rs` counting zero allocations across a render;
`fynkel_core/tests/audio.rs` eight tests — a 1 s clip starts at its set
tick and ends at tick 59 with `PLAYBACK_ENDED`, loop/pause/resume/
`SetVolume`/`SetPitch`/`Stop`, `MISSING_COMPONENT` and four
`INVALID_PAYLOAD`s, `PlaySound` on both lanes and `AudioBus`, an unknown
clip once per source and on every `PlaySound`, the lowest listener wins
and a destroyed one falls back to 1, 32 voices then a priority-5 thief
stealing voices[0] and a −1 newcomer refused, a clip change restarting;
the native ABI `audio_commands_events_and_queries` — components 23/24
through `QUERY_COMPONENTS`, `QUERY_AUDIO`/`QUERY_AUDIO_STATS`,
`PLAYBACK_ENDED` within 60 steps, the presentation batch rejected 5 of
5 with `WRONG_LANE` last, `fynkel_world_audio_open` opening the Mac's
device (48 kHz, 2 channels) and suspend/close leaving the voices; the
headless fixture — `audio: 1 sources, 1 sounds, 1 voices started (0
stolen, 0 refused), 1 ended, 0 diagnostics` under the same checksum; the
fixture counts everywhere (17 assets, 6 sources, 31 references, 23
components, 8 retained) and the two tests the M3-19c rebase had left
red — the synthetic 10 000-entity scene at formatVersion 3, the native
authoring session's asset count —, both `codegen --check` pipelines up
to date (fingerprint `0xb223170a5c7bdaec`, fixtures rolled, four locks
refreshed), Dart suites green (`packages/fynkel` 146 with
`audio_test.dart` — the fixture's tone registered and played to its
end with a behaviour replaying it once, object control and
component-driven play, `world.audio.play` on both lanes, the settings
store's volumes reaching the buses, every rejection code, the ghost
source once, 32 voices stolen and refused through the SDK, suspend/close
as no-ops —, `fynkel_flutter` 24 with the view opening, suspending and
closing the device on the lifecycle, `fynkel_generator` 5, `spinner_game`
4, `hosts/flutter_apple` analyze clean), the iOS `fynkel_platform_apple`
release build with `cpal` linked, a fresh starter through validate,
headless (`audio: 1 sources, 1 sounds` — the Dart Mover plays them, so
headlessly nothing starts), the walk replay, `codegen --check`, analyze
and its 6 tests (the new one: 4 voices started and ended, `land.ogg`
registered by its first one-shot, 2 `PlaybackEnded` on the cube), the
showcase and the reference game unchanged (checksums `8c186fe3e17251df`,
`aa675a1b9fc65ee1`; both registries list the two components). Outside
content: the starter's two clips are Kenney's (CC0); the acceptance on
the reference game is part 2's.

M3-11 part 2 (2026-09-21): fmt/clippy clean, `cargo test --workspace`
green (388 tests: `fynkel_audio::stream` — a mono ramp started at frame
600 of 1 000 plays 400 frames and ends, a 500-frame loop wraps to frame 0
without a gap across 1 200 reads and a stop ends the decoder, a bad
source fails and ends —, `tests/engine.rs` — a streamed quarter-second
stereo ramp ended by the clock at 30 ticks with `starved == 0`, looped
past four lengths with the state's `time` wrapped, stopped to silence,
its decoder respawned across a device close, `streams` 1 → 0 on
unregister —, `tests/allocation.rs` with a streamed voice among the 32
(121 callbacks, zero allocations, one tap returned), `fynkel_import`
`a_streamed_sound_keeps_its_source_instead_of_samples` (the same format
and frames as the plain import, no samples, the file's bytes, the
summary's `streamed`, the `.fyni` v5 round trip, a truncated WAV still
`IMPORT_INVALID_DATA`, `"stream": "yes"` rejected at `/settings/stream`)
and `a_streamed_sound_cooks_to_a_strm_chunk` (`STRM` where `CLIP` was,
`decode_stream` → 8 000 Hz × 2 × 400, the bytes the source's; fixtures
re-rolled), `StreamPayload` round trip and short-blob rejection, the
document crate's importer table at `fynkel.audio: 2`), both `codegen
--check` pipelines up to date (fingerprint `0xab5d834fb20093b6` —
`AudioStats` + `streams`, `starved` —, fixtures rolled, the four locks
refreshed with the importer at 2), Dart suites green (`packages/fynkel`
146, `fynkel_flutter` 24, `fynkel_generator` 5, `spinner_game` 4,
`hosts/flutter_apple` analyze clean), a fresh starter through validate,
headless (`audio: 1 sources, 1 sounds (0 streamed)`), the walk replay,
`codegen --check`, analyze and its 6 tests, the showcase unchanged
(checksum `8c186fe3e17251df`), the reference game with its sound —
validate clean (23 documents, 12 sources, 325 references), headless
`audio: 1 sources, 1 sounds (1 streamed), 1 voices started (0 stolen, 0
refused), 0 ended, 0 diagnostics` at checksum `57b05435ef2870ee` (600
ticks) and `8ec23e8df14397a2` (the route, 1 398 ticks; the checksums
moved with the `Music` entity), the render, analyze and its 6 tests (the
new one, 'the sounds': `report.audio.sounds 1, streams 1`, the music's
state playing at the track's 74.25 s duration, one one-shot per marker
over 100 walked frames, the pickup at coin 1, `started == 1 + footsteps
+ 1`, `starved == 0`, the hit on a fall with the music still playing).
On the Mac's output device: the standalone host with `--audio` on the
reference game's cache streamed the music for 12 s — `1 voices (1
started), 1 clips (1 streamed), 1131 callbacks, 0 underruns, 0 starved`
— and for 80 s, through the track's loop at 74.25 s (`7497 callbacks,
0 underruns, 0 starved`, the one voice still playing).
Outside content: the reference game's five clips are Kenney's and
Juhani Junkala's (CC0, credited in its README); the acceptance of item
11 is on them.

M3-15 (2026-09-21): fmt/clippy clean, `cargo test --workspace` green
(390 tests, no engine change in this item — one hung `rustdoc` doctest
process had to be killed and the suite rerun; a cargo jobserver quirk,
not a test), the four `codegen --check`s up to date (the reference
game's `game.Player` schema regenerated for its new doc), Dart suites
green (`packages/fynkel` 146, `fynkel_flutter` 24, `fynkel_generator` 6,
`spinner_game` 4, `hosts/flutter_apple` analyze clean), a fresh starter
through validate, headless, the walk replay, `codegen --check`, analyze
and its 6 tests, the showcase unchanged (checksum `8c186fe3e17251df`),
the reference game regenerated byte-for-byte (25 documents, 14 sources,
382 references): the Menu as the startup scene headless `29 entities,
audio: 1 sources, 1 sounds (1 streamed), 1 voices started` at checksum
`28009f79997fa559` (600 ticks), the Results `13 entities, 2 sources, 2
sounds (1 streamed), 2 voices started, 1 ended` at `e88fd46c69989688`
(200 ticks; the jingle ends, the music loops), the Playground route by
`--scene … --input … --steps 1400` unchanged at `8ec23e8df14397a2`, the
render, analyze and its 8 tests (the two new ones: the flow — Menu →
`play()` → the route → the Results after 89–92 ticks with `lastRun`
10/10 and no deaths, the `Jingle` source playing once, the click on
the presentation lane, Play again → 10 collectibles, Give up →
`won false`, Menu → 'Ring coin 8' — and the settings — the defaults,
`bindingsOf('jump')` = `keyboard:Space`/`gamepad:south`/
`touch:overlay/jump`, `musicVolume` 0.3 / quality `high` / jump
rebound to J written to a temp `settings.json` and `lastReport`'s
`rebound`, Space no longer jumps and J does, a reset, a relaunch reading
the file). Played by hand: the macOS bundle end to end (title →
Settings → Rebind waiting/Cancel and Esc → the music slider → quality
`high` in `~/Library/Application Support/Fynkel Reference Game/
settings.json` → Play → Esc pause menu → Settings → Give up → Results
"Run over 0/10" → Play again → Quit to menu; one `missedDeadline` on a
scene transition, nothing else in the log) and the iPhone 16 Pro Max
simulator (the title without Quit, the overlay's stick collecting two
coins, the on-screen pause button → Give up → Results 2/10; the HUD
under `SafeArea`, the keyboard hint hidden). The real-iPhone
qualification stays M6. Outside content: `win.ogg` and `click.ogg` are
Kenney's (CC0, credited).

M3-16 part 1 (2026-09-21): fmt/clippy clean, `cargo test --workspace`
green (the renderer's `pass_timestamps_and_the_memory_estimate` on the
M3 Max: shadow ≈85 µs, scene ≈65, post ≈37, frame span ≈0.7 ms; the
native `memory_stats_and_the_snapshot_budget`; the leak test now reads
the crate's own counting allocator — a second `#[global_allocator]` in a
test binary is a link error), the four `codegen --check`s up to date,
Dart suites green (`packages/fynkel` 149 with `instrumentation_test`,
`fynkel_flutter` 24 with the ABI 3 fields asserted, `fynkel_generator`
6, `spinner_game` 4, `hosts/flutter_apple` analyze clean), a fresh
starter through validate, headless (the new `timing:`/`memory:` lines),
the walk replay, `codegen --check`, analyze and its tests, the showcase
and the reference game unchanged in checksum with their HUD `work`/`gpu`/
`memory` lines; the standalone host on the fixture at 3024×1964: `sim ms
p50=0.07 | prepare ms p50=0.29 | gpu ms p50=0.42 p95=3.9 n=121 untimed=5 |
mem MB native=1.4 resident=130 gpu=201` (the 4× MSAA attachments and
the 4096² shadow map are most of the estimate).

M3-16 part 2 (2026-09-22): fmt/clippy clean, `cargo test --workspace`
green, the four `codegen --check`s up to date, Dart suites green
(`packages/fynkel` 149, `fynkel_flutter` 25 — `heap_probe_test` reads
≈ 41.4 MB back for 41.3 MB allocated under `flutter test
--enable-vmservice`, `summary()` asserted in the game view test —,
`fynkel_generator` 6, `spinner_game` 4, `hosts/flutter_apple` analyze
clean), a fresh starter through every step, the showcase and the
reference game unchanged in checksum; `benchmarks/m0/capture.py`'s
`alloc_aot` on the host's profile bundle: 43 KB/frame, heap 10 MB, 19
scavenges + 2 old-space phases in 6 s (4 ms); **`benchmarks/m3/
capture.py` → `results/2026-09-22-0427213`**, fifteen checks green on
the third run (the earlier two: one 11.4 ms standalone window, the
stale phase count): stress total work 7.59 ms p95 (simulate 3.50,
render() 4.08: extract 2.13, prepare 1.74, encode 0.02), 120 frames/s,
GPU frame 2.75 ms p95 standalone; reference game total work 1.62 ms
p95 (behaviours 0.19, native 0.32, render() 1.09), the route's step
p95 64 µs / extract 42 / frame 106, native 6 MB; Dart heap 39 KB/frame
(host) and 56 KB/frame (game), 3–4 scavenges a second. The tail varied
between runs (physics p95/max 3.9/8.8 → 13.3/48.8 ms once; the Flutter
host's GPU spans with Flutter's raster beside them) — recorded in
`docs/instrumentation.md`, medians stable.
The allocation gap (2026-09-22, after 17): the full gate again on the
Dart-only change (`packages/fynkel` 150 tests, `fynkel_flutter` 25,
the starter, showcase and reference game unchanged in checksum), then
both captures alone: `benchmarks/m0/results/2026-09-22-871c353` (the
JIT bench 2.7 KB/frame with the new `find` + `get` per tick; the host's
AOT 44.6 KB/frame, unchanged by design) and **`benchmarks/m3/results/
2026-09-22-871c353`, sixteen checks green on the first run**: stress
total work 8.06 ms p95, reference game 1.52 ms p95, the game's Dart
heap **26.3 KB/frame and 25 collections in 12 s** where `0427213` read
68.8 and 61 on the same window (17.6 KB/frame idle in the attribution
harness; docs/instrumentation.md § What a Flutter frame costs).
M3-17 thin (2026-09-22): **clean-clone run at `0668ad0`** — `git clone`
into a temporary directory, `cargo install --path apps/cli` (1 min
release build), then `examples/reference_game` through its README's
commands only with `fynkel` on `PATH` and no `FYNKEL_CLI`: `validate`
(23 documents, 0 errors), `test --headless` (the Menu, checksum
`28009f79997fa559`), the recorded route (1398 ticks, 182 entities,
checksum `338f8239bd95b101`, 0 diagnostics), `render` (179 entities, 53
instances in 13 draws), `flutter test` (8 passed), `cook`, `flutter build
macos --debug`, the bundle launched with `FYNKEL_CAPTURE=1` — ten
`[stats]` lines at 121 frames/s, total work 1.49 ms p95. The one hidden
assumption: the game's README linked a top-level `README.md` that did
not exist (added, minimal). The fresh starter (create → validate →
headless with the recording → codegen → analyze → test) green with the
stats section, `profileDartHeap` and the capture line in place.
E1, image-based lighting (2026-09-22, three commits): the full gate on
each — fmt/clippy clean, `cargo test --workspace` green (83 targets; the
document crate's mirrored cache version bumped with the lock files),
both `codegen --check` pipelines and all three projects up to date
(fingerprint unchanged at `0xab5d834fb20093b6` — E1 adds no wire), the
Dart suites (`packages/fynkel` 150, `fynkel_flutter` 25, the generator,
`spinner_game`), a fresh starter create → validate → headless →
`--input` → codegen → analyze → test, the showcase and the reference
game validate → headless (checksums `8c186fe3e17251df` and
`28009f79997fa559`, the route's `8ec23e8df14397a2` unchanged) → render →
codegen → analyze → test. The showcase's sky is its own HDR file now
(`tools/sky.py` → `assets/textures/sky.hdr`, regenerated byte-identical
in CI) and `renders/e1.png` is the picture: the metals reflect the
environment and the shadows take the sky's blue. Both captures alone
after a release build: **`benchmarks/m3/results/2026-09-22-954db83`,
sixteen checks green** (stress total work 7.76 ms p95 at 120 frames/s,
standalone 122; the reference game 0.65 ms p95; the stress scene's GPU
frame 2.34 / 4.94 ms against 3.06 / 6.65 at `0427213` and 3.56 / 8.01 at
`871c353` — the mesh shader now samples the two cubes and the LUT
whether or not a scene has an environment, and that did not show above
the run-to-run spread) and `benchmarks/m0/results/2026-09-22-954db83` (standalone 122
frames/s, Flutter Path A 120, engine CPU 0.30 ms p95, leak tests
passed). One standalone window in an earlier run of the same build held
a 15.6 ms frame (the gate's p99 check is the max of per-second p99s);
three bare runs of the host showed the same hitch in one run of three,
before and outside the sample — recorded here, not chased.
E1, light culling (2026-09-22, two commits): the renderer change with the
full gate (fmt/clippy clean, `cargo test --workspace` green over 84
targets, four `codegen --check` pipelines and all three projects up to
date, fingerprint unchanged at `0xab5d834fb20093b6` — the light list
rides an existing instance attribute and a storage buffer, so no wire or
ABI change —, `packages/fynkel` 150 and `fynkel_flutter` 25, the showcase
and reference suites, a fresh starter create → headless
`8408fa8489f76bff` → test, the reference game's checksums
`28009f79997fa559` and `8ec23e8df14397a2` unchanged), and the showcase
and reference-game renders **byte-identical to their committed PNGs** —
the scenes whose lights all reach everything look exactly as they did.
Then the measurement, since "prepare stays cheap" is the claim that
needed numbers: `generate.py --lights` builds the same stress field under
256 point and spot lights and `capture.py` runs it next to the baseline
(`benchmarks/m3/results/2026-09-22-64221f7`, **eighteen checks green**,
the binaries `64221f7`'s — part 2 touches only the generator, the
capture and these docs): 64 of the 256 reach the view and fill the pool
(189 dropped), at most 8 reach any one object (738 of 5 530 instances
clipped), 121 frames/s against 122, and prepare 2.00 / 2.10 ms against
1.56 / 1.64 — **+0.45 ms for 5 571 instances against a full pool**, the
ceiling for the loop, since the pool is capped and more authored lights
only grow the once-a-frame frustum pass. The GPU frame grew more than
the CPU did (3.73 / 4.24 ms against 2.93 / 2.94): eight lights a pixel
cost more than choosing them. Two runs were thrown away first, and why is
worth writing down: an unattended run throttles — the hosts present to a
real surface, so an idle display left the stress scene at 84.5 frames/s
with 248 skipped and the Flutter host's timers zeroed, and the lights run
produced no windows at all (the gate now fails that case instead of
skipping the check, and the capture's usage says `caffeinate -du`); the
run after it was green everywhere but one window's p99 interval of
10.39 ms (the same one-hitch-in-three the paragraph above records), and
the run after that was clean.

R1, cascaded shadow maps (2026-09-22): the full gate (fmt/clippy clean,
`cargo test --workspace` green over 84 targets — `platform/apple`'s
load-sensitive `presenter_wait_yields_lag_zero` failed once while a
release build shared the GPU and passed alone, as this section already
records —, fingerprint unchanged at `0xab5d834fb20093b6`: cascades are
renderer-internal, no document, wire or ABI change) plus the capture,
because the shadow pass now runs four times a frame:
`benchmarks/m3/results/2026-09-22-5abaac3`, **eighteen checks green**,
122 frames/s standalone with a p99 interval of 8.66 ms. The shadow pass
did **not** get four times dearer — 0.31 / 0.55 ms against 0.40 / 0.40 —
because each cascade is a quarter of the map, so the rasterised area is
what it always was and only the vertex work multiplied. The scene pass
did pay: 1.73 / 3.08 ms against 1.85 / 2.42, ≈ +0.7 ms at p95 for the
per-pixel cascade selection and the blend band; the GPU frame is
3.56 ms p95 against 2.82. That is the price of the picture and it is
inside the 120 Hz budget with room.

One measurement changed the shader: the first cut put the cascade's
depth into the blend band, and the two captures it produced were both
16.7 ms at p99 (two frames). A cascade's ortho box reaches `3r + 10 m`
back toward the sun, so *every* receiver sits near its far plane —
the band was the whole screen and every pixel paid two PCF sets. The
band is the sides only now; the run after that was clean. The showcase
render `examples/showcase/renders/r1.png` is the before/after against
`e1.png`: the same scene, shadows with edges instead of smears, and the
runner's shadow touching its feet.

R2, ambient occlusion (2026-09-22): the full gate — fmt clean, clippy
clean, `cargo test --workspace` green over 84 targets (the render crate's
24 pixel tests include the new
`ambient_occlusion_darkens_what_a_wall_closes_in_and_leaves_the_open_floor`),
both `codegen --check` pipelines up to date and the fingerprint unchanged
at `0xab5d834fb20093b6` (R2 touches no wire record; what it does bump is
the separate Apple viewport ABI, 4 → 5, for `gpuAo` in the stats struct),
150 `fynkel` + 25 `fynkel_flutter` + 9 `fynkel_authoring` Dart tests, 8
reference-game and 219 editor tests, the starter created → validated →
headless → replayed → analysed → tested, the showcase the same, and the
runtime still free of editor symbols. One thing was caught by the gate
rather than by review: the Apple ABI constant had been bumped in Rust
only, so `viewportAbiVersion` in `package:fynkel_flutter` (and the
header's `#define`) still said 4 — the package's own
`abi version matches the native library` test is exactly the tripwire for
that, and it fired.

Then the capture, because three passes joined the chain:
`benchmarks/m3/results/2026-09-22-94490ad`, **eighteen checks green**,
122 frames/s standalone and 120 in both Flutter apps, unchanged. What the
GPU timers say is more interesting than "it still fits". On the stress
scene through the Flutter host — 5 530 instances, the one frame where the
GPU is actually saturated — the whole GPU frame got **cheaper**: 3.07 ms
p95 against R1's 3.56. The AO stage costs 1.81 ms p95 there, and the
scene pass fell 3.08 → 2.80 and the shadow pass 0.55 → 0.20, because the
depth prepass hands the scene pass a complete depth buffer and early-Z
throws away the overdraw it used to shade. At stress scale the prepass
pays for the occlusion and a little more. The reference game is the other
end: a 53-instance frame where the GPU idles between passes, so three
more pass boundaries show up as span, and its GPU frame went 4.01 → 6.11
ms p95 with an AO span of 4.85 — the spans overlap (the doc's own
caveat), so that is not 4.85 ms of added work, but the direction is real
and it is the cost of a full-screen effect in a frame with almost nothing
in it. Both are inside the 8.33 ms budget at 120 Hz with the frame rate
untouched. The lever, if a future scene needs it back, is a half-
resolution AO target — the estimate is low-frequency and already blurred.

The showcase render `examples/showcase/renders/r2.png` is the
before/after against `r1.png`: the same scene, 88 680 pixels changed and
**every one of them darker** — the crates, the pillars and the lamp post
grow a contact shadow where they meet the floor instead of floating on
it, while the sunlit faces are untouched, which is the point of putting
the term on the ambient and IBL contribution alone.

R3a, volumes (2026-09-22): the full gate from a clean `target/` — fmt
clean, clippy clean, `cargo test --workspace` green over 85 targets (the
render crate's 25 pixel tests include the new
`volumes_shape_the_ambient_occlusion_where_the_camera_stands`; new
`fynkel_document` `tests/volume.rs` and the cook test
`a_volume_profile_cooks_as_its_document_and_a_scene_depends_on_it`),
both `codegen --check` pipelines up to date and the fingerprint bumped
`0xab5d834fb20093b6` → `0xc2e5042968b9bde5` for `fynkel.Volume` (the
wire fixtures regenerated under the new directory; the Apple viewport
ABI stays 5), 151 `fynkel` + 25 `fynkel_flutter` + 9 `fynkel_authoring`
+ 6 generator + 4 `spinner_game` Dart tests, 8 reference-game, 1
showcase and 219 editor tests, the starter created → validated →
headless → replayed → analysed → tested, the showcase the same (49
entities now — the Global Volume — which its own test counts and the
gate caught), and the runtime still free of editor symbols. The render
with the showcase's volume disabled is `r2.png` to the bit.

Two things the gate turned up that were not R3a. Every GPU test had
started spending ~20 s *before its first line*: Metal's first device
query makes CoreFoundation treat a command-line binary's directory as its
bundle and list it for `.lproj` folders, and `target/debug/deps` had
grown to 680 000 files (94 GB) — `presenter_wait_yields_lag_zero` failed
its bound under that, a fresh worktree of R2 passed in 0.8 s, and a
`sample` of the test showed it inside `_CFBundleCopyLProjDirectoriesForURL`.
`cargo clean` fixed it; when the GPU tests turn slow, look at the size of
`target/` first. And the editor's two 30 s-timeout failures were the
workspace build running beside them; alone, 219 of 219.

**The capture is pending.** Two runs were taken and neither is a
measurement: two unrelated processes held a full core each for over ten
days and another project was compiling, so the first run lost the
standalone p99 (10.08 ms) and the Flutter stress host's windows, and the
second lost the standalone `[1s]` windows altogether; neither result was
kept. Nothing in R3a changes a frame without a volume (the stress scene
and the reference game have none — `resolve_volumes` returns the
defaults before it looks at anything), so it is taken together with
R3b's on a quiet machine.

R3b, bloom (2026-09-23): fmt clean, clippy clean, `cargo test
--workspace` green over 85 targets (the render crate's 26 pixel tests
include the new
`bloom_spreads_bright_light_past_its_edges_and_the_threshold_keeps_the_rest`,
`PIPELINE_VARIANTS` 23; the volume document and `fynkel_core::volume`
tests read and blend the `bloom` block), both `codegen --check`
pipelines up to date with the fingerprint unchanged at
`0xc2e5042968b9bde5` (no component or wire record changed; the Apple
viewport ABI stays 5 — the stage is timed inside the post pair), 151
`fynkel` + 25 `fynkel_flutter` + 9 `fynkel_authoring` + 6 generator + 4
`spinner_game` Dart tests, 8 reference-game, 1 showcase and 219 editor
tests, the starter created → validated → headless → replayed →
analysed → tested, the showcase the same, and the runtime still free of
editor symbols. The showcase with its bloom intensity at 0 renders
`r3a.png` to the bit.

One thing the gate turned up: `viewport_lifetimes_do_not_grow_the_process`
failed at 4–7 MiB against its 4 MiB bound, on a quiet machine and with
the bloom chain not even allocated, while the same test at `ae03864`
passed every run. It was not a leak — 240 lifetimes grew no more than
60 — but the three new pipelines per renderer move the process's plateau
past the test's ten warm-up lifetimes, so the baseline was taken on the
slope. The warm-up is thirty now; the bound is unchanged.

The capture for R3a and R3b together follows as its own commit.

## 6. Verdict and next steps

**M4 is closed with a GO** (`m4-gate-report.md`, 2026-09-25): a level is
made in the editor instead of code, every edit goes through the shared
authoring commands and the authoring log, Play runs the project's own
game, and the user's own session passed. Carry-overs F1–F7 are in the
report's §5.

**The order from here is `docs/roadmap.md`** (agreed 2026-09-23 after a
full foundation review): Phases 1 and 2 are done; next the extensibility
and data-model ADRs (Phase 3), rendering, M5, M6. This section stays the
log of what is done.

M3 is closed with a GO (`m3-gate-report.md`, 2026-09-22) carrying two
named exceptions that are decisions, not failures: the repository stays
private (item 17 done thin — license, `LICENSE`, `CONTRIBUTING.md`, CI on
a fork and the C ABI bump wait for a reader, "public readiness (b)"), and
the reference game ran on the iPhone simulator, not a device (M6
qualifies it). Everything else the §16 gate asked for is in: a clean
clone builds and runs the game through its README (run at `6176d5e`),
it is playable end to end on macOS, no editor is needed, the frame paths
are instrumented on every surface, and both scenes are captured against
the manifest with sixteen checks green
(`benchmarks/m3/results/2026-09-22-871c353`). The carry-over list is
E1–E9 in the report's §5; §3 above is its living copy.

**E1 is done, both halves** (2026-09-22, five commits): image-based
lighting — the `.hdr` import and the cooked `ENVM` products, the
split-sum in `mesh.wgsl`, the showcase's own HDR sky (`docs/renderer.md`
§ Image-based lighting) — and light culling: a 64-light frame pool the
camera frustum filters, the eight that reach an object shading it, and
the many-lights stress variant that measures the assignment (§ Lights per
object). **Next on this stream: making the rendering
good** — the renderer's own leftovers, one item each: R1 (cascaded
shadow maps), R2 (ambient occlusion), R3a (volumes: the look as
scene data, `docs/renderer.md` § Volumes) and R3b (bloom, the profile's
second block, § Bloom) are done, and so is R3c (colour grading, the
third block, § Colour grading; 2026-09-25) and R3d (contact shadows,
the fourth block, § Contact shadows; 2026-09-26); then the package architecture (which also decides the deferred
game-defined data asset, a `ScriptableObject`-like `@DataAsset`) and the
shader assets the user asked for. M4 closed on 2026-09-25 (item 30
below).

**Shader assets** (ADR-0031) are done as of 2026-09-26: one `.shader`
file — a header and a WGSL `surface()` the engine lights — cooked,
drawn with its own pipelines, changed from Dart (`createShaderMaterial`,
`setMaterialValue`), made and edited in the editor (Create ▸ Shader, the
shader's page, the material's Shader menu), with three examples in the
showcase (dissolve, water, rim light) and `docs/shaders.md`. The user's
check in the app is pending.

**The Animator Controller** (ADR-0032, Phase 5) is done as of
2026-09-26: a `.controller.json` of parameters, states and transitions
per layer, run by the engine every tick (`fynkel_animation::controller`);
`setFloat`/`setBool`/`setTrigger`, `stateOf` and the state events from
Dart; the editor's Animator window (the graph, parameters, layers, the
Inspector for states and transitions, Create Animator Controller on a
model, the live view in Play). The reference game's Player and the
showcase's Runner run on one; `docs/animator.md` is the guide. The
user's check in the editor is pending.

**Tags and layers** (ADR-0020, 2026-09-23) come in while the joint R3a +
R3b capture waits for a quiet machine: several tags and one engine-wide
layer per object, as the optional components `fynkel.Tags` and
`fynkel.Layer`; physics membership from the object's layer (the
colliders' own `layer` bits migrated away), camera and point/spot light
culling masks, the Dart API by name (`hasTag`, `findWithTag`, `layer`),
and the editor's object header and Tags and Layers settings. The steps
are L1 (the format and physics membership), L2 (the Dart API), L3
(renderer masks), L4 (editor) and L5 (the reference game), each its own
commit.

**L1 is done** (2026-09-23): the project's top-level `layers` and `tags`
(project `formatVersion` 2, `project/1-2/layers`; `FYN-P0014`),
`fynkel.Layer` (26) and `fynkel.Tags` (27) with `FYN-H0012` for an index
the project does not declare, scene and prefab `formatVersion` 4
(`scene|prefab/3-4/object-layers`: a collider's `layer` bits become its
entity's `fynkel.Layer`, lowest bit wins; cameras and point/spot lights
gain `culling_mask`, not yet read by the renderer — L3), Rapier's
membership from the object's layer, regrouped in the tick the layer
changes; the load report's `layers`/`tags` (Dart `LoadReport.layersAndTags`,
`world.layersAndTags`, `world.layerMask` reading them; `Layer`/`Tags`
exported); the editor's layer-mask field reads the new list and hints
`culling_mask`. Fingerprint `0x17859a02e41b113a`, cache version 7
(imported prefabs are version 4). Every document in the repository
migrated with `fynkel migrate` (the `fixtures/migrate` inputs stay old on
purpose); the starter template and `benchmarks/stress/generate.py` write
the new format.

**L2 is done** (2026-09-23): the Dart API speaks names.
`GameObject.layer` (get and set), `tags`, `hasTag`, `addTag` and
`removeTag` go through `fynkel.Layer`/`fynkel.Tags` with read-your-writes
like any component. `World.findWithTag`/`findAllWithTag` read the per-tick
entity table: `EntityInfoRecord` now carries `layer` and `tags`, so a
lookup costs no extra query. An undeclared name throws, listing the
project's names. `test/layers_test.dart` covers it, including an object
moved to another layer at run time leaving one raycast group for the
other. Fingerprint `0xa5c456a220cc42d8`. The cache version is unchanged
(no cooked format moved).

**L4 is done** (2026-09-23): the editor. The Inspector's object header
shows the layer as a dropdown and the tags as chips (`f9dc499`). The
authoring service has `setLayersAndTags` (`45db70b`): renames and
additions write the project file only, while a reorder or a removal
rewrites every scene and prefab so each object keeps its layer, tags and
masks (prefab overrides included). It is refused while such a document
has unsaved edits, and it is undone with the asset history. The **Tags
and Layers** dialog (`Edit Layers…` / `Edit Tags…` in the header's menus)
edits both lists (add, rename, reorder, remove) and applies them as that
one operation. The open documents it rewrote are re-read.

**L3 is done** (2026-09-23): the renderer reads the masks. The extract's
records carry their entity's layer bit. An object outside the scene
camera's `culling_mask` is neither drawn nor casts a shadow
(`FrameStats::layer_hidden`, `layerHidden` in `fynkel render --json`). A
point or spot light whose mask leaves the object's layer out is not
assigned to it. The editor camera draws every layer. The sun's mask stays
deferred. `shading.rs` covers all three cases in one pixel test. No wire
format, fingerprint or cache version changed.

**L5 is done** (2026-09-23): the reference game uses tags and layers, the
end-to-end proof. `fynkel.project.json` declares the tags `player` and
`pickup`, and `tools/playground.py` tags the Player and the coin prefab
(`Scene.add(tags=…)`). The coins' and the switch's zones were already on
the `trigger` layer with a player-only mask after L1. The coins and the
switch now also answer only the object `world.findWithTag('player')`
finds. `game_test.dart` finds the player and the ten coins by tag, sees
none left after the recorded route, and drops two balls into a coin. The
first, a `props` ball, is kept out by the layer mask. The second sits on
the `player` layer but has no tag, and the tag check keeps it out. With
the tag check removed, that test fails. ADR-0020 is complete. The
directional light's mask, the editor's per-layer Scene-view toggles and a
collision matrix stay deferred, as the ADR says.

The M3 backlog as it landed, for the record:

1. ~~**`package:fynkel_flutter`** (D1)~~ — done (M3-01): the viewport widget
   and Swift plugin as a package; the template uses it; a created project
   shows its spinning cube.
2. ~~**Rust material model** (D6)~~ — done (M3-02): `fynkel_core::material::PbrMaterial`
   is the one definition behind the generated material schema, the codecs,
   the importer's output, the cooked `MATL` chunk and what the renderer
   reads; materials are formatVersion 2 via the first real migration.
3. ~~**Cooked-scene loading** (D2)~~ — done (M3-03): `fynkel cook` writes
   expanded scenes, `fynkel_assets` is the runtime store with residency, the
   ABI and `World.loadProject` read the cache only; the pixel test renders
   from a cache whose sources were deleted.
4. ~~**Renderer V1 (a)**~~ — done (M3-04): physically based shading with
   every map, HDR + ACES + camera exposure, sun/ambient/sky, 8 point/spot
   lights, orthographic camera; `docs/renderer.md`.
5. ~~**Renderer V1 (b)**~~ — done (M3-05): the sun's PCF shadow,
   transparency, MSAA 1/4, render scale, quality profiles switched at
   runtime from every surface, debug lines; the M0 capture unchanged.
   5a (M3-05a): `examples/showcase`, the scene every renderer item is
   looked at through (`renders/<item>.png`).
6. ~~**Renderer V1 (c)**~~ — done (M3-06): one draw per `(mesh,
   material)` pair, frustum culling on cooked bounding spheres (camera and
   sun's box), the stress scene generator and `benchmarks/m3/capture.py`
   with the gate green at 5 000 meshes.
7. ~~**Input** (C2, D3)~~ — done (M3-07): actions from the project file,
   the §5.1 held/edge rules in the core, keyboard/mouse/gamepad/touch
   overlay in the Flutter host, recordings replayed headlessly and in
   `flutter test`; `docs/input.md`.
   7a (M3-07a): typed native component access from Dart — the one path
   `RigidBody`/`Collider` use; `docs/design/dart-sdk.md` §21.2.
8. ~~**Physics on Rapier**~~ — done (M3-08): `RigidBody`/`Collider`,
   the step at the §5.1 slot, body commands, contacts, queries, project
   gravity and layers, the stress scene's 500 bodies; `docs/physics.md`.
   8b (M3-08b): developer ergonomics — mip chains, `find`/`findChild`,
   spawn-then-configure in one tick, `World.instantiate`, nested prefabs.
9. ~~**Character controller and third-person camera**~~ — done (M3-09):
   `CharacterController` (12) over Rapier's kinematic controller,
   `ThirdPersonCamera` (13), `Move`/`Jump` body ops, `characterState()`,
   the Dart `PlayerController` example, the Playground headless gate;
   `docs/physics.md`.
10. ~~**Native component catalogue**~~ — done (M3-10): `Rotator`,
   `Oscillate`, `Follow`, `LookAtCamera`, `LifetimeDespawn`, `Emitter`
   (14–19) ticked by the core, per-system time everywhere, the stress
   scene's 5 000 movers next to 300 `Wobble` behaviours;
   `docs/catalogue.md`.
11. ~~**Runtime scenes, trigger zones, settings**~~ — done (M3-12,
   blueprint item 12; the backlog's execution order was revised after
   item 10: 12 → 13 → 14 → 19 → 11 → 15 → 16 → 17 → 18; scenes before
   polish): replace/additive/unload in the core and over the ABI,
   `SceneManager.transition` from inside a tick, `TriggerZone` (20) →
   zone events on behaviours, `SettingsStore` behind
   `GameContext.settings`; `docs/scenes.md`.
12. ~~**Flutter HUD models and projected world labels**~~ — done (M3-13,
   blueprint item 13): `GameModel`/`ModelValue` bound with
   `ModelBinding`/`ModelBuilder`, the bounded projection view over
   `fynkel_world_project`, `ProjectionLayer`/`WorldLabel`, opt-in
   occlusion by collider mask, `FYN-R0012` beyond 64 anchors; `docs/hud.md`.
13. ~~**The reference game's world and loop**~~ — done (M3-14,
   blueprint item 14): `examples/reference_game`, the Playground
   generated from `tools/`, four behaviours, the recorded route green in
   CI, the relative-look and camera-smoothing gaps decided
   (`docs/input.md` rule 5, `*<factor>` binding scales).
14. ~~**Animation**~~ — done (M3-19, blueprint item 19, pulled forward
   from M5): the one clip core, glTF skins and clips through the cache,
   `Animator` (21) / `Skin` (22), crossfades, `ANIMATION_ENDED`, the
   skinned render path (14 pipelines), `GameObject.animation` in Dart,
   the reference game's rigged character from `tools/character.py`
   posed from the controller, the showcase's runner; `docs/animation.md`.
15. **The character pipeline and animation depth** — done (the user's
   call on 2026-09-21 after a Mixamo X Bot went through M3-19 — the
   core held, the feature was shallow): ~~**19a** glTF prefab instancing~~
   — done (M3-19a: a scene's `prefab.asset` and `World.instantiate`
   accept a source's `prefab` sub-asset, `fynkel import --prefab`, the
   Khronos rigs as fixtures) —, **19b** ~~FBX import over `ufbx`~~ —
   done (M3-19b: `fynkel.fbx`, the glTF importer's twin, a Mixamo
   download imports as it is; the rig document `hero.rig.json` — one
   skeleton, clips from other files matched by bone name, `anim/<name>`,
   written by `fynkel import --rig`) —, **19c** ~~animation depth~~ —
   done (M3-19c in two commits: blend spaces as `.blend.json` documents
   played like a clip and phase-synced by `setParameter`, layers as a
   second `Animator` on a descendant entity (`weight`, `additive`),
   clip markers as `AnimationMarker` events, opt-in root motion, the
   200-rig stress gate; the reference game's Player on a blend space
   with the wave layer and footsteps in the HUD). From 19a on an item
   passes only on content its implementer did not make (Khronos sample
   rigs committed under CC-BY, a Mixamo download checked locally —
   blueprint scope rule). **11 audio done** (M3-11 part 1:
   `fynkel_audio` on `cpal` + Fynkel's own mixer, the components, the
   tick's audio phase, the commands/queries/event, the device lifecycle
   through the ABI, the Flutter view and the standalone host, the Dart
   surface, the starter's jump and landing sounds); **part 2 done**
   (M3-11 part 2: streamed music — `stream: true` in a sidecar, the
   `STRM` chunk, a decoder thread per streamed voice into a 1 s ring,
   loop by reopening, `starved`/`streams` on the wire — and the
   reference game's sound: Kenney's footsteps per 19c marker, pickup
   and hit, Juhani Junkala's chiptune streamed on the music bus; CC0,
   credited); **the reference game's menus/settings/results done**
   (M3-15, blueprint item 15: the Menu and Results scenes from
   `tools/playground.py`, `Game.play/finish/toMenu`, `GameState.screen`
   and `lastRun`, `lib/ui/` — menu, pause menu, results, the settings
   panel with rebinding through `onKeyEvent` —, `win.ogg`/`click.ogg`,
   played end to end on macOS and the iPhone simulator). **Instrumentation
   part 1 done** (M3-16 part 1: the timers of `docs/instrumentation.md`
   — the frame's split in Dart, extract/prepare/encode, Metal pass
   timestamps, the counting allocator and resident set, residency and
   the GPU memory estimate, Flutter timings, load time, the C5 budget
   diagnostic — on every surface: viewport ABI 3, `FynkelWorldStats`,
   the headless JSON, the standalone line, the hosts' HUDs). **Part 2
   done** (M3-16 part 2: the C3 `DartHeapProbe` on the VM's GC timeline,
   the AOT capture in `benchmarks/m0/capture.py`, `FynkelFrameStats.
   summary()`, `benchmarks/m3/capture.py` over both scenes with the
   12 ms p95 total-work check — `results/2026-09-22-0427213`: stress
   7.59 ms p95, reference game 1.62 ms p95, every check green). **Public
   readiness (17) done thin** (2026-09-22, the user's call — no reader
   yet): the starter template and its `AGENTS.md`/`README.md` at the M3
   SDK (the stats section, `profileDartHeap`, the `[stats]` capture
   line), a minimal top-level `README.md`, and the gate run — a clean
   clone through `cargo install --path apps/cli` and the reference
   game's README commands only, up to the built bundle printing
   `[stats]` lines; the license, `LICENSE`, `CONTRIBUTING.md`, issue
   templates, the first-time-reader README and CI on a fork are
   deferred to a "public readiness (b)" item for when the repository
   goes public (blueprint item 17). The Dart allocation gap is
   attributed and closed on Fynkel's side (2026-09-22: the game 57 →
   17.6 KB/frame, the framework's ≈ 12 KB floor documented in §3 and
   docs/instrumentation.md). **The gate report (18) done short**
   (2026-09-22, `m3-gate-report.md`: the six criteria with evidence, the
   clean-clone run at `6176d5e`, the numbers, the decided deviations —
   the ABI stays at 1 —, carry-overs E1–E9, GO).
16. **M4 — the editor — has started in parallel** (2026-09-21, the user's
   call): branch `editor` in its own worktree, the backlog in blueprint
   §19.2e (ten items: the authoring ABI D5 + D7 first, then the shell,
   Hierarchy, Inspector, Project browser, Scene view, prefab mode,
   Play/Stop over a stdout channel, profiler/IDE/`fynkel editor`, the
   gate), the resolutions in ADR-0016 and the extension model in
   ADR-0018 (Proposed). The two streams touch shared crates only through
   separate small commits merged to `main` first (`fynkel_document`,
   `fynkel_core`, `fynkel_native`, `package:fynkel`, the generator, the
   CLI — the closed list is in §19.2e). **Item 1 is done (M4-01,
   2026-09-21):** `crates/fynkel_authoring`, the `authoring` feature and
   `fynkel_authoring_*` ABI in `fynkel_native`, `packages/fynkel_authoring`
   with a Dart test driving a complete session over the ABI; D5 (the ABI
   half) and D7 closed, the two §3 rows retired; a game's runtime carries
   none of it (CI check). **Item 2 is code-complete (M4-02,
   2026-09-21; the gate is driven by the user):** `apps/editor` — the
   docking spike (`docking` 1.16.2, seven criteria, ADR-0016), launcher
   (New over `fynkel create --json`, Open, Recent, doctor checks),
   §22.2 workspace saved per user with Reset layout, native menu bar +
   command registry + palette (23 commands, each reachable three ways),
   Console over the session's diagnostics with exact positions and
   open-in-IDE, read-only banner with the lock's reason, migration
   preview over the new `migrations` query, preferences; 45 widget/unit
   tests, seven of them over the real session. **Item 3 is done (M4-03,
   2026-09-21, driven by the user):** document tabs
   (open, dirty dot, save / save-as / save-all / revert / close prompts,
   undo and redo per tab, the watcher's silent reload and the conflict
   dialog with three revisions and a diff — `FYN-E0001`, never an
   overwrite) and the Hierarchy (built once, patched from `affectedIds`,
   one transaction per operation, keep-world-transform drops, search and
   filters, badges); 86 tests, the real-session ones on a 10 000-entity
   scene and the watcher; three read-only queries added to
   `fynkel_authoring`, no shared crate touched. **Item 4 done
   (M4-04, 2026-09-21; driven by the user in two rounds on copies of the
   fixture project and the reference game):** the Inspector —
   specs from schema + metadata + an editor table for the built-ins
   (ADR-0018 accepted, with the note that the Rust `#[fynkel(...)]`
   attributes are still the M3 stream's), one widget per field kind,
   multi-object editing with mixed values and one transaction per
   gesture (a drag commits once on release), precheck + session
   validation with the message kept and the value back, Add / Remove
   Component, prefab override revert, Open script, the codegen banner
   over committed drift and `fynkel codegen --check --json`; shared
   touches: `@Expose`/`@FynkelBehaviour` hints in `package:fynkel`, their
   emission in `fynkel_generator` (example regenerated, the fixture's
   `game.Spinner` schema updated to match), `--json` on `fynkel codegen`;
   116 editor tests. **Item 5 done (M4-05, 2026-09-21; gate driven by
   the user on copies):** the Project browser (folders / flat
   list, search, kind filters, sort, previews), the Asset panel (sidecar
   settings, Apply and Reimport, sub-assets, dependents), New / rename /
   move / delete-with-refusal / Reveal / Copy id, the asset history with
   its own undo, prefab drop onto the Hierarchy; shared touches:
   `AssetDatabase::update` + `Editor::relocate` in `fynkel_document`,
   `fynkel_authoring_operate` (ABI 2) in `fynkel_native`; 140 editor
   tests. **Item 6 done (M4-06, 2026-09-22; gate driven by the user on
   the fixture project and the reference game's Playground):** the Scene
   view over the session's edit-preview
   world (edit mode, incremental diff sync with a proven fresh-load
   equality, in-process cook), the editor camera, picking and marquee
   through the world, gizmos with one transaction per drag, editor-only
   hidden/locked in `.fynkel/editor.json`, debug-line overlays, Save
   Frame pixel-identical to `fynkel render`; shared touches: authoring
   ABI 3 in `fynkel_native`, viewport ABI 4 (`debug_lines`,
   `read_frame`) in `platform/apple` + `package:fynkel_flutter`
   (`renderHandle`/`debugLines`/`readFrame`) — the latter two an
   amendment to the §19.2e closed list; 194 editor tests. **Item 7 done
   (M4-07, 2026-09-22; gate driven by the user on copies of the fixture
   project and the reference game's Playground):** prefab mode — instances open to
   their derived rows (the `instance` query, live against an open prefab
   tab), every edit on a derived entity an override on the anchor, bold
   overridden fields with per-field Revert and Apply, Apply to prefab as
   two undo steps in two documents, Unpack as one `unpackPrefab`
   transaction, prefab and imported-hierarchy tabs (`<id>#prefab`,
   read-only), broken prefabs shown and never rewritten, five `prefab.*`
   commands; shared touches: `unpackPrefab` + `Editor::apply_with_prefabs`
   in `fynkel_document`, `apply --prefabs` in `apps/cli`; 219 editor
   tests. **Item 8 done (M4-08, 2026-09-22; gate driven by the user on a
   copy of the reference game, five findings fixed the same day):** Play / Stop over `fynkel run` with
   `FYNKEL_EDITOR_CHANNEL=stdio`, `package:fynkel`'s one-directional
   stdout channel (`log` / `diagnostic` / `stats` / `state` records, and
   silence when the variable is unset — the shared touch, with
   `package:fynkel_flutter`'s stats window), the Console's Game tab with
   the Project tab's filters and a row that opens the document with its
   entity selected, the process-tree Stop, the save prompt, the refusal
   to play a project with errors and the "changes apply on the next
   Play" banner; four `play.*` commands; 242 editor tests, two of them
   against a real process (`kill -9` leaves every document
   byte-identical). **Items 9a–9d** (added 2026-09-22 at the user's
   request, before item 9 so the Profiler is built in the new look):
   **9a done** (the design tokens and theme, the Density preference, Lucide
   icons, the keyboard focus ring, notices instead of snackbars, nothing
   animated but hover; gate driven by the user), **9b–9d done 2026-09-23** (gate
   driven by the user) — one title bar that is the macOS title bar, the Scene
   view's toolbar over the picture, the Inspector read left to right, one
   Inspector for whatever was selected last (the Asset panel folded into
   it, pages registered by document kind, a lock), one picker for every
   "which one?"; editor-only, no shared crate touched; 274 editor tests.
   **9e done 2026-09-23** (the gate's one finding, trackpad zoom, fixed in `FynkelViewport`) (added 2026-09-22 at the user's
   request during the 9b–9d gate) — panel tabs with the × on hover and a
   right-click menu (maximise, close, New <Panel>), trackpad scroll and
   pinch zoom in the Scene view, an orientation gizmo in its corner, and
   copies of the Hierarchy, Scene view, Inspector and Project browser
   with their own state and padlocks, brought back after a restart;
   still no shared crate touched (the Scene view's copies set their own
   camera on the one preview world); 293 editor tests.
   **9f done 2026-09-23** (gate driven by the user on a copy of the
   reference game; its judgements applied: 1 pt lines, 64-segment
   circles, colliders for the selection only, no Y axis, an unselected
   camera faint) (ADR-0019, the user's call of
   2026-09-23 after 9e's gate: gizmos "pixelated and not clean") — the
   renderer's overlay pass after post (anti-aliased lines with a width,
   faded behind the scene, flat triangles, the analytic ground grid; the
   old `LineList` debug pipeline gone, `DebugDraw` its client), the Apple
   viewport ABI v6 (`fynkel_apple_viewport_overlay`), `ViewportOverlay` /
   `FynkelViewportController.overlay` in `package:fynkel_flutter`, and the
   Scene view's grid, outline and icons moved onto it; shared touches in
   `fynkel_render`, `platform/apple` and `package:fynkel_flutter`, each
   its own commit; 293 editor tests.
   **9g in progress**: the handle framework (shapes, screen-space hit
   test, axis/plane/ring/scale/radius constraints) and the tool registry
   (`SceneTool`/`SceneTools`; move, rotate, scale rebuilt on it, a
   combined transform tool on Y as the proof), the handles drawn through
   the overlay at 80 pt; component handles (`ComponentHandlers`: a
   point/spot light's range, a spot's outer and inner angles, box, sphere
   and capsule collider sizes, a camera's field of view or orthographic
   height) behind the Inspector card's Edit toggle, in place of the
   tool's gizmo, one transaction per drag (overrides on a derived
   component); 312 editor tests. Next: the user's 9g gate.
   **9h done** (gate driven by the user, 2026-09-23): the runtime `Gizmos` API in
   `package:fynkel` (a `GizmoBuffer` the world clears each frame, on
   through `World.showGizmos`; lines, rays, circles, spheres, boxes,
   capsules, arrows, frusta; colour and matrix scopes; a `const` no-op in
   release) drawn by `FynkelGameView(gizmos:)` through the overlay, on
   from the editor's *Show Gizmos in Play* (`FYNKEL_GIZMOS=1`); the
   declarative `@Gizmo.radius/box/point/direction/cone/lineTo/path`
   annotations, their `gizmo` hint in `x-fynkel` from the generator (with
   a new `List<Vec3>` field kind for paths; the spinner example's
   `game.Patrol` as the end-to-end check), drawn in the Scene view for
   the selection or always, and dragged behind the Edit toggle, one
   transaction per drag; native hints for the Emitter's velocity and the
   Follow and camera-rig targets; the reference game's `Switch.target` a
   line to its gate. Shared touches in `package:fynkel`,
   `package:fynkel_generator` and `package:fynkel_flutter`, each its own
   commit; 321 editor tests. **9g done** (the user's gate passed
   2026-09-23). Next: item 9 (Profiler, IDE navigation, `fynkel editor`, polish).
17. **Play in the editor (ADR-0021) — P1–P6 done 2026-09-23.** The
   project's game is compiled into its own editor (`fynkel editor <dir>`),
   Play runs it in the Game view from what the editor holds, the launcher
   hands a project over to its editor, opens are fast when nothing native
   changed, and the separate process lives on as **Build and Run**. Next:
   P7, live Play mode — its design note first.
18. **Stable sub-asset ids (roadmap Phase 1 item 1) — done 2026-09-23.**
   A model's sub-asset keys are its parts' names (`mesh/Cube`,
   `anim/walk`, `skin/Skeleton`; `#3` for an unnamed part, `~2` for a
   repeated name), and imported-prefab ids derive from node name paths, so
   reordering parts in the modelling tool keeps every reference; renaming
   one breaks its references loudly (`docs/design/stable-sub-asset-ids.md`,
   documents.md § Sub-asset keys). Sidecar `formatVersion` 2, `fynkel.gltf`
   6, `fynkel.fbx` 2. `fynkel migrate` moves a project
   (`meta/1-2/name-keys`): each outdated model imported once, the
   sidecar's keys renamed keeping their uuids, every reference renamed,
   and override targets on a model's hierarchy — direct, through a rig or
   through a nested prefab — moved to the name-derived ids. The editor's
   migration preview shows the same plan. The cook key now carries the
   sidecar's `formatVersion` and sub-asset keys, so a migrated model never
   reuses a stale import. Every project, fixture, the starter template
   and `benchmarks/stress/generate.py` are migrated. The cooked format and
   the fingerprint are unchanged. Next: Phase 1 item 2 (`SetActive`).
19. **Material remapping (roadmap Phase 1, next to item 1) — done
   2026-09-24.** A model's sidecar `settings.materials` maps an embedded
   material's key to a project material document; the renderers of its
   hierarchy draw that material in every instance (an instance's override
   still wins; explicit references to the embedded material and the
   embedded sub-assets are untouched). The targets are runtime
   dependencies, not key inputs. New `FYN-A0007` for a target that is not
   a material document. `fynkel.gltf` 7, `fynkel.fbx` 3. Editor: a
   Materials section on a model's asset page and Extract Materials (one
   asset-history step); CLI: `fynkel asset remap` and `fynkel asset
   extract-materials` (`docs/design/material-remapping.md`, documents.md
   § Material remaps). Cooked format and fingerprint unchanged. Next:
   Phase 1 item 2 (`SetActive`).
20. **SetActive / activeInHierarchy (roadmap Phase 1 item 2) — done
   2026-09-24.** An entity may carry `"active": false` (absent = true, no
   format bump); a prefab instance overrides it with the `setActive` op;
   the authoring command is `setObjectActive`. At runtime an inactive
   subtree is not drawn, simulated, animated or heard (`fynkel_scene`'s
   `InactiveSelf`/`Inactive` markers, every system filtered; a body leaves
   the physics world and returns at rest; a looping source resumes). New
   wire command `SetActive` (kind 13), `EntityInfo.active`, fingerprint
   `0xa6bee88bec527d9b`. Dart: `GameObject.setActive`, `activeSelf`,
   `activeInHierarchy`; a behaviour is live only while enabled and active
   in the hierarchy (deferred `onCreate`, `onDisable` children first,
   `onEnable` parents first). Editor: the Inspector header's checkbox
   (with override Revert on a prefab instance), a greyed Hierarchy
   subtree, live Play and Apply to Scene. Design:
   `docs/design/set-active.md`. Next: Phase 1 item 3 (editor and cook
   scale).
21. **Editor and cook scale (roadmap Phase 1 item 3) — done
   2026-09-24.** Measured on a generated project of 10 001 assets and
   11 600 references (`benchmarks/scale/`: `generate.py`, the
   `fynkel_authoring` example `scale`). `AssetDatabase::by_path` is a map,
   so the editor's reference prune after a rescan is linear, and a
   one-file rescan takes 2 ms instead of 29 ms. The cook no longer holds
   every source's bytes (peak RSS 238 → 155 MB). It stamps each source's
   size and time in `.fynkel/cache/stamps.json`, so an unchanged source
   is not read again. It no longer reopens cached textures and sounds to
   look for a prefab chunk. A cook with nothing to do takes ~850 ms
   instead of 1 050 ms; the rest is parsing and validating every sidecar
   and document. Cache version, keys and fingerprint are unchanged.
   Next: Phase 1 item 4 (automatic sidecars).
22. **Automatic sidecars (roadmap Phase 1 item 4) — done 2026-09-24.**
   A source file with no sidecar (dropped into `assets/` from Finder, or
   present when the project opens) gets a fresh one on the editor
   session's scan (`OpenProject::rescan`: a new v4 id, the importer from
   the extension), then it is read like any asset, so there is no
   `FYN-A0001` to act on. The rescan report lists `sidecarsWritten`
   (`RescanReport.sidecarsWritten` in Dart). An existing sidecar is never
   replaced, and nothing is written in a read-only project. `fynkel
   validate` still reports `FYN-A0001`: the CLI does not write files
   unasked. Next: Phase 1 item 5 (housekeeping).
23. **Housekeeping (roadmap Phase 1 item 5) — done 2026-09-24.** CI
   runs the editor's suite (`apps/editor`: analyze and `flutter test`,
   after the reference game). A `wasm` job runs `cargo check --target
   wasm32-unknown-unknown` over `fynkel_core`, `fynkel_scene`,
   `fynkel_physics` and `fynkel_render` (blueprint §12.1). Core, scene and
   physics were already clean. Render needed one change: `fynkel_document`
   (reached through `fynkel_assets`) enables uuid's `js` feature on that
   target only, for fresh ids; `Cargo.lock` is unchanged. The ADR index in
   `docs/README.md` now lists 0019–0021 and shows 0018 as accepted.
   **Phase 1 is complete.** Next: Phase 2 (close M4).
24. **Scripts in the editor (roadmap Phase 2 item 1) — done 2026-09-24.**
   The Project browser has a Scripts root over `lib/`. A double-click
   opens a script in the IDE; a file that declares a behaviour shows as
   one. **New ▾ → New Behaviour…** writes a class from a template, opens
   it in the IDE, runs `fynkel codegen --project <game> --project-only`
   and then the session's new `registerBehaviour` operation. That
   operation adds the type to the project's component registry (undoable)
   and re-reads the metadata and schemas, so Add Component lists the type
   at once. `--project-only` is a new CLI flag that skips the native
   pipeline, so the editor never rewrites the engine's generated files.
   Running codegen from the editor is a dated amendment to ADR-0016
   (explicit action only; the CLI subprocess runs `build_runner`, never the
   editor process). Next: Phase 2's Profiler panel.
25. **The Profiler (roadmap Phase 2) — done 2026-09-24.** The Profiler
   panel, a placeholder until now, charts the per-second stats windows
   the game already sends under Play (the Game view and Build and Run
   alike). It has CPU, GPU, Rendering and Memory charts with 60 and 120
   fps lines, Record, Clear, Clear on Play, a selectable window, and that
   window's full breakdown (timers as p50 / p95 / p99). The editor
   samples nothing itself. Next: IDE navigation.
26. **IDE navigation (roadmap Phase 2) — done 2026-09-24.** A game's
   uncaught errors now reach the Console's Game tab with their Dart
   stack. While an editor listens, `FynkelGameView` installs reporters
   before Flutter's and the platform dispatcher's error handlers, which
   still run. The SDK method is the new `EditorChannel.error`. The row's
   location is the first frame in the game's own code (`lib/…:line`).
   ↩ or a double-click opens the IDE at that line, and the details list
   every frame, with the game's own frames as links. Build and Run's
   printed frame lines open the same way. The Inspector's Open Script
   now opens at the behaviour's `class` declaration. Next: the a11y/l10n
   basics, the docs, and the M4 gate report.
27. **Scripts, found by the user's check (2026-09-24).** Four fixes after
   the user tried New Behaviour.
   - The editor used an old `fynkel` from PATH that lacked
     `--project-only`. It now takes the newest of PATH and the checkout's
     build, and says so when the CLI is too old.
   - A failed `codegen` could leave the generated files deleted. The CLI
     now restores them on any failure.
   - A new behaviour vanished on Play after a hot reload. The generated
     types were top-level finals, which a hot reload keeps. They are
     getters now.
   - A saved script now updates the Inspector: the editor runs codegen
     and the new `reloadBehaviours` (second ADR-0016 amendment).
   - A field added to a script later showed "—" because the scene had
     no value for it. The Inspector now shows the script's default, as
     the game already used.
   - Faster: the editor keeps `fynkel codegen --watch` running, so a
     save reaches the Inspector in about a second (it was 3.3 s, and
     18 s for the first save). Play waits while a script has an error
     (third ADR-0016 amendment).

28. **Accessibility and the words (item 9, 2026-09-24).** The keyboard
   reaches every panel: Go to *Panel* on Unity's ⌘1… keys, with each
   panel a Tab group. The Scene tools keep only Q W E R Y, as in Unity.
   A screen reader finds no nameless control (the panel splitters
   excepted: they are the docking package's). A keyboard-only walkthrough
   is a widget test. The command, menu and panel words are in one table,
   `lib/src/l10n/strings.dart`. The words inside the panels are not yet.

29. **The M4 gate report (item 10, 2026-09-24) — GO on 2026-09-25
   (item 30).** `docs/m4-gate-report.md`: five of the six gate rows pass;
   the sixth is the user's own rebuild of the start area, with its
   checklist in the report. Writing it found two gaps, both closed:
   - There was no authoring log. The session now writes every change —
     each transaction's commands, undo, redo, save, asset operations — to
     `.fynkel/authoring.log`, and a log replayed on a copy writes the
     same bytes.
   - The startup scene could only be changed by editing
     `fynkel.project.json`. A scene's context menu now has **Set as
     Startup Scene**.
   Warm `fynkel editor` open on the reference game: the built app launches in 0.2 s,
   hot reload on at 2.0–2.5 s.

30. **M4 closed — GO (2026-09-25).** After the Unity parity pass (tiers
   1–3, `docs/unity-parity.md`; ADR-0022's variants amendment, ADR-0024
   and ADR-0025 confirmed by the user the same day) the user ran the
   gate's hands-on session and reported "all working good". It found one
   bug, fixed in `a69d448`: Finder's `.DS_Store` got an automatic sidecar.
   The report's §5 lists the carry-overs (F1–F7), among them the Unity
   differences left (a selection spans one scene; no Rig or Animation
   import tabs) and CI, which GitHub has not run since 2026-09-23
   because of the account's billing. Next: roadmap Phase 3.
