# Hosts sharing the core (M1-07, iOS smoke M1-09)

The engine is a library and the host owns the loop (ADR-0007). Three hosts
drive the same `fynkel_core` with the same per-frame sequence (ADR-0008):

```text
begin_frame(dt) -> plan
for each planned fixed tick:
    pull events            (drain_events)
    gameplay               (stage commands on the simulation lane)
    step()                 (apply staged commands, propagate, snapshot, publish events)
presentation writes        (presentation lane; override this frame's extract)
extract()                  (S[n-1]..S[n] interpolated by alpha, overrides applied)
render                     (host-specific target)
```

| Host | Crate / app | Loop source | Gameplay | Render target |
| --- | --- | --- | --- | --- |
| Rust headless | `hosts/headless` (`fynkel_host_headless`, lib + bin) | test loop, one fixed step per frame by default | Rust `DemoScene` (commands) | none; reports JSON |
| Standalone | `hosts/standalone` (`fynkel_host_standalone`) | winit `RedrawRequested`, vsync (`Fifo`) | Rust `DemoScene`, or a cooked scene (`--cache`) | winit swapchain via `fynkel_render` |
| Flutter macOS + iOS | `hosts/flutter_apple` (`macos/`, `ios/`), a thin app on `package:fynkel_flutter` | `FynkelGameView`'s Flutter `Ticker` on the main isolate | Dart `DemoScene` on `package:fynkel` | `platform/apple` IOSurface ring → Flutter `Texture` (`FynkelViewport`) |

All three render (or checksum) the same scene: a tumbling hub cube with
`satellites` child cubes on their own rings, driven by one `SetTransform`
batch per fixed tick — the simplest thing that exercises hierarchy
propagation, staged commands and alpha interpolation.

## `fynkel_render`

`ExtractRenderer` (crate `fynkel_render`, `docs/renderer.md`) draws an
`Extract`: the scene's cooked meshes with their PBR materials (or, for the
demo scene, instanced placeholder cubes) from its camera and lights,
through the `shadow → opaque → transparent → post → presentation` chain
into the host's target. It is host-agnostic: it takes a `wgpu::Device`,
a target format and a `TextureView`; the host's format decides whether
the post pass sRGB encodes itself. The project's `quality` profiles
(MSAA, shadows, render scale, anisotropy) come from the cooked cache
through the world's store and a host switches them by name
(`set_quality_profile`); a host can also queue debug lines (`debug()`).
`render_offscreen` is the readback helper for tests. A scene that authors
no lighting (the demo scene) is lit by the renderer's fallback sun, which
casts a shadow.

## Headless host and `fynkel test`

```sh
cargo run -p fynkel_cli -- test --headless --steps 600 --seed 42 --satellites 24 --json
cargo run -p fynkel_host_headless -- --steps 600 --json
```

The report carries tick count, live entities, commands applied/rejected,
events, diagnostics, missed deadlines, step-time percentiles and a
**checksum**: FNV-1a 64 over the final extract's world matrices. Equal
configs must give equal checksums (blueprint §5.6); the crate's tests assert
this and that seeds differ. The Dart headless game host (Behaviours) plugs
into the same command later.

