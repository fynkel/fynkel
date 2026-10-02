# ADR-0054: ECS change tracking boundary

Status: Accepted (first F4 implementation slice)

## Problem

A cache cannot trust revisions while supported mutation routes bypass them. The ECS facade previously returned both `&hecs::World` and `&mut hecs::World`. Even the shared reference permitted mutable queries and component borrows. Scene activity and transform propagation, interpolation history, emitter state and audio state used those routes. Tracking only gameplay transactions would miss these changes.

## Decision

The Fynkel ECS owns storage mutation bookkeeping. Remove both raw world accessors. Internal engine queries remain behind the `internal` API implementation: shared queries require `QueryShared`, while bulk mutable queries require exclusive access to the Fynkel world. Structure changes use its validated entity/component operations. Scene traversal uses Fynkel identities throughout.

Expose opaque world-specific change stamps with three domains: entity membership, component presence, and possible component value writes. Spawn/despawn advance all three; component insertion/removal advances presence and values; replacement and write leases advance values. Reads, rejected missing/stale accesses, and queued-but-unapplied destruction do not advance stamps. Deferred destruction advances them when applied. Recycled slots and different worlds cannot alias earlier stamps. Counters use checked increments, never wrapping or saturating into a falsely unchanged stamp.

A successful `get_mut` starts a write lease even if the caller ultimately leaves the value unchanged. No settled stamp is returned while any such lease is outstanding. The last guard dropping makes the new stamp observable. A bulk query advances values once; its exclusive Rust borrow prevents observing a stamp while any yielded mutable reference remains usable. Empty or no-op write queries may conservatively invalidate values.

A stamp is an invalidation token, not a transaction or lock. A snapshot built with shared access must see equal, present stamps before and after its reads. A subsequent writer can invalidate it immediately. Direct native writes retain their existing semantics, including possible partial mutation on panic; stamps do not provide rollback. Gameplay transactions retain their separate atomicity contract.

The component trait still accepts arbitrary `Send + Sync` Rust types. Mutation through a component's own locks, atomics or external shared state is not observable as a storage write. Such service/execution state requires its own publication contract; it must not be cached as plain gameplay data using these stamps. Registered gameplay components remain pure data under ADR-0053.

## Scope and consequences

This establishes the mutation boundary, not the full F4 optimization. There are no compatibility raw-world getters. Read queries stay read-only by construction; mutable bulk iteration and individually guarded writes cover the migrated native callers. Guard bookkeeping uses a mutex because shared component access can cross threads. Its cost must be included in later performance qualification.

The counters are global, not per-component or per-entity deltas. They cannot yet make sparse component changes cheap, distinguish hierarchy/transform/name changes, or remove the host callback reconciliation and entity-table scans. F4 still needs domain-specific deltas, explicit subsystem ownership, cache consumers, dirty roots/membership, snapshot publication/lifetime work and measured performance gates. Do not mark those complete based on this slice.

Permanent tests cover membership/value distinctions, all facade write routes, no-op and rejected access, multiple outstanding guards, unwinding, cross-thread guards, slot reuse, world identity and counter exhaustion. Compile-fail tests enforce read-only queries and the bulk write lifetime boundary. Existing scene, physics and core suites qualify caller migration.
