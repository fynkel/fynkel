# ADR-0060: Stable world allocation across bridge transactions

Status: Accepted
Date: 2026-10-03

## Context

F8 dense-scene CPU profiling found substantial memory-copy cost around owned
native transaction sessions. `WorldSlot` and `GameplaySession` stored `World`
inline, so moving exclusive ownership through begin/finish/abort also moved the
world's subsystem storage. This cost repeated for ordinary callback transactions
and read-only lifecycle queries. It is distinct from copying game data for a
snapshot and from the asset-ownership scan fixed previously.

## Decision

The native host allocates its core world once in `Box<World>`. `WorldSlot` and
`GameplaySession` transfer that box at begin, successful commit, abort and failure.
The session retains exclusive ownership: the host has no mutable world access
until the session returns it. Prepared resource publication returns the same box.
Transaction journals, validation, publication, poisoning and rollback rules remain
unchanged. No raw pointer or Rust borrow survives a bridge exchange.

The Rust owned-session API accepts and returns `Box<World>` consistently. Initial
Rust callers box a world once; subsequent sessions reuse that allocation. There
is no alternate inline bridge path. Ordinary borrowed `World::edit_gameplay` and
the Dart gameplay API are unchanged. The opaque C world handle and its ABI are
unchanged. Allocation addresses are not public object/world identity.

## Verification and consequences

A regression checks the same allocation across repeated commits, explicit abort,
failed edits, final-validation panic, and refused entry on a poisoned world.
Existing session, input, prefab resource, native bridge and SDK checks retain
visibility and failure guarantees. The native host pays one allocation per world,
not one per callback. Standalone Rust worlds can still be stack-owned until an
owned bridge session is needed.

The [F8 measured comparison](../../benchmarks/foundation/results/2026-10-03-f8-stable-world.json)
records single-fixed-tick AOT results and a bounded dense iPhone capture. A lower
CPU cost does not establish sustained 60/120 Hz; physics, callback scheduling and
GPU screen-space work remain separate optimization targets.
