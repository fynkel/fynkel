# M1 gate report and go/no-go

**Date:** 2026-09-19 · **Milestone:** M1 (blueprint §19 "M1 — Runtime foundation, `hecs` wrapper, and bridge") · **Verdict: GO — proceed to M2.**

M1 asked whether the runtime foundation holds up under the contracts the
blueprint fixed: a `hecs`-backed world that Dart drives through one batched
bridge, hosts sharing one core, and a Dart per-frame cost that is measured
rather than assumed. Everything below is reproducible with
`python3 benchmarks/m0/capture.py` (profile `mac-m3max-2026`; the iOS numbers
come from a profile-mode run on the device named in `hosts.md`). The latest
capture is `benchmarks/m0/results/2026-09-19-44c8c15.md`.

## 1. Gate criteria

| Gate | Result | Evidence |
| --- | --- | --- |
| Dart can create, inspect, update, and destroy objects through Fynkel APIs | **Pass.** `World.spawn` → `GameObject` with `localTransform`, `worldTransform`, `parent`, `children`, `setLocalTransform`, `setPresentationTransform`, `reparent`, `destroy`; reads follow writes through the staged-write overlay until the next tick resolves them. | `packages/fynkel/test/sdk_test.dart` ("spawn resolves on the next tick; reads follow writes"), `docs/design/dart-sdk.md` |
| Stale handles fail; entity reuse does not alias stale Dart handles | **Pass.** Handles are `(index, generation)`; a despawned slot is reused with a bumped generation, so a stale handle misses in Rust and a rejected command surfaces as `FynkelError` in debug (ADR-0014). | `crates/fynkel_ecs/tests/world.rs::stale_handles_fail_and_slot_reuse_does_not_alias`, `churn_and_mapping_integrity`; `sdk_test.dart` ("rejected command clears the overlay …") |
| Batched updates in hot paths; no per-property FFI | **Pass.** All mutations are wire records in one `CommandBatch` per lane per frame; `submitBytes` hands one buffer over one FFI call. Views (snapshot, diagnostics, events, after-physics, query) are read through a per-family cached `WireBuffer` that reparses in place. Bridge stats count calls and bytes: 1000 entities updated per tick = 1 submit + 1 step + 1 read; 191 µs encode+submit, 169 µs step, 47 µs read, 54.8 KiB in / 86.2 KiB out. | `packages/fynkel/lib/src/wire/buffers.dart`, `lib/src/native/bridge.dart`, `test/bandwidth_test.dart`, capture "Bridge @ 1000 entities" |
| Snapshot subscriptions are bounded/measurable | **Pass.** Subscriptions are explicit (`subscribe`/`unsubscribe` records), counted in `BridgeStats.subscriptions`, dropped when their entity is destroyed, and the snapshot view holds exactly the subscribed set (bytes reported by the stats). | `crates/fynkel_core/tests/contract.rs` ("subscriptions of destroyed entities drop"), `sdk_test.dart` ("bridge stats count calls and bytes"), `bandwidth_test.dart` |
| Steady-state Dart allocation per frame on the M1 test scene is zero or documented | **Documented, non-zero: 5.1 KB/frame on the 25-entity scene (JIT).** See §2. The SDK's own fixed share is ≈0.7–1.2 KB/frame; the remainder is the scene's immutable math values (`Transform`/`Vec3`/`Quat`, ≈3.6 KB) which are gameplay code, not bridge cost. GC over 3000 frames: 2 events, 1.6 ms total. | `packages/fynkel/tool/alloc_bench.dart`, capture "Dart allocation" |
| World teardown is leak-safe; transform hierarchy passes cycle/reparent tests | **Pass.** Counting global allocator over 20 SDK-shaped lifetimes (runtime + world, 500 children, subscribe, 30 ticks, query, destroy half, teardown): bytes in use after == baseline (20651 B), 7634 allocations/cycle, no growth. Apple viewport: 60 create/render/resize/render/destroy lifetimes, RSS +128 KB total (< 4 MiB bound; the ~37 KB/cycle Metal autorelease leak found by this test is fixed with explicit pools). Hierarchy: reparent keep-world, cycle rejection, subtree destroy. | `crates/fynkel_native/tests/leak.rs`, `platform/apple/tests/lifetime.rs`, `crates/fynkel_ecs/tests/world.rs::teardown_drops_every_component`, `crates/fynkel_core/tests/contract.rs::hierarchy_commands_reparent_and_reject_cycles`, `sdk_test.dart` ("hierarchy: …") |
| `hecs` is not visible in public APIs/files | **Pass.** `hecs` is confined to `fynkel_ecs::internal`; a contract test asserts no public item mentions `hecs` and no other runtime crate depends on it (ADR-0003). | `crates/fynkel_ecs/tests/contract.rs::hecs_hidden_behind_internal_module` |
| The cube renders on an iOS device | **Pass.** iPhone 16 Pro Max (A18 Pro), same `platform/apple` interop as macOS: 120 Hz, present lag 0, no dropped frames; simulate p50 0.06 ms, `render()` 0.41 ms, GPU 1.84 ms. Simulator: 60 Hz, no drops. | `docs/hosts.md` (iOS section), commits `8250902`, `44c8c15` |

No hard-kill criterion was hit. The "do not" list held: no custom ECS, no
parallel scheduler, no gameplay isolate or Rust sim thread, no mutable native
memory exposed to Dart (every view is a copy into a Dart-owned buffer).

## 2. Dart allocation per frame (blueprint line "Dart allocations per frame and GC pauses")

