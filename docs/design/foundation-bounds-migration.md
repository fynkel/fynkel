# Conservative render bounds migration (F1.4)

**Date:** 2026-09-30  
**Decision:** [ADR-0049](../decisions/ADR-0049-conservative-render-bounds.md)  
**Scope:** Static affine bounds, current-pose skin bounds, camera/shadow culling and safe fallback for undeclared deformation.

## Implementation

`crates/fynkel_render/src/culling.rs` now bounds affine sphere stretch using an upper bound on the largest singular value. The old longest-column rule could reject visible geometry under shear. The replacement handles rotations, non-uniform scale, reflection and collapsed axes. Sphere unions use double-precision intermediate calculations and outward rounding; their centre interpolation no longer clamps tiny distances to an unrelated minimum.

The same module builds `SkinBounds` from the mesh vertices and weights during `upload_mesh`. Each positively influencing joint gets a sphere around its influenced bind-space vertices. `GpuMesh` owns that metadata and releases it with the resident mesh. No new asset schema, manual bound component or per-frame vertex copy is needed.

`prepare` evaluates those spheres with the current `ExtractMesh::skin` palette and the draw's complete world matrix. This includes local-pose interpolation, presentation changes and extra-world placement. It unions the transformed influence spheres before camera/shadow rejection and uses that same sphere for caster-fit tightening and object light selection. A skinned mesh without a palette follows the existing bind-pose rendering path and uses static bounds.

Invalid palette ranges, invalid weights, missing influence metadata and non-finite/non-affine matrices yield an unbounded result. Custom shader materials with a vertex hook also use that result because the engine has no declared displacement envelope. Such draws remain eligible for camera and shadow passes. If an unbounded caster participates, shadow cascades retain the ordinary camera-range fit instead of tightening around finite caster bounds. No infinite bound is passed into cascade fitting.

## Ownership and cost

The renderer owns both stages of the calculation:

| Stage | Work | Lifetime / allocation |
| --- | --- | --- |
| Mesh upload | Group bind vertices by positive joint influence; build spheres | Once per resident mesh, O(V × 4 × log J); O(J) retained metadata |
| Static draw preparation | Transform one sphere with a conservative affine stretch | O(1), no new bound allocation |
| Skinned draw preparation | Transform and union one sphere per influencing joint | O(J) before culling; no temporary influence arrays or CPU vertex skinning |
| Invalid/undeclared deformation | Keep draw/caster; skip caster-bound tightening | Observable conservative fallback |

`FrameStats.bounds_joints` counts evaluated influence spheres and `bounds_unbounded` counts draw candidates with no valid finite bound. These are Rust renderer statistics; no existing C/Dart statistics record or ABI layout was changed.

A manual release microbenchmark evaluates 64 influence spheres per rig for 1, 200 and 1,000 rigs, with 30 samples per population. It reports median and p95 CPU time. It excludes upload, animation evaluation, extraction, GPU skinning and the rest of draw preparation. It is an observation on this machine, not a portable frame-budget guarantee. The captured Apple M3 Max release run measured:

| Rigs × 64 joint bounds | Median CPU time | p95 CPU time |
| --- | --- | --- |
| 1 | 3.583 µs | 3.625 µs |
| 200 | 564.250 µs | 613.291 µs |
| 1,000 | 1,987.125 µs | 2,513.042 µs |

## Regression evidence

- A shear witness is inside the camera and light clip volumes while the old longest-column sphere is outside. The new bound retains it.
- Sampled sphere directions remain contained under shear, reflection and collapsed axes.
- Weighted skinned vertices remain inside the evaluated bound across changing joint poses and extra-world placement.
- Invalid weights/palettes and invalid bounds cannot justify spatial rejection; tiny sphere unions remain conservative.
- A GPU image test places a sheared cube at the orthographic camera edge. Culling on/off gives identical pixels and retains the visible sliver.
- A cooked rig's model is moved 100 metres off-screen while its palette moves vertices back into view. Camera and shadow culling retain both skinned draws; the images match with culling on/off. The same test covers extra-world placement and independent palette storage.
- A custom vertex shader brings an off-screen bind mesh back into view. Its unbounded fallback preserves the image. Existing custom-shader shadow movement tests remain in the renderer suite.

Qualification is run with `tools/foundation_baseline.py --only bounds`. It includes the 15-command foundation build/test/analysis/codegen gate and the manual release bounds benchmark. The [archived qualification capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-bounds.json) records all **16 commands passing** on the working tree against `87e3dec8`: **830 Rust tests** (7 ignored in the normal suite; the manual benchmark passed separately), **353 SDK**, **16 authoring**, **39 Flutter**, and **91 focused editor tests**. Workspace checking, strict Clippy, formatting, code generation and package analyses pass. Raw logs are in `target/foundation-baseline/bounds-migration`. The original identity, clock and transform probes remain passing.

## Limits and follow-up

The calculation assumes the engine's supported linear-blend skinning with nonnegative influences; malformed data fails open. Conservative spheres can retain more geometry than a tighter shape would. Extremely large coordinate magnitudes remain subject to float precision; outward margins are practical numerical protection, not arbitrary-precision geometry.

Unknown shader deformation can increase draw cost and prevent shadow-fit tightening. A future explicit deformation envelope could restore bounded culling for these materials. Existing camera masks, visibility flags and shadow range remain intentional visibility policies. Shadow filtering, normal transformation and arbitrary affine physics are separate concerns.

F1.1 identity-authority qualification and the broader F0/F1 release gates remain open. Completing this task does not certify the full editor/reference-game release or close the foundation milestone.
