# F6 rendering foundation gate

Date: 2026-10-02. Result: **F6.1–F6.4 complete for the recorded qualification scope.**

The implementation is in the working tree on base `22229b4f1967f4d07a8f0ed968ba64d7342648d5`, together with the preceding uncommitted F5 work. This is not a claim that a release has been committed or that F8 platform/product qualification is complete. [ADR-0057](decisions/ADR-0057-render-extraction-visibility-and-resource-lifetimes.md) records the decisions. [Machine-readable evidence](../benchmarks/foundation/results/2026-10-02-22229b4f-f6-completion.json) contains source hashes, commands, raw samples, log hashes and retry history.

## What changed

| Item | Implementation and correctness evidence |
| --- | --- |
| F6.1 | Maintained render-component membership; `extract_into` reuses owned output and transform memo capacity. Both hosts reuse the snapshot buffer. The allocating convenience API delegates to the same implementation. Tests retain previous snapshots across world mutation and exercise hierarchy/skin dependencies, irrelevant spatial entities and buffer capacity. |
| F6.2 | Persistent conservative sphere BVH; sparse/dense refitting and measured-cost rebuild, independent camera/shadow queries, fully-contained subtree acceptance, and reuse of current posed bounds. Unknown assets/displacement remain visible so visibility cannot suppress loading. Spatial tests compare against linear queries through movement/removal; existing animated skin, shear, extra-world and off-camera-shadow pixel tests pass. |
| F6.3 | Growable GPU storage light pool, full-width instance indices, spatial candidate selection, stable ranking and masks. Default 1,024 frame lights; configurable maximum 16,384 and eight per object. Invalid settings leave previous settings intact. Tests cover camera masks before admission, ties, overflow, and actual GPU pixels referencing light 300. CPU/WGSL layout agreement is checked through shader reflection. |
| F6.4 | Documented pass/resource lifetimes, unchanged-size no-op, complete renderer recreation, Apple device-loss result, standalone device/surface recovery, and Flutter recovery with retained settings and a retry UI. Tests destroy a real device, recreate its viewport, cycle sizes/quality and compare pixels/memory, and verify unregister-before-destroy ordering and failed-unregister retry ownership. |

Readiness now describes the **prepared frame**, not just the cache at the end of preparation. A controlled worker-completion regression proves that a later GPU cache hit cannot hide an earlier fallback in the same frame. This prevents incomplete captures and missed idle redraws.

## Qualification

Apple M3 Max, Metal, macOS 26.5.1, arm64; Rust 1.93.0. Native checks use release builds and the locked dependency graph. Native libraries and CLI were rebuilt before the final Dart/Flutter runs.

| Check | Result |
| --- | --- |
| Entire Rust workspace, serial test threads | 1,171 passed, 0 failed, 12 ignored manual tests |
| Workspace Clippy, all targets, warnings denied | Passed |
| Rust formatting, generated bindings and runtime dependency isolation | Passed |
| Flutter viewport package analysis and complete tests | No issues; 42 passed |
| Editor analysis and complete tests | No issues; 589 passed, one existing skipped test |
| Dart SDK complete tests | 608 passed |
| Explicit extraction, renderer and animated-bounds qualification | Three manual tests run and passed separately |

Successful final logs supersede recorded failures; assertions were not weakened. The first Flutter run used an older CLI from PATH and failed to cook current documents. The final run explicitly selected the rebuilt CLI. Earlier compile/lint mistakes and the intermittent fallback-frame pixel failure are retained in retry history; the latter was fixed with the deterministic regression described above.

## Measurements

These are observational medians, not promised budgets. Raw samples include startup spikes. Extraction has 60 samples per case, with 10,000 entities and 1,000 render owners. It measures extraction separately from simulation/authoring work: static **139 μs**, one changed transform **133 μs**, all 10,000 transforms changed **1,429 μs**.

Rendering has 40 frames per case. Cubes use MSAA 1, shadows off and ambient occlusion off. Mostly-offscreen cases render at 64×64; all-visible cases render at 1280×720. The headless harness waits for readback; these are stage measurements, not end-to-end presentation throughput. GPU timestamps arrive after their CPU frame; the GPU column uses available positive timestamp samples, with raw zero/unavailable samples preserved in the evidence.

