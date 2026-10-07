# Engine comparison: Fynkel vs Bevy vs Unity (2026-10-06)

**Question.** Is Fynkel slow or ugly "under the hood", or did we ask the
phone for too much? Should the runtime move to Bevy (or only the renderer)?
We answer it with numbers from one phone, not with opinions.

**Timebox.** About one week. No engine features while it runs.

## Rules (fixed before any number exists)

1. **One scene, the same files.**
   - Khronos glTF Sponza: 262k triangles, 25 materials, 69 textures,
     with alpha-masked plants. It is fetched by `tools/compare/fetch_sponza.sh`
     into `benchmarks/compare/sponza/` and not committed, because of its
     licence (CRYENGINE agreement: benchmarking only).
   - Added to it: 20 copies of `character.glb`, our own generated skinned
     character (`benchmarks/compare/character.glb`, from
     examples/reference_game), each playing its `walk` clip in place.
   - Every engine loads these files as they are; Unity loads them through
     glTFast.
2. **The scene is made in code, with the same numbers everywhere.**
   Positions are in metres, Y is up, and the Sponza node keeps its own 0.008
   scale. The floor of the nave is at y = 0.
   - **Camera.** φ = 2π·t/30 (t in seconds). The camera sits at
     (10·sin φ, 2.0, 0) and looks along (cos φ, −0.05, sin φ). The vertical
     field of view is 60°, near 0.1, far 100. At t = 0 it stands in the
     middle of the nave looking down it towards +x.
   - **Characters.** Character i (0…19) stands at
     (−9 + i·18/19, 0, i even ? 1.2 : −1.2), turned to face +z (yaw 0),
     playing `walk` on a loop. Its start time is offset by i·0.1 s.
   - **Sun.** It shines along (−0.3, −1.0, −0.25) (normalised) at 10 000
     lux, white, and casts shadows.
   - **Eight point lights.** They sit at (x, 1.5, z) for x ∈ {−10.5, −3.5,
     3.5, 10.5} and z ∈ {−3, 3}. Each is warm (1.0, 0.8, 0.6) at 2 000
     lumen with a 6 m range, and none casts shadows.
   - **Ambient.** Uniform white light at 2 000 lux, with no environment
     map or IBL. In Bevy that is about 637 cd/m² (2000/π).
   - **Exposure.** Manual, EV100 = 12, with ACES tone mapping and a plain
     sky colour (0.5, 0.6, 0.8).
   - **Shot mode.** `t` is frozen at 0, the characters at 0.5 s into the
     clip, and a PNG of the engine's own picture is saved after 5 s.
3. **Three settings levels.** Each level runs at native resolution and at
   0.75 scale.

   | | Plain | Full | Full+MSAA |
   |---|---|---|---|
   | MSAA | off | off | 4× |
   | Shadows | 4 cascades, 2048 | 4 cascades, 2048 | 4 cascades, 2048 |
   | AO | off | screen-space | screen-space |
   | Bloom | off | on | on |

   - Bevy's SSAO cannot run with MSAA, so Bevy has no Full+MSAA run.
   - Contact shadows are left out, because not every engine has them.
   - Plain also uses 4 cascades, because Fynkel always draws 4 (`shadow.rs:24`).
   - **Stats output.** Every app prints one line a second, so one parser
     reads them all:
     `[cmp] engine=<name> level=<plain|full|msaa> scale=<s> fps=<f> cpu_ms=<median> gpu_ms=<median> gpu_p95=<p95> thermal=<0-3>`.
     Thermal is `ProcessInfo.thermalState`: 0 nominal, 1 fair,
     2 serious, 3 critical.
4. **One phone, same state.**
   - The phone is the iPhone 16 Pro Max on iOS 26.6.1, release/profile
     builds, 120 Hz, unplugged.
   - It starts at thermal state nominal and cools between runs.
5. **Measured the same way everywhere.**
   - Measurements: GPU time per frame (median and p95), CPU time per frame,
     frames per second, and the thermal state after 5 minutes.
   - Each engine uses its own GPU timestamps: Fynkel's `[stats]`, Bevy's
     render diagnostics and Unity's `FrameTimingManager`.
   - One run per engine is cross-checked with an Instruments Metal System
     Trace.
6. **Pictures are compared too.** One screenshot per engine and level, from
   the same pose, goes into `benchmarks/compare/shots/`. The user judges how
   they look.

