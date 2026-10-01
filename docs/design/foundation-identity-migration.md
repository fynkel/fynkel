# Live identity migration (F1.1)

**Date:** 2026-09-30  
**Decision:** [ADR-0047](../decisions/ADR-0047-identity-clocks-frame-order-and-readiness.md)  
**Source:** `main` working tree against `87e3dec8afa33c2c439c6fe848a7f0599cc512aa`; not a released revision.  
**Progress ledger:** [foundation roadmap](../foundation-roadmap.md).

## Implemented contract

Every actual ECS world allocates its namespace from the engine's shared,
monotonic u64 authority. Rust core, native runtime worlds and authoring worlds
use this route. Callers no longer supply a world ID in WorldConfig,
FynkelWorldDesc or Dart World.create. Disposing a runtime/world never returns
its namespace to the allocator. Exhaustion fails rather than wrapping.

A live entity is `{world: u64, slot: u32, generation: u32}`. The C layout is
16 bytes, aligned to 8; generated wire codecs write these three fields in
little-endian order. Full namespace, slot and generation participate in lookup,
equality and map keys. Despawn increments generation; a slot at u32::MAX is
retired permanently. Checked world/spawn constructors return exhaustion errors;
the native world constructor exposes an output status and returns NULL on
failure. Convenience Rust constructors panic on exhaustion.

Dart uses EntityRef rather than an integer. JSON diagnostics, load/clone
reports, runtime object references and authoring live records use exactly 32
hexadecimal digits. The string is the u128 diagnostic value: generation,
slot, then world, each zero-padded. On native Dart, a u64 world with its high
bit set is represented by a signed int bit pattern; the wire/JSON codec
preserves all bits. Saved scenes and prefabs continue to use authored UUIDs.
No compatibility path accepts old integer entity references.

Uncached World.attach validates through native lookup and rejects foreign,
stale, temporary and null references. Generated component records, command
writers, subscriptions, query lists, contacts, physics hits, animation,
projection, scene reports, persistent objects, authoring previews, editor live
selection and Apple outlines carry the complete reference. Rapier user_data
keeps all 128 bits. Replay comparisons exclude the world namespace because
independent runs deliberately have different runtime identities; this does not
weaken runtime lookup validation.

## Breaking interfaces

This records the first identity migration at ABI 2. The subsequent
[clock migration](foundation-clock-migration.md) advances the current runtime
to ABI 3 and fingerprint `0x52ca2a9cfe616eff`.


| Boundary | New contract |
| --- | --- |
| Runtime ABI | 2; FynkelEntity is a struct, not uint64_t; world_create adds optional out_status |
| Wire schema fingerprint | `0xbd13fb6ff5603ff8`; generated Rust/Dart records and fixtures regenerated together |
| Authoring ABI | 5; live runtime-handle JSON uses hex strings |
| Apple viewport ABI | 12; outline arrays contain FynkelEntity structs |
| Dart gameplay | EntityRef replaces integer runtime handles; worldId configuration removed |

Rebuild CLI, native libraries and Dart bindings together. ABI/fingerprint
checks reject old binaries. The SDK lock and test project lock declare ABI 2.
The SDK lock removes ABI 1 prebuilt download entries: no ABI 2 binaries have
been published by this work. Source builds are required until new artifacts
are qualified. Rewriting a project lock retains artifacts only when runtime
version, ABI and schema fingerprint all match.

Example project locks and existing document migrations remain qualification
work. Updating a lock alone does not make the reference game's old prefab and
skeleton data valid. This migration preserves the user's generated asset-file
changes and does not regenerate or repair example content.

## Regression evidence

The actual native-world test creates separate runtimes, verifies a foreign
query and destroy are rejected without harming the destination entity, drops
the original runtime and recreates 300 worlds. The Dart tests exercise separate
actual runtimes, attach validation, destruction/slot reuse, 300 Play-style world
recreations, full-width wire/JSON/map round trips, and invalid old-number input.
Rust unit tests exercise allocator exhaustion and maximum-generation retirement
in actual ECS storage; counters are seeded near their limits rather than
allocating billions of objects. Native world creation reports successful status.

The migration also runs the broader Rust workspace, generated-contract checks,
Dart/Flutter analyses and SDK/authoring/Flutter/editor suites. Captures record
commands, hardware, toolchain and working-tree hashes. See the archived
[identity capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-identity.json)
and [prototype requalification](../../benchmarks/foundation/results/2026-09-30-87e3dec8-identity-prototypes.json).
Captured failures are retained and corrective verification is identified explicitly.

Final verification: **817 Rust workspace tests pass (6 ignored)**, with full
release workspace/all-target compilation, formatting, strict Clippy and codegen
checks passing. All analyses pass; SDK **344**, authoring **16**, Flutter **39**
and the focused editor identity/hierarchy tests **91** pass. Prototype bridge
and state qualification also pass. The complete editor suite has **586 passes,
1 skipped and 2 failures**: reference-game validation (old ABI lock plus
pre-existing prefab/skeleton data) and the 10,000-row hierarchy timing threshold.
The hierarchy test passes in the subsequent focused rerun without concurrent
build activity; that does not erase the failed timing observation. The
[followup capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-identity-followup.json)
records corrected fixtures, strict lint/generation and the focused editor run.
F0/F8 are not certified by these results.

## Cost and qualification

On an Apple M3 Max, the qualification release capture of the same
300-step headless workload gave these observations:

| Workload | Earlier frame p95 | Identity frame p95 | Earlier tracked native memory | Identity tracked native memory |
| --- | ---: | ---: | ---: | ---: |
| 1,001 entities | 444 us | 332 us | 708,817 B | 800,673 B |
| 10,001 entities | 3,049 us | 3,349 us | 8,176,529 B | 9,336,353 B |

At 10,001 entities, extraction p95 changed from 1,949 to 2,118 us. These are
single observational runs, not a controlled proof of improvement/regression
or a production budget. Different checksums are expected because the diagnostic
checksum encoding changed. The SDK bandwidth workload transfers about 62.7 KiB
in and 152.6 KiB out per tick for 1,000 subscribed entities. Wider references
increase storage/bridge traffic; F0/F8 still need repeated release/profile/AOT
captures and representative gameplay/render workloads.

## Remaining gates and next work

1. The subsequent [host authority migration](foundation-identity-authority.md)
   implements process-lifetime storage and explicit handoff across independently
   loaded/replaced libraries. It records loader and SDK qualification separately
   from the original single-library capture above.
2. Existing queued tick commands retain a transitional world-zero/generation-zero
   temporary target. Live lookup rejects it. F2 must replace this with separately
   tagged, transaction-scoped references and commit/abort expiry; the new identity
   representation does not implement gameplay transactions by itself.
3. [F1.2 clocks](foundation-clock-migration.md) are implemented and verified,
   including waits, invokes, tweens, pause, zero-tick frames and catch-up drops.
   [F1.3 affine transforms](foundation-transform-migration.md) and
   [F1.4 bounds](foundation-bounds-migration.md) are also implemented and verified.
   The F2 state/frame migration remains separate work.
4. Finish broader editor/example, generator, stripped runtime, host/AOT and
   performance qualification in F0/F7/F8. General component registry,
   readiness, resource ownership and rendering follow their roadmap gates.