| 10,000-record case | Prepare median μs | Encode median μs | GPU median μs | Drawn / culled | Estimated resident GPU bytes |
| --- | ---: | ---: | ---: | --- | ---: |
| Static | 218 | 1 | 89 | 81 / 9,919 | 216,196 |
| One transform changed | 222 | 1 | 89 | 81 / 9,919 | 216,196 |
| All transforms changed | 422 | 1 | 28 | 81 / 9,919 | 216,196 |
| 256 local lights | 626 | 1 | 36 | 81 / 9,919 | 216,196 |
| All visible, no local lights | 832 | 1 | 165 | 10,000 / 0 | 20,197,060 |
| All visible, 1,024 local lights | 4,832 | 2 | 613 | 10,000 / 0 | 20,197,060 |

The first cold preparation took **27.979 ms**, retained rather than removed from the data. Small sub-microsecond encode measurements are limited by integer microsecond resolution. GPU resource bytes estimate renderer-owned resources, including light/list capacities; they are not driver allocation totals or process RSS.

The mostly-offscreen case performs 309 visibility node tests; the fully-contained case performs 15. The 1,024-light case admits all lights with zero frame drops, tests **153,602** spatial candidates instead of a potential **10,240,000** object/light pairs, and reports 588 objects exceeding the eight-contribution limit. The 256-light case reports 81 such objects. Overflow is explicit supported behavior, not concealed loss.

The separate conservative animated-bounds microbenchmark uses 64 joints per rig: medians **4.542 μs** for one rig, **359.042 μs** for 200, and **1,806.667 μs** for 1,000. Full animated rendering correctness is covered by the pixel suite; this microbenchmark isolates bound construction only.

## Deliberate boundaries

- The supported path is spatial forward lighting with eight selected local contributions per object. It does not promise arbitrary overlapping lights, local-light shadow maps, or clustered shading.
- These cube workloads justify hierarchical frustum culling and spatial light queries. They do not establish an occlusion or high-poly LOD bottleneck. Occlusion and automatic LOD have explicit ADR decisions rather than stub implementations.
- The fixed pass chain remains the execution authority. There is no parallel render-graph representation or new mutable render thread. Hosts submit the prior prepared frame before preparing another; existing GPU submission/presentation overlap is retained.
- Device recreation obtains fresh GPU resources and preserves logical renderer/controller settings. Failure to obtain a replacement device is visible; standalone treats it as fatal and Flutter retains a failed viewport with a retry action. Physical iOS and second-machine validation remain F8 obligations.
- This is a completed rendering-foundation milestone, not evidence that arbitrary production scenes meet these timings. F7 unifies the ordinary gameplay/editor workflow; F8 performs broader product qualification.

## Follow-up code review (2026-10-02)

The review inspected extraction membership/dependencies, conservative visibility and refitting, storage light indices/budgets/masks, resource invalidation/recreation, native device-loss handling, and Flutter presenter lifetimes against F6.1–F6.4. No additional concrete defect was found in the Rust extraction/visibility/lighting implementation. The original performance evidence remains applicable because those sources were unchanged.

The Flutter lifecycle boundary did need correction:

- Detach could complete before delayed texture registration, allowing a late attachment after closure/disposal. Teardown now waits for registration and rebind operations before unregistering/freeing; disposed controllers cannot recreate.
- Synchronous detached-state listeners could start creation before the teardown guard was published. The guard now exists before any teardown notification.
- Rebind failure could escape asynchronously and leave the controller marked attached. It now enters a recoverable failure state, retains the requested size for retry, and cannot render against a stale presenter binding. Registration returning no texture id is also an explicit failure.

Six permanent tests cover delayed registration, disposal while registering, delayed rebind, rebind failure/recovery with preserved size, reentrant listeners, and missing texture ids. The delayed-registration test was first run against the old implementation and failed, then passed after the ownership fix. Existing failed-unregister ownership tests remain passing. These fixes establish ordering and failure semantics; they do not add timing delays or retry loops to hide races.

Review validation: 477 native tests passed (six manual tests ignored), 48 Flutter package tests passed, and 589 editor tests passed (one existing skip). Flutter/editor analysis found no issues. Final affected scene session/view tests were rerun after the last controller edits. [Review evidence](../benchmarks/foundation/results/2026-10-02-22229b4f-f6-review.json) records command scope, logs and updated source hashes; the original completion artifact remains historical. F6 remains complete within its recorded scope, with the deliberate boundaries above unchanged.