Inside a project (or with `--project <dir>`, `--scene <path|id>`,
`--cache <dir>`) `fynkel test --headless` cooks the project and loads a
cooked scene instead of the demo scene — the host reads the cache only
(M3-03: `HeadlessConfig { cache, scene }`, `fynkel_assets::Assets`; no
source document is opened, prefabs were expanded by the cook), retains the
scene's closure, then just ticks `--steps` times. The report gains `scene:
{ cache, path, loaded, id, name, key, entities, rejected_components,
retained, document_diagnostics[] }` (`FYN-R` codes when the cache is
missing, stale or lacks the scene). The standalone host takes the same
cache (M3-06): `fynkel_host_standalone --cache <dir> [--scene <id>]
[--quality <profile>] [--seconds N]` loads the cooked startup scene (or
`--scene`) into its world, retains the closure, points the renderer at the
store (placeholders off) and ticks it without gameplay — it never cooks
(exit 1 on a missing or stale cache, 2 on usage or an unknown profile).
Its per-second line ends with the renderer's counts — `draws=… instances=…
culled=… batches=… shadow=<casters>/<draws>` — which
`benchmarks/m3/capture.py` reads for the stress scene.

`--input <file.input.json>` (M3-07, [input.md](input.md)) replays a
recorded input stream: the project's `inputActions` are bound after the
scene loads, each recorded frame is `capture_input(events)` + the frame's
ticks at the recorded `dt` (the recording's end ends the run, as does the
ordinary `--steps` cap), and the report
gains `input: { recording, frames, events, actions: [{ name, kind,
pressed, released, held_ticks, held, x, y }], binding_errors }` — per
action the ticks that saw an edge (exactly one per press, §5.1), the ticks
it was held and its final state. A binding the table cannot compile or an
unreadable recording is reported and fails the run. The text mode prints
one line: `replayed test/walk.input.json (70 frames, 11 events): move held
11 ticks, ends at (0.00, 0.00), jump pressed 2 released 2 held 4 ticks, …`.
Behaviours do not run in the Rust host, so this proves the stream and the
table; `hosts/headless/src/recording.rs` is the format's Rust reader and
writer (`Recording`, `RecordedFrame`, `RecordedEvent`).

Physics (M3-08, [physics.md](physics.md)): the host applies the project's
`physics.gravity`, registers the scene's mesh colliders from the cache
(`Assets::register_mesh_colliders`) and counts the events of every tick;
when anything stepped, the report gains `physics: { bodies, colliders,
mesh_colliders, characters, collision_enter, collision_stay,
collision_exit, trigger_enter, trigger_exit, diagnostics }` and the text
mode prints `physics: 2 bodies, 3 colliders (0 mesh), 0 characters,
collisions 1 enter / 0 stay / 0 exit, triggers 0 enter / 0 exit`. The
fixture project's second scene, Playground (`--scene
c0000000-0000-4000-8000-000000000031 --input test/platform.input.json`,
M3-09), walks its `fynkel.CharacterController` Player onto a platform
from the recording with the `fynkel.ThirdPersonCamera` rig behind it —
the item 9 gate, a headless test with a stable checksum. The third
scene, Catalogue (`--scene c0000000-0000-4000-8000-000000000032`,
M3-10), runs the six native catalogue components with no behaviour —
a headless test with a stable checksum ([catalogue.md](catalogue.md));
the host registers the prefabs the scene's `Emitter`s name
(`Assets::register_prefabs`, the scene report's `prefabs`) and
accumulates `World::system_times` over the run into `systems: [{ system,
entities, total_us, max_us, mean_us }]` (tick order, `step` first),
printed by the text mode as `systems (mean per tick, last count): commands
0 µs (0), … physics 2017 µs (6081), …`. M3-16
([instrumentation.md](instrumentation.md)): the host extracts once per
frame as a renderer would and reports `extract_time` and `frame_time`
(begin_frame → extract) next to `step_time`, and `memory { native_bytes,
native_peak_bytes, resident_bytes, residency_bytes }` at the end of the
run; the text mode prints a `timing:` and a `memory:` line. The checksum covers the bodies' final
poses, so a physics regression changes it; the fixture project's Crate B
landing on its Floor and sleeping is a test of the crate. Animation
(M3-19, [animation.md](animation.md)): the host registers the clips and
skins the scene's `Animator`s and `Skin`s name (`Assets::register_animation`;
a blend space counts as one clip and brings its points' clips, M3-19c)
and counts the `ANIMATION_ENDED` and `ANIMATION_MARKER` events and the
animation diagnostics of every tick; when an animator or skin was
stepped the report gains `animation: { entities, clips, skins,
skinned_meshes, joints, ended, markers, diagnostics }` and the text mode
prints `animation: 2 animators + skins, 1 clips, 1 skins, 1 skinned
meshes (11 joints), 0 ended, 0 markers, 0 diagnostics`;
the fixture project's fourth scene, Rig (`--scene
c0000000-0000-4000-8000-000000000034`), poses its skinned column and its
Prop — a headless test with a stable checksum (the joints' poses are in
it). Audio (M3-11, [audio.md](audio.md)): the host registers the clips
the scene's `AudioSource`s name (`Assets::register_audio`), never opens a
device (the null backend runs the same clock), counts the
`PLAYBACK_ENDED` events and the audio diagnostics (`SOUND_UNAVAILABLE`,
`AUDIO_DEVICE`, anything on an `AudioSource`) of every tick; when the
scene has a source or sound the report gains `audio: { sources, sounds,
streams, started, stolen, refused, ended, diagnostics }` and the text
mode prints `audio: 1 sources, 1 sounds (0 streamed), 1 voices started
(0 stolen, 0 refused), 1 ended, 0 diagnostics` — the fixture project's
Robot plays its tone once at the load and it ends at tick 59 (test);
the reference game's `Music` is `1 sounds (1 streamed)`, its decoder
thread running under the headless ticks like under the app's. The standalone host takes
`--audio` to open the output device after the load (`audio: output
device open at 48000 Hz, 2 channels`, or the `AUDIO_DEVICE` diagnostic),
suspends it while the window is occluded, closes it with the window and
prints the engine's counters at the end (`audio: 1 voices (1 started, 0
ended), 1 clips (1 streamed), 1131 callbacks, 0 underruns, 0 starved` —
the reference game's music streamed to the Mac's device for 12 s).
Read-side:

```sh
cargo run -p fynkel_cli -- state dump --scene assets/scenes/main.scene.json --steps 60 --json
```

prints the runtime state keyed by **authoring id** (never handles; the
entities a prefab instance expands to carry the ids the cook derived):
each entity's `id`, `name`, `parent`, `children`, `local`/`world`
transform and typed `components[]` in the document's own JSON shapes; exit
1 when the scene does not load or a component value was rejected, 2 for a
scene outside any project (nothing could cook it). This is the M2 gate
"project can be validated/imported headlessly", now from the cache.

### Prefabs at runtime (M3-08b)

`fynkel_world_instantiate(world, &FynkelInstantiateDesc { prefab_id_utf8,
prefab_id_len, name_utf8, name_len, parent, position, rotation, scale })`
spawns a cooked prefab **now**, outside the command path, the way a scene
instance expands: a new *anchor* entity (named `name`, or the prefab's
name when empty; at the transform; a child of `parent`, or a root when
`parent` is 0) whose children are the prefab's roots, the prefab's entities
under them with their own local transforms, their components applied
through the same command path a scene load uses (`rejectedComponents`),
its mesh colliders registered and its asset closure retained on the
world. The prefab comes from the loaded project's cache only
(`Assets::prefab(id)` reads the flat `document/prefab` chunk the cook
wrote — or, since M3-19a, the `prefab` chunk of a cooked **source**: an
imported model's node hierarchy, keyed by the source's own id, so
`World.instantiate(AssetRef(glb, sub: 'prefab'))` spawns the rig with its
meshes and `Skin`s and an `Animator` added to the anchor finds the joints
by name; no new ABI, the Dart side checks the sub — empty or `prefab`,
anything else is an `ArgumentError` — and passes the id; the closure
retained is the source's own): no project loaded is `FYN-R0001`, an id
that is neither a prefab document nor a source with a hierarchy
`FYN-R0006`, one not in the cache `FYN-R0003`, a dead or foreign `parent`
or a non-finite transform is a load error; every refusal returns
`FYNKEL_STATUS_LOAD_FAILED` with nothing spawned, a malformed id or
string `FYNKEL_STATUS_INVALID_ARGUMENT`. `fynkel_world_load_report`
then carries `{ ok, scene: null, instance: { prefab, name, path, anchor,
entities, rejectedComponents, assets, meshColliders, behaviours[] },
cache, residency, diagnostics }` — the `behaviours` are the prefab's
authored Dart behaviours with their spawned handles, attached by the SDK
like a scene's (`FYN-B0001`/`B0002` warnings through
`onLoadDiagnostics`). `World.instantiate` returns the anchor as a
resolved `GameObject` (`lastInstantiate` is the `InstantiateReport`);
`FynkelInstantiateError` wraps a refusal.

### Scenes at runtime (M3-12)

`fynkel_world_load_scene(world, cache, scene_id)` **replaces**: every
loaded scene is unloaded (last first) and every remaining root subtree
destroyed — runtime prefabs and spawns included — before the scene
loads, so nothing survives a level change (`DESTROYED` events, one
`SCENE_UNLOADED { scene, entities }` per scene, then `SCENE_READY {
scene }`, in that order on the next drain). `fynkel_world_load_scene_with(
world, cache, scene_id, flags)` takes `FYNKEL_LOAD_ADDITIVE` (1) to keep
the world and stack the scene (an id that is loaded already is
`FYN-R0011`, nothing changes; unknown flag bits are
`INVALID_ARGUMENT`). `fynkel_world_unload_scene(world, scene_id)`
destroys the still-alive entities of that scene only (runtime spawns
stay) and releases its residency; a scene the world does not hold is
`FYN-R0010`. The load report gains `mode` (`replace` | `additive`) and
`scenes: [{ id, name, entities }]` (what the world holds, load order);
an unload's report is `{ ok, unloaded: { id, name, entities }, scenes,
residency, diagnostics }`. `fynkel_world_bind_actions(world, json)`
lays the user's rebinds — a JSON object `{ "<action>": ["<device>:
<control>", …] }`, `{}` to clear — over the loaded project's actions
now and after every later load (the world keeps them); the report is `{
ok, input: { actions: [{ name, type, bindings, rebound }],
bindingErrors[] } }`, `LoadFailed` before a project. Event kinds on the
general lane are `SCENE_READY` 5 (now with its scene's `AssetRef`),
`SCENE_UNLOADED` 6, `ZONE_ENTERED` 7 and `ZONE_EXITED` 8 (both a
`fynkel.ev.Zone { zone, other, event }` — the `fynkel.TriggerZone`'s
string comes through the events buffer's string table). The reference
tests: `crates/fynkel_native/tests/load.rs`
(`scenes_stack_unload_replace_and_zones_fire_through_the_abi`),
`crates/fynkel_core/tests/scenes.rs`; the contract [scenes.md](scenes.md).

## Flutter host

`hosts/flutter_apple` uses **one** dylib, `libfynkel_platform_apple.dylib`,
which exports both the engine ABI (`fynkel_*`, used by `package:fynkel`) and
the viewport ABI (`fynkel_apple_*`, `platform/apple/include/fynkel_apple.h`).
It arrives as the code asset `package:fynkel/fynkel_native` from
`packages/fynkel/hook/build.dart` (ADR-0015): explicit `native_lib`
user-define → source checkout `cargo build` → checksummed prebuilt named by
`fynkel.lock.json` → `cargo build` fallback when Rust is installed → an
error that names the fix. All bindings are `@Native` on that asset; nothing
calls `DynamicLibrary.open`. The Flutter toolchain bundles it as a
universal framework, so the hook builds once per architecture.
Each Flutter frame: `World.frame(dt, fixedUpdate: …)` (gameplay + `step()`
per planned tick, presentation flush), then
`fynkel_apple_viewport_render(viewport, world)` which extracts the world in
Rust and renders into a free ring slot. Swift's `FynkelViewportTexture`
hands the newest GPU-complete slot to Flutter's `Texture`.
`FynkelAppleViewportStats` ends (appended) with the last frame's
counts — M3-05a: `transparent_last_frame`, `lights`, `lights_dropped`,
`shadow_casters`, `shadow_map` (side, 0 when none), `msaa`,
`render_width`, `render_height`; M3-06: `culled_last_frame`,
`draws_last_frame`, `shadow_draws_last_frame` (`instances_last_frame` is
what the scene passes drew, the culled are not in it) — which
`ViewportStatsSnapshot` carries to Dart (the showcase HUD prints them;
the Flutter host's stats line has `draws=… culled=… shadow=…`).
M3-16 (ABI 3, [instrumentation.md](instrumentation.md)): `extract_*`,
`prepare_*`, `encode_*` percentiles of the render call's stages,
`gpu_timestamps`, `gpu_frames_timed`/`gpu_frames_untimed`, the
`gpu_shadow_*`/`gpu_scene_*`/`gpu_post_*`/`gpu_frame_*` percentiles from
Metal timestamps and `gpu_bytes`, the renderer's GPU memory estimate.
`fynkel_apple_viewport_set_quality(viewport, name, len)` (M3-05, additive
to ABI 2) selects a quality profile of the loaded project by name (an
empty name: the project default); `-1` and a logged warning naming the
profiles when it does not exist. **ABI 4** (M4-06, additive; the
editor's Scene view): `fynkel_apple_viewport_debug_lines(viewport,
lines, count)` queues world-space line segments for the next render —
ten floats each (`ax ay az bx by bz r g b a`, metres, linear RGBA) —
drawn by the renderer's debug pass (depth-tested against the opaque
scene, unlit) and cleared by every render, so a host queues again before
each frame; `fynkel_apple_viewport_read_frame(viewport, slot, out, cap)`
copies the frame of ring slot `slot` (a render's return value) as
tightly packed RGBA8, top row first, blocking until the GPU is idle — a
screenshot, never a per-frame call. Games never call either.

Since M3-01 all of that lives in **`package:fynkel_flutter`**
(`packages/fynkel_flutter`), the Flutter plugin every game depends on:

- `darwin/fynkel_flutter/` — one Swift package for macOS and iOS
  (`sharedDarwinSource`, SwiftPM with a podspec for CocoaPods projects):
  `FynkelViewportTexture` (the `FlutterTexture` over the ring) and
  `FynkelFlutterPlugin`, registered by the generated plugin registrant, on
  the method channel `fynkel_flutter/viewport`: `registerTexture` (ctx,
  acquire/set-callback function addresses, IOSurface addresses → `textureId`,
  installs the Rust→Swift frame-ready callback), `rebindTexture`, `stats`,
  `unregisterTexture` — every call after registration names its
  `textureId`, so an editor can show several viewports.
- `lib/src/viewport_native.dart` — the viewport ABI **v5** as `@Native`
  externals on the same code asset as `package:fynkel` (v5 adds `gpuAo` to
  the stats struct: R2's ambient-occlusion stage).
- `FynkelViewportController` — one native viewport: `requestSize(w, h)` in
  physical pixels creates the ring and registers the texture, later calls
  are coalesced resizes (native resize → Swift rebind; `render()` returns
  null in between so the simulation never waits); `assetCache` (only for a
  world that loaded no cooked scene — a viewport rendering a world that
  `loadProject`/`loadScene` filled adopts that world's `fynkel_assets` store
  at the first `render`, M3-03), `placeholders`, `camera`, `quality` (a
  project profile name or null for the default, pushed to the viewport
  when attached; `qualityOk` says whether the last name was known;
  M3-05); `takeStats()` (Rust) and `platformStats()`
  (Swift); `detach()` is the ordered teardown (clear callback → unregister →
  destroy) and the controller is reusable afterwards. M4-06 (ABI 4): `renderHandle(worldAddress)` renders a world the controller does not own — the editor's edit-preview world, by its native address —, `debugLines(Float32List)` queues the overlay segments for the next render, `readFrame(slot)` copies a rendered slot as RGBA8 (`Uint8List`, `physicalWidth × physicalHeight × 4`).
- `FynkelViewport` — the widget: `LayoutBuilder` × device pixel ratio →
  `requestSize`, the `Texture`, `Focus` + `Listener` (pointer positions in
  viewport physical pixels; `onFocusChange`), a placeholder until attached
  and the error when creation failed.
- Input (M3-07, [input.md](input.md)) — `FynkelGameView(input:
  FynkelInputCapture)` hands `capture.take(dt)` to `World.frame` each frame
  (a capture is owned when none is given): keys from the viewport's focus
  after `onKeyEvent` declined them, mouse buttons/movement/wheel from its
  `Listener`; `FynkelGameViewState.capturePointer(bool)` hides and captures
  the cursor (macOS), `hasFocus`/`pointerCaptured`; a focus loss, a pause
  and a hidden app reset the capture and release the pointer.
  `FynkelInputChannel` is the method channel `fynkel_flutter/input`: the
  Swift `FynkelInputPlugin` (registered with the viewport plugin) sends
  `events` batches of `[device, control, phase, x, y]` rows — gamepad
  changes from `GCController` on macOS and iOS, captured-mouse deltas on
  macOS — and answers `capturePointer {capture}` and `gamepads`.
  `TouchOverlay` (`OverlayStick`/`OverlayButton`, `fromActions(report.
  inputActions)`) draws `touch:overlay/<name>` controls into the same
  capture on touch platforms. The ABI: `fynkel_world_capture_input(world,
  buffer, len)` takes a `FYNKEL_BUFFER_INPUT_EVENTS` (6) batch,
  `fynkel_world_input_view(world)` returns the world's
  `FYNKEL_BUFFER_INPUT_ACTIONS` (7) buffer, read in place by `WorldInput`.
  Native components from Dart (M3-07a): `GameObject.get<T>()`/`set`/
  `add`/`remove` are `FYNKEL_COMMAND_SET_COMPONENT` (6: the
  `SetComponentCommand` header then the component's bytes) and
  `FYNKEL_COMMAND_REMOVE_COMPONENT` (7) on either lane, read back through
  `FYNKEL_QUERY_COMPONENTS` (6: every native component of an entity, each
  framed by a `ComponentHeader { component, size }`) — one query per object
  per tick; `docs/design/dart-sdk.md` §21.2.
  Physics (M3-08, [physics.md](physics.md)): `applyForce`/`applyImpulse`/
  `setLinearVelocity`/`setKinematicTarget`/`teleport` and friends are
  `FYNKEL_COMMAND_BODY` (8: `BodyCommandRecord { entity, op, vector,
  rotation }`, simulation lane only, applied by the tick's physics step),
  `bodyState()` is `FYNKEL_QUERY_BODY` (7: one `BodyStateRecord`),
  `move(v)`/`jump([speed])` the `Move`/`Jump` body ops (a `Jump` carries
  its speed in `vector.y`) and `characterState()` is
  `FYNKEL_QUERY_CHARACTER` (9: one `CharacterStateRecord { entity,
  grounded, sliding, velocity }`) (M3-09; component ids 12
  `CharacterController` and 13 `ThirdPersonCamera` in the header),
  the native catalogue's six components are ids 14–19 in the header
  and travel the M3-07a path, and `World.systemTimes()` is
  `FYNKEL_QUERY_SYSTEMS` (10: one `SystemTimeRecord { system, entities,
  micros }` per `SystemKind` in tick order, the last step's wall time —
  the starter HUD's `systems` line; M3-10, [catalogue.md](catalogue.md)),
  `FynkelWorldStats` (M3-16, [instrumentation.md](instrumentation.md))
  ends with `snapshot_bytes`, `snapshot_peak_bytes`, `snapshot_budget`,
  `native_bytes`, `resident_bytes`, `residency_bytes`, and
  `fynkel_world_set_snapshot_budget(world, bytes)` is the subscription
  budget (C5): the first tick over it reports `SNAPSHOT_BUDGET` (20,
  `FYN-R0013`) once per crossing; every load report carries
  `"timing": { "ms" }`,
  `raycast`/`raycastAll`/`shapeCast`/`overlap`/`physicsQuery` send a
  `FYNKEL_BUFFER_PHYSICS_QUERIES` (8) batch of `PhysicsQueryRecord`s to
  `fynkel_world_physics_query(world, buffer, len, &view)` and read the
  `QUERY` buffer of `PhysicsHitRecord`s it answers with; contacts arrive as
  `ContactEvent`s on the physics lane (`fynkel_world_after_physics_events`)
  and reach `onCollisionEnter`/… the same tick. World labels
  (`World.projection.update()`, M3-13, [hud.md](hud.md)) send a
  `FYNKEL_BUFFER_PROJECTION_ANCHORS` (9) batch of `ProjectionAnchorRecord`s
  to `fynkel_world_project(world, width, height, buffer, len, &info,
  &view)` — `FynkelProjectionInfo { camera, projected, dropped }` and a
  `QUERY` buffer of `ProjectedPointRecord`s, one per anchor in order,
  from the frame's camera and interpolated poses; at most
  `FYNKEL_MAX_PROJECTION_ANCHORS` (64) are projected, the rest come back
  `FYNKEL_PROJECTED_DROPPED` with a `TOO_MANY_ANCHORS` (15) diagnostic
  (`fynkel_world_diagnostics` is rebuilt so the host sees it before the
  next step); a framing error is `Protocol` with an empty view and the
  world stays usable. The load report gains
  `physics: { gravity, layers, meshColliders, prefabs }` — `prefabs` is
  how many prefabs the scene's (or an instantiated prefab's) `Emitter`s
  name and the host registered with the world
  (`Assets::register_prefabs`, M3-10).
  Animation (M3-19, M3-19c, [animation.md](animation.md)):
  `GameObject.animation` `play`/`crossfade`/`stop`/`pause`/`resume`/
  `speed =`/`time =`/`weight =`/`setParameter` are
  `FYNKEL_COMMAND_ANIMATE` (9: `AnimateCommandRecord { entity, op,
  seconds, speed, looping, clip, value, name }`, simulation lane),
  `animation.state` is `FYNKEL_QUERY_ANIMATOR` (11: one
  `AnimatorStateRecord { entity, time, duration, speed, fade_remaining,
  playing, looping, clip, weight, parameter, root_delta, root_yaw }`, none
  without an `Animator`), a one-shot's end is
  `FYNKEL_EVENT_ANIMATION_ENDED` (9: `AnimationEndedEvent { entity,
  clip }`) and a marker crossed `FYNKEL_EVENT_ANIMATION_MARKER` (10:
  `AnimationMarkerEvent { entity, clip, name }`) on the general lane, and the
  components are ids 21 `Animator` and 22 `Skin` in the header on the
  M3-07a path; the native layer registers from the cache, before a batch
  is applied, a clip a `play`/`crossfade` or a set `Animator` names and
  the skin a set `Skin` names, so a behaviour is not limited to what the
  scene authored. The load report gains a top-level `animation: { clips,
  skins }` (`LoadReport.animation`).
  Audio (M3-11, [audio.md](audio.md)): `GameObject.audio`
  `play`/`stop`/`pause`/`resume`/`volume =`/`pitch =` are
  `FYNKEL_COMMAND_AUDIO` (10: `AudioCommandRecord { entity, op, value }`,
  simulation lane), `world.audio.play` is `FYNKEL_COMMAND_PLAY_SOUND` (11:
  `PlaySoundCommandRecord { clip, bus, volume, pitch, priority }`, either
  lane, no entity, no event) and `world.audio.setBus`/`applySettings`
  `FYNKEL_COMMAND_AUDIO_BUS` (12: `AudioBusCommandRecord { bus, volume,
  muted }`, either lane; on the presentation lane both apply at submit);
  `audio.state` is `FYNKEL_QUERY_AUDIO` (12: one `AudioStateRecord {
  entity, playing, paused, time, duration, volume, pitch, looping, clip,
  bus, voice }`, none without an `AudioSource`), `world.audio.stats`
  `FYNKEL_QUERY_AUDIO_STATS` (13: one `AudioStatsRecord { device,
  sample_rate, channels, voices, max_voices, started, stolen, refused,
  ended, clips, callbacks, underruns, suspended, streams, starved }`), a source's end
  `FYNKEL_EVENT_PLAYBACK_ENDED` (11: `PlaybackEndedEvent { entity, clip }`,
  general lane), and the components are ids 23 `AudioSource` and 24
  `AudioListener`; a `PlaySound` or a set `AudioSource` registers its clip
  from the cache before the batch like an `Animator`'s. The device is
  `fynkel_world_audio_open(world)` (`FYNKEL_STATUS_OK`, or
  `FYNKEL_STATUS_UNSUPPORTED` (9) with an `AUDIO_DEVICE` diagnostic when
  the platform or the build has none — the null backend stays),
  `fynkel_world_audio_suspend(world, bool)` and
  `fynkel_world_audio_close(world)`; `FynkelGameView(audio: true)` calls
  them after the load, on the app lifecycle and on dispose
  (`World.audio.open/suspend/close`). The load report gains a top-level
  `audio: { sounds, clips, device, sampleRate, channels, maxVoices }`
  (`LoadReport.audio`).
  Developer ergonomics (M3-08b): `World.find(name)`/`findAll(name)`,
  `GameObject.findChild('Arm/Hand')`/`findDescendant(name)` read the
  hierarchy as the last tick left it from **one** `FYNKEL_QUERY_ENTITIES`
  (8: an `EntityInfoRecord { entity, parent, name }` per live entity)
  per tick, cached in the SDK until the next step or load. A temp
  reference (`FynkelEntity` with generation 0 and the spawn's temp id in
  the slot bits, `Entity::temp(id)`) may stand for an entity spawned
  **earlier in the same tick's simulation batch** in any command's entity
  field — the world resolves it against that tick's spawns before applying
  the command (`INVALID_TEMP_ID` when nothing spawned under that id, or
  on the presentation lane) — so `spawn()` then `add(RigidBody)` then
  `applyImpulse` in one `fixedUpdate` all land in the spawn's tick.
  `World.instantiate(prefab, transform:, parent:, name:)` is
  `fynkel_world_instantiate(world, &FynkelInstantiateDesc)` (below).
- `FynkelGameView` — the loop: a `Ticker` runs `World.frame(dt, fixedUpdate:)`
  then `controller.render(world)`; dt is clamped (`maxFrameDt`), the clock is
  re-primed when the app resumes and when the viewport attaches (a start-up
  stall is not simulated time), the ticker is muted while hidden/paused
  (`pauseWhenHidden`), `onStats` delivers a `FynkelFrameStats` per window
  (`frames` and `fps` — ticker callbacks over the window's wall time, the
  rate the game actually runs at; tick interval / simulate / render-call
  percentiles, fixed ticks, missed deadlines, dropped frames, world +
  viewport + platform counters; with `profileDartHeap: true` also
  `dartHeap`, the C3 `DartHeapStats` from the app's own VM service —
  debug/profile builds, null in release; `summary()` is the window as
  one `key=value` line for a capture — instrumentation.md), and
  `overlay` is the HUD composited over the texture. The showcase and
  starter HUDs open with `fps N  frame p50 ms (p99)  render ms`.
- `DartHeapProbe` — the C3 probe on its own: `connect()` (null without
  a service), `sample()` per window (heap in use/capacity/external, the
  bytes allocated since the last sample from the VM's `GC` timeline
  events, collections and pauses), `close()` (restores the timeline
  streams the app recorded before). `hosts/flutter_apple` prints one
  `[alloc]` line per window from it; `benchmarks/m0/capture.py` runs the
  host's `--profile` bundle for the AOT numbers.
- `unpackBundledCache({into, bundle})` — the cooked cache on a device: the
  cook stages `<project>/fynkel_cache/` (`index.json` + the `.fyna` objects
  flat) and the app bundles it as Flutter assets (documents.md § `fynkel
  run`); Flutter assets are readable through `rootBundle` only while the
  runtime opens a cache *directory* by path, so this writes the bundle to
  `<system temp>/fynkel_cache/{index.json, objects/}` (content-addressed:
  objects already there are kept, unnamed ones deleted, the index always
  rewritten — a warm unpack is milliseconds) and returns the path for
  `World.loadProject(dir, cacheDir: …)`, or `null` when the app bundles no
  `fynkel_cache/index.json`. A created project's `main.dart` takes
  `FYNKEL_PROJECT` when that directory exists on the device (macOS, the
  simulator) and the bundle otherwise (an iPhone). Tested against a fake
  `AssetBundle` (`test/bundled_cache_test.dart`).

`flutter test` runs the package's tests against the real Rust viewport (the
hook builds the dylib; Metal creates the ring headlessly) with the texture
channel mocked, so create → render → resize → detach is covered without a
window. `hosts/flutter_apple/lib/main.dart` keeps only the harness: destroy
/ recreate, inset resize, lifecycle log, focus/pointer probes, the HUD, and
the `[host]` lines `benchmarks/m0/capture.py` parses (unchanged format).

Measured on mac-m3max-2026 at 1600×1104 physical, 24 satellites: 120 Hz
frames, 60 fixed ticks/s, Dart simulate ≈ 0.02 ms, `render()` call ≈ 0.1 ms,
present lag 0, no dropped frames.

## iOS smoke test (M1-09)

The same host, Dart code, Swift texture glue (`FynkelViewportTexture.swift`,
one file for both platforms in `package:fynkel_flutter` since M3-01, `#if
os(iOS)` for the imports and registrar accessors) and Rust crate run on iOS: `platform/apple`
has no AppKit/UIKit dependency and the hook cross-compiles it for
`aarch64-apple-ios`, `aarch64-apple-ios-sim` and `x86_64-apple-ios`
(Flutter builds two simulator slices). Two things surfaced:

- the iOS simulator's Metal device sits below WebGPU's default limits (15
  inter-stage variables), so every `request_device` now asks for
  `fynkel_render::device_limits(adapter)` — the defaults lowered to what
  the adapter offers;
- a `MissedDeadline` diagnostic after a startup stall threw `FynkelError` in
  debug mode and stopped the host. Missed deadlines are informational
  (`FynkelDiagnostic.isInformational`): reported through `onDiagnostics`,
  never thrown.

Measured (profile mode, 1320×2508, 24 satellites):

| | iPhone 16 Pro Max (A18 Pro, iOS 26.6.1) | iPhone 16 Pro Max simulator (debug) |
| --- | --- | --- |
| Flutter frames | 120 Hz (ProMotion), tick interval p50 8.33 ms | 60 Hz |
| fixed ticks/s, missed deadlines | 60, 0 | 60, 0 |
| Dart simulate (gameplay+submit+step+snapshot) | p50 0.06 ms, p99 0.12 ms | p50 0.09 ms |
| `render()` call (extract+encode+submit) | p50 0.41 ms, p99 0.56 ms | p50 0.62 ms |
| submit → GPU complete | p50 1.84 ms, p99 2.51 ms | p50 1.31 ms |
| raster acquire wait | p50 0.90 ms, max 1.89 ms | p50 0.00 ms |
| present lag / dropped frames / stale presents | 0 / 0 / 0 | 0 / 0 / 0 |
| Flutter raster | p50 1.73 ms, p99 2.66 ms | p50 0.99 ms |

The cube renders on the device through the same interop; M1-09 gate item
met. Full iOS qualification (touch overlay, lifecycle, thermal, packaging)
is M6. A created game (and the showcase) on a **physical** iPhone reads its
scene from the bundled cache (`unpackBundledCache`, above): the simulator
shares the Mac's filesystem and hid that `FYNKEL_PROJECT` cannot name a
directory on the phone (first seen on the showcase, `FYN-R0001` for the
Mac path).

Path C (engine-owned `CAMetalLayer`) was an M0 prototype; it is not carried
into `platform/apple`. Its numbers remain in `benchmarks/m0/results`, its
code at the M0 commits (ADR-0002 keeps it as the documented fallback).

## Per-frame cost instrumentation (M1-10)

Three probes feed the M1 gate (`docs/m1-gate-report.md`) and are run by
`python3 benchmarks/m0/capture.py` alongside the host benchmarks:

- `dart run tool/alloc_bench.dart [--frames N] [--satellites K] [--json]` in
  `packages/fynkel`: the Flutter demo scene without a renderer, driven from
  Dart; reports heap bytes per frame (heap-usage deltas after forced GC, chunks
  with a GC event excluded), GC count and pause time from the VM service.
  JIT numbers; the class breakdown is indicative only. With the C1 writers
  (`TransformWriter`, `GameObject.writeLocal`) the 25-entity scene sits at
  ≈1.5 KB/frame with no GC in 3 000 frames; the remainder is per-frame SDK
  results (`FramePlan`/`StepResult`) and event/diagnostic views.
- `cargo test --release -p fynkel_native --test leak`: counting global
  allocator over repeated runtime + world lifetimes; bytes in use after
  teardown must equal the baseline.
- `cargo test --release -p fynkel_platform_apple --test lifetime`: repeated
  viewport create/render/resize/destroy; process RSS growth bounded.