## The runs

| # | What | Why |
|---|---|---|
| C1 | Fynkel, in Flutter (`FynkelGameView`) | what we ship today |
| C2 | Fynkel, standalone (no Flutter) | what the Flutter texture bridge costs |
| C3 | Bevy, native iOS app | Bevy's renderer and runtime on the same content |
| C4 | Unity 6 URP, native iOS app | the outside reference |
| C5 | Bevy drawn into Fynkel's Flutter bridge | only if C3 clearly beats C1 |

## Decision rule

- **Bevy is clearly faster at the same settings, or clearly better-looking**
  (more than 20 % less GPU time, or the user prefers its picture): the
  runtime or renderer moves to Bevy. That needs an ADR and the C5 bridge
  proof first.
- **About the same as Fynkel and Unity:** the engine is not the problem.
  We do the quality pass (docs/foundation.md style) and ship mobile
  defaults.
- **Unity is clearly better than both:** the renderer is the problem in
  general. We look at renderer-only options (Filament, or Bevy's
  renderer alone).
- **C2 is much faster than C1:** the Flutter bridge is a bottleneck,
  whatever the answer above.

## Results (2026-10-07, partial — stopped by the decision below)

iPhone 16 Pro Max, iOS 26.6.1, release builds, one 60 s run each (the
first 15 s dropped), alternating engines with 90 s cool-downs. GPU time is
each engine's own timestamps. Fynkel's settings were baked in with
`--dart-define` (devicectl's launch environment does not reach a Flutter
app's `Platform.environment` on iOS).

**Cubes** — the Bevy spike's scene (spikes/bevy_flutter): ground, shadowed
sun, 1 000 cubes moved from Dart every frame, one walking character.
Bevy runs inside Flutter (C5, the spike); Fynkel in `FynkelGameView` with
`CMP_SCENE=cubes`, level `plain`.

| | fps | GPU ms | engine CPU ms/frame | Dart ms/frame |
|---|---|---|---|---|
| Fynkel, native 1320×2868 | 120 | 5.9 | 0.7 (render thread) | 2.3 |
| Bevy in Flutter, native | 120 | 5.4 | 2.6 (`app.update`) | 0.02 |
| Fynkel, 0.75 | 120 | 5.9 | 0.7 | 2.3 |
| Bevy in Flutter, 0.75 | 120 | 5.2 | 2.6 | 0.02 |

A tie on what the screen shows; GPU time is a fixed per-frame cost in both
(it hardly moves with resolution or cube count). Fynkel's engine CPU is
about a quarter of Bevy's. The Dart column is API shape, not engine: Fynkel
sets each cube through `GameObject.transform` (1 000 calls), the spike
sends one float buffer.

**Sponza + 20 characters, `plain`, native** (only pair completed):

| | fps | GPU ms | CPU ms/frame |
|---|---|---|---|
| Fynkel (C1, in Flutter) | 106 | 9.2 | 2.9 (render thread) |
| Bevy (C3, standalone) | 60 | 15.1 | 8.6 |

Thermal: Fynkel's run went 0 → 2, Bevy's 0 → 1, so heat worked against
Fynkel. `full` and the 0.75 runs were not taken (the phone auto-locked).

**Pictures.** Bevy's Sponza is far darker (floor median 7–9 of 255 vs
Fynkel's 24). Most of that is our Bevy setup, not Bevy: its ambient term at
roughness 1 is ≈ 0.45 × `brightness` × albedo (`pbr_ambient.wgsl`,
`EnvBRDFApprox` with `F_AB(1, NdotV).x` ≈ 0.45), so `2000 / π` gives about
half the intended 2 000 lux. A hand estimate for that light puts the floor
at ≈ 25–50, so Fynkel is about right. Brightness does not change GPU work;
the timings stand. The shots also differ in aspect (Bevy 16:9, Fynkel 4:3).

**Verdict.** The premise that rendering is Fynkel's bottleneck and Bevy
would be faster does not hold on the target phone: equal on the simple
scene, Fynkel ahead on the heavy one (one pair, Bevy setup not tuned).
Decision (the user, 2026-10-07): keep Fynkel's own wgpu renderer and hecs
world, and restart the framework code-first with no editor; the state
before that restart is tagged `fynkel-1-final`.