Measured with `dart run tool/alloc_bench.dart --frames 3000 --json` in
`packages/fynkel`: a mirror of the Flutter host's demo scene (hub + 24
satellites, one `setLocalTransform` per satellite per frame, snapshot
subscription on all 25, events drained), 1200 warm-up frames, then heap-usage
deltas per 50-frame chunk after a forced GC, minus the measured overhead of a
zero-frame chunk, with chunks that contained a GC event excluded.

| | Before M1-10 | After |
| --- | --- | --- |
| heap bytes / frame (25 entities) | 10 720 | **5 105** |
| frame time (JIT, no renderer) | — | 23.7 µs |
| GC events / 3000 frames | — | 2 (one old-space mark start, one mark-sweep finalize), 1.59 ms total |

Where the remaining 5.1 KB goes (per frame, from the allocation profile's
class counters, which under-count JIT-inlined allocations and so are
indicative only):

- ≈3.6 KB gameplay math: 25 `Transform`, 48 `Vec3`, 27 `Quat` — the scene
  builds a new immutable transform per satellite per frame. This is the API's
  value-type contract (ADR-0013), and a scene that mutates fewer objects
  allocates proportionally less. Not bridge cost.
- ≈0.7–1.2 KB SDK: one `FramePlan`/`StepResult`/`SubmitResult` per frame, the
  snapshot iterator, and ≈12 B per subscribed entity for the snapshot index
  refresh. The command batch, its backing store, and every view buffer are
  reused across frames (`CommandBatch.backingStore`, `WireBuffer.reparse`).
- The rest is service-protocol and JIT noise (`_OneByteString`, `Instructions`,
  `ICData`) that appears only under `dart run` with the VM service attached.

What was changed in M1-10 to get there: per-family cached `WireBuffer` views
re-parsed in place; `CommandBatch` writing into a persistent backing store
with `setTransformOf` avoiding the intermediate record object; capacity-keeping
target lists and an in-place snapshot index in `World`; `drainEvents()`
returning `const []` when empty; `MissedDeadline` demoted to an informational
diagnostic so a debug host never allocates a throwable for it.

**Known gap:** these numbers are JIT (`dart run`) numbers. The AOT (Flutter
release) profile is expected to be equal or lower, but is not captured by the
benchmark script yet; it belongs to the M3 host profiling work together with
the per-frame instrumentation in the Flutter host.

## 3. Measured numbers

From `benchmarks/m0/results/2026-09-19-44c8c15.md` (120 Hz, macOS, profile
`mac-m3max-2026`):

| Metric | Standalone | Flutter Path A |
| --- | --- | --- |
| frames per second | 121 | 120 |
| presented interval p50 / p95 | 8.33 / 8.77 ms | 8.33 / 8.34 ms |
| engine CPU encode+submit p50 / p95 | 0.08 / 0.13 ms | 0.04 / 0.08 ms |
| submit → GPU complete p50 / p95 | — | 0.76 / 0.81 ms |
| present lag (frames) | 0 | 0 |
| Flutter raster p50 / p95 | — | 0.86 / 0.94 ms |
| dropped / stale presents | — | 0 / 0 |

Headless host (`fynkel test --headless --steps 3000`): 25 entities, step
p50/p95/max 2/2/28 µs, deterministic checksum `b4597e697bfaa3dd`, 0 missed
deadlines. Bridge at 1000 subscribed entities: 191 µs encode+submit, 169 µs
step, 47 µs read per tick.

Compared with the M0 report the Path A numbers are unchanged within noise;
M1 replaced the M0 throwaway core with `fynkel_core` + `fynkel_render` under
the same interop without moving the composition cost.

## 4. What M1 delivered

- `fynkel_ecs` (`hecs` wrapper with generational handles, deferred despawn,
  teardown test), `fynkel_core` (scheduler phases, accumulator + interpolation
  alpha, command lanes with staged-write overlay, hierarchy, subscribed
  snapshots, pull-only events, diagnostics), `fynkel_render` (instanced
  extract renderer, `device_limits`).
- `fynkel_native` ABI v1 (`fynkel.h`, poisoning on panic, status codes) and
  `package:fynkel` with `@Native` bindings delivered by a native-assets build
  hook (ADR-0015): source checkout → cargo, else prebuilt from
  `fynkel.lock.json`.
- Schema codegen (ADR-0012): Rust structs → JSON Schema, Dart records, Rust
  codecs, fixtures; `fynkel codegen --check` in CI.
- Hosts sharing the core (`docs/hosts.md`): headless, standalone,
  Flutter macOS + iOS via `platform/apple`.
- Instrumentation: `BridgeStats` (calls, bytes, subscriptions),
  `tool/alloc_bench.dart`, `tests/leak.rs`, `tests/lifetime.rs`, all folded
  into `benchmarks/m0/capture.py`.

## 5. Known gaps carried into M2

- Allocation numbers are JIT-only (§2); AOT capture is deferred to M3 host
  profiling.
- The allocation profile's per-class breakdown is indicative (JIT-inlined
  allocations are not counted; VM-service traffic pollutes it). The
  heap-delta figure is the one the gate uses.
- Benchmarks run on one macOS profile; the iOS numbers are a single
  profile-mode session, not a scripted capture.
- A second display / DPI change is still not exercised live (unchanged from
  M0).
- The `fynkel.lock.json` prebuilt artifacts have no published sha256 yet; the
  hook's prebuilt path is exercised only by unit tests until the first
  release is cut.

## 6. Decision

**GO.** M2 (project documents, assets, prefabs, materials, behaviours) starts
on the M1 runtime as is. The M2 backlog inherits the gaps above as explicit
items rather than blocking on them.
