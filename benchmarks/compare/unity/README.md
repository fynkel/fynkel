# C4: Unity 6 URP (docs/engine-comparison.md)

Unity 6000.3.9f1, URP 17.3 (Forward+), glTFast 6.14.1.

- `Assets/Scripts/CmpBootstrap.cs` builds the whole scene in code.
- `Assets/Editor/CmpSetup.cs` sets the project up and builds it. It copies
  `../sponza` and `../character.glb` into `Assets/Models` (git-ignored), and
  glTFast imports them **at import time** (editor import, Mecanim clips). It
  then writes the URP asset with the SSAO feature, the volume profile, the
  player settings and `Assets/Scenes/Cmp.unity`.
- `Assets/Plugins/iOS/Thermal.mm` returns `NSProcessInfo.thermalState` and
  writes the `[cmp]` line to stdout.
- `build_ios.sh` exports the Xcode project, runs xcodebuild (Release, signed)
  and prints the `.app` path.

macOS sanity build:
`Unity -batchmode -nographics -projectPath . -buildTarget OSXUniversal -executeMethod CmpSetup.BuildMac -quit`
→ `Build/mac/CmpUnity.app`. On macOS the lines go to the player log, so
pass `-logFile <file>`.

## Environment

| Variable | Values |
|---|---|
| `CMP_LEVEL` | `plain`, `full` (default) or `msaa` |
| `CMP_SCALE` | URP renderScale, e.g. `1.0` or `0.75` |
| `CMP_SHOT=1` | shot mode: saves `cmp_shot_unity_<level>_<scale>.png` to `Application.persistentDataPath` (iOS: Documents) after 5 s |
| `CMP_NSLOG=1` | iOS: write lines with NSLog instead of stdout |

These are debug variables only, not for measured runs: `CMP_DEBUG=1` (raw
FrameTiming), `CMP_AO=r,i`, `CMP_AMBIENT_SPEC=0`, `CMP_NOSHADOW=1`,
`CMP_POINT_GAIN=x`, `CMP_CAM=x,y,z,dx,dy,dz`, `CMP_CLEAR=r,g,b`, and on macOS
`CMP_VSYNC=1` and `CMP_UNCAPPED=1`.

## Mappings and deviations

- **Axes.** glTFast negates X (glTF is right-handed), so every spec
  coordinate (x, y, z) is used as (−x, y, z). The picture is the same, not
  mirrored.
- **Exposure.** URP has no physical exposure, and its BRDF has no 1/π. The
  intensities are therefore `physical × 1/(1.2·2^12) / π`:
  - sun: 10 000 lux → 0.6476
  - point: 2 000 lm = 159.15 cd → 0.01031, range 6, colour (1, 0.8, 0.6)
    linear
  - ambient: 2 000 lux uniform → flat SH 0.1295 (linear)

  `lightsUseLinearIntensity` is on. ACES is URP's ACES.
- **Ambient specular.** URP's flat ambient lights the diffuse term only, and
  with no skybox the default reflection is black, so metals turn black. The
  glTF default material (the character's) is metallic = 1. Fynkel
  (`(diffuse + f0) · ambient / π`) and Bevy's AmbientLight also light the
  specular term. So the custom reflection is a uniform cube of the same
  0.1295. It is not an environment map. Turn it off with
  `CMP_AMBIENT_SPEC=0`.
- **Sky colour.** Fynkel writes (0.5, 0.6, 0.8) after tone mapping. URP
  clears the HDR buffer, which then goes through ACES. The linear clear
  colour (0.268, 0.419, 0.965) was measured to give sRGB (127, 153, 204).
- **Shadows.** All levels use 4 cascades and a 2048 atlas (1024 per
  cascade), as Fynkel does. Distance is 60 m, splits are 4.14 / 9.35 /
  20.35 m (Fynkel's λ = 0.75), and filtering is URP soft shadows (medium).
- **SSAO.** URP SSAO with depth+normals at full resolution, radius 0.6 m
  (Fynkel's default) and intensity 0.2. URP's estimator scales with the
  radius, so intensity 1 at 0.6 m turns the ambient fully black.
  `DirectLightingStrength` is 0, so AO darkens the ambient only, as in
  Fynkel.
- **Bloom.** URP Bloom with threshold 0.9 and scatter 0.7 (Fynkel copies
  these defaults), intensity 1, low-quality filtering. The spec gives no
  intensity.
- **CPU time.** `cpu_ms` = max(main-thread work, render-thread frame time).
  Main-thread work is timed in the player loop, from the start of
  EarlyUpdate to the end of PostLateUpdate. FrameTimingManager's
  `cpuMainThreadFrameTime` includes the targetFrameRate wait, so it is not
  used. `gpu_ms` / `gpu_p95` come from `FrameTiming.gpuFrameTime`.
- **Character time offset.** Character i plays `walk` at
  `(t + 0.1·i) mod length`, driven through a Playable graph (Mecanim). In
  shot mode every character is at 0.5 s.
- **IL2CPP Master.** On iOS the Xcode script maps Release to
  `il2cpp --configuration=Release`. The post-build step patches it to
  `ReleasePlus`, il2cpp's equivalent of Master.
- **macOS build uses Mono.** The Mac IL2CPP module is not installed. It is a
  sanity build only.
