# F5 gate report: asset readiness, residency and binding

**Date:** 2026-10-02. **Status:** complete. **Decision:** [ADR-0056](decisions/ADR-0056-asset-readiness-residency-and-bindings.md). This report qualifies the F5.1–F5.4 contract; it does not qualify production-scale rendering or all platforms.

## Delivered

| Item | Implementation and permanent coverage |
| --- | --- |
| F5.1 | Strict, cache-only dependency preparation; separate CPU/simulation/GPU readiness; atomic scene and prefab activation. Native missing/corrupt dependency tests prove sync and async failure attribution and preservation of the live world. Existing cancellation, replacement and activation-with-cache-removed tests pass. |
| F5.2 | One bounded priority worker per store, deduplicated typed polling, no disk/decode waits in normal renderer preparation, shared upload admission and explicit fallbacks. Regressions block the worker to prove draw does not decode, check byte/operation bounds, exercise queue capacity/priority/panic recovery and reject stale publication after clear. Loading/export readiness waits explicitly and refuses failure; editor capture preserves overlays and extra worlds across retries. |
| F5.3 | Separate decoded CPU, runtime-owned CPU and GPU asset budgets; all-or-nothing runtime reservation; ownership for scenes, individual entities, registered asset fields, prefabs, queued effects, fades and active sound. Tests cover pressure, pinned refusal, representation replacement, cache-store transfer, aborted edits, additive/persistent scenes, queued exactly-once sound, runtime clip retirement and repeated unload returning both cache and simulation registries to zero. Permanent native prefab caches are removed. |
| F5.4 | `AnimationBinding.key` unifies clip, skin, controller root/mask and IK targeting. Imported indices and authored keys replace name lookup. Duplicate display names, renaming, hierarchy edits, separate cloned instances, missing/ambiguous keys, rig remapping and source reordering are covered. Editor mask/IK selections persist keys while displaying names. Cache/payload versions reject old data; migration and recook instructions are in ADR-0056. |

Native simulation preparation happens before publication and is independent of a live world's mutation. `World::instantiate_prepared` uses the same transaction preflight as ordinary gameplay. Runtime budget admission reserves the entire edit before its entity/resource state commits. Store adoption validates ownership transfer before scene/editor publication. These are foundation changes, not per-component patches or a second command transport.

The source review additionally closed budget-accounting errors when switching material representations and replacing IBL environments, a stale prefab preparation publication window, bounded change-history invalidation, a queued-sound ownership gap, and idle-view readiness with mixed failed/pending assets, hidden layers and zero-weight volumes. CPU and GPU refusal stays observable instead of silently pretending readiness.

## Qualification

[Machine-readable evidence](../benchmarks/foundation/results/2026-10-02-22229b4f-f5-completion.json) records commands, results, log hashes, source hashes, measurements and failed attempts. Base revision: `22229b4f1967f4d07a8f0ed968ba64d7342648d5`; the record describes the F5 working-tree changes on that base rather than pretending the base already contains F5.

- Full release workspace: **1,157 tests passed**, 9 ignored in that run. The subsequent material-accounting and pending-renderer suite passes **27 tests** (overlapping the workspace, with one additional regression). A final readiness review also passes the renderer/Apple suite and adds coverage for mixed failures, pending assets and inactive inputs. Both manually enabled F5 measurement tests and independent dynamic-library identity qualification pass separately.
- Stripped builds: **50 core tests**, **26 native unit tests**. The unrestricted stripped integration suite is not this gate: some fixtures require enabled audio/physics and were recorded as a failed exploratory invocation.
- SDK: **608 tests**, plus analysis. Authoring: **16 tests**, plus analysis. Flutter viewport/host: **39 tests**, plus analysis. Editor: **589 tests**, one existing ignored, plus analysis; affected animation/capture checks also pass after the final changes.
- Workspace Clippy with warnings denied, Rust formatting, generated bindings check, runtime dependency isolation and the F4 synchronization benchmark pass. The latter still visits zero static/sleeping synchronization candidates with zero synchronization allocations, and two candidates for one sparse scene/physics change at 10,000 entities.
- Showcase (3 tests), reference game (10 tests, one optional hero test skipped) and code-only game (18 tests) pass their required validate → codegen check → analyze → tests → headless loop. The code-only headless run explicitly selects its authored level because the project has no startup scene.

Example qualification found a separate lifecycle bug: generated periodic callback participation also filtered `start()`, so event-only collectibles never initialized. The SDK now applies that optimization only to periodic dispatch. A regression is archived failing before the fix and passing after it; lifecycle still runs exactly once across disable/re-enable. [ADR-0053](decisions/ADR-0053-registered-gameplay-and-extension-contract.md#lifecycle-participation-correction-f5-qualification) records the correction. Example event helpers now accept `EntityRef`, and the reference game's expected resource count includes all prepared blend dependencies.

The exploratory `examples/code_only_try` loop found pre-existing old document versions and the retired `@Expose(kind: 'entity')` interface. No F5 changes remain in that sample; its broader SDK/document migration is explicitly assigned to F7.4 rather than weakening validation or claiming that sample passed. An ignored local reference-game `hero.prefab.json` also needed its existing document migration for validation; the standard migrator preserved the original, whose backup was moved outside the asset root so it would not become an imported asset. No local hero asset is added to this change.

## Measurements and limits

Apple M3 Max, macOS 26.5.1 arm64, Rust 1.93.0 release, Metal. Tests/builds shared the machine; timings are observations, not isolated hardware guarantees. Raw values are retained in the evidence record.

| Workload | Result |
| --- | --- |
| Cold engine-cache draw preparation, 12 loads of the six-entity Main fixture, source files removed | 72 samples: p50 **53.000 μs**, p95 **126.833 μs**, p99 **144.625 μs**; max **15.590 ms** first-use spike. OS file cache was uncontrolled. |
| Warm draw preparation alongside simulation | 2,880 samples: p50 **21.708 μs**, p95 **31.459 μs**, max **122.250 μs**; no asset uploads in settled warm frames. |
| Simulation during the render loading workload | 2,952 steps: p50 **2.834 μs**, p95 **6.250 μs**, max **84.250 μs**. |
| Upload admission in that workload | At most **2 operations/frame**, observed maximum **1,872 asset bytes/frame**; all 12 loads settled in six preparation frames. |
| Residency | CPU cache high-water **10,666 bytes**; GPU accounting high-water **4,513,460 bytes**. After every unload: CPU cache **0**, GPU **4,508,800 bytes** of fixed frame resources. |
| Async additive activation while Main runs | 24 fresh hosts; add five-entity Playground. Preparation wall p50 **1.622 ms** (includes 1 ms polling). Atomic activation p50 **56.291 μs**, p95 **79.917 μs**, max **95.958 μs**. CPU cache peak **22,499 bytes**, zero after unloading both scenes in every run. |

These small fixtures establish correctness, admission counters, cache-only loading and a bounded repeated-unload baseline. They do not establish large-game throughput. Resource accounting excludes external payload owners, allocator/container overhead, immutable cache indexes, core playback copies and GPU driver/pipeline memory. Fixed frame attachments have separate ownership. The 15.590 ms first-use spike demonstrates why byte/operation admission is not a hard millisecond bound.

The defaults are one worker/64 outstanding jobs, 512 MiB each for decoded cache, runtime-owned payloads and GPU assets, and 16 operations/32 MiB of asset uploads per prepare call. Running decode jobs and individual GPU operations are indivisible; time/cancellation bounds are cooperative. Scene activation remains capacity-bounded and atomic on the host thread, not time-sliced. Oversize/pinned budget failures are explicit. These limits are the selected F5 contract, not hidden deferred implementation.

Reproduce the measurements with:

```sh
FYNKEL_F5_BENCH_OUT=/tmp/f5-streaming.json cargo test --release --locked -p fynkel_render --test project asset_streaming_qualification -- --ignored --nocapture
FYNKEL_F5_ACTIVATION_OUT=/tmp/f5-activation.json cargo test --release --locked -p fynkel_native --test load asset_activation_qualification -- --ignored --nocapture
cargo run --release --locked -p fynkel_core --example f4_sync_bench
```

Imported source-index bindings survive display rename and separate instances. Arbitrary exporter node-table reorder requires a coherent recook and review/regeneration of external authored controller keys. F6 owns render extraction/visibility/lighting/resource lifetime scaling; F7 owns the remaining workflow and legacy-example migration; F8 owns production workloads and platform qualification.


## Follow-up implementation review — 2026-10-02

The review traced native preparation/publication, runtime reservations, cache admission and release, asynchronous request state, renderer readiness and error recovery, animation binding resolution/invalidation, and their regression tests. It found and corrected these issues in the implementation:

- **Discarded mesh accounting:** replacing a cached mesh with a runtime mesh removed the cooked payload without removing its CPU cache charge. Publication now retires that charge under the same store lock. A pinned-resource regression verifies that the remaining cooked budget can shrink to zero while the runtime mesh remains usable.
- **Readiness error lifetime:** an unrelated eviction cleared all renderer errors, including errors for still-required resources. Errors now retain asset identity and survive unrelated evictions and live edits. Readiness considers the current view's required dependencies; a failed resource retained by another owner does not prevent a view that no longer references it from becoming ready. The regression exercises failure, unrelated edit/unload, and recovery without manually resetting the renderer.
- **Absent asset source:** explicit texture/material/volume/environment references could silently fall back when no asset store was installed. These now produce attributed errors. A sky-only regression verifies that explicit export readiness refuses this case; the existing missing-mesh test now verifies separate mesh and material diagnostics.
- **Stale queue refusal:** submission happens outside the store lock, so a delayed rejection could remove or poison a newer request. Rejection now checks generation before publication. Eviction also advances the asset generation, invalidating work from a previous residency lifetime. A deterministic state-transition regression checks both busy and unavailable refusals and current-generation retry behaviour.

The mesh-accounting and unrelated-eviction regressions were observed failing before their fixes. The review also corrected the runtime animation channel comment to describe binding keys rather than names. These changes preserve the F5 architecture; they do not introduce another loading, mutation, or binding path.

Follow-up qualification, source hashes, failed attempts and repeated-load measurements are recorded in [the review evidence](../benchmarks/foundation/results/2026-10-02-22229b4f-f5-review.json). The initial broad run hit the existing document timing assertion during concurrent work; a later broad run hit the Apple presenter wait timing assertion (4,240 μs against its 4,000 μs upper bound). Neither threshold was changed. A broad run passed 1,162 tests before the final targeted edits; final-source asset/render suites passed 130 tests, and native/Apple suites passed 106 with serial test execution. Seven real editor scene/capture checks, workspace Clippy and formatting passed. Both manually enabled qualification workloads passed, with CPU cache usage returning to zero after all 12 render cycles and all 24 activation cycles. The final checks and limits are recorded in that evidence rather than replacing the original completion measurements.

F5 remains complete within the acceptance scope above. Its bounded serial preparation worker, indivisible upload/decode operations, explicit budget failures, and imported-index recook contract remain deliberate limits. Small-fixture qualification is not evidence of large-world renderer scalability; that is the F6 gate.
