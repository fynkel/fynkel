# ADR-0054: ECS change tracking boundary

Status: Accepted (F4 mutation tracking and conditional identity synchronization). The completed delta/publication contract is specified by [ADR-0055](ADR-0055-incremental-synchronization-and-publication.md); statements below about remaining F4 work describe the earlier slices.

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

Global counters are supplemented by component-specific stamps. There are still no per-entity deltas: a relevant change invalidates the whole domain. Unchanged callback identity reconciliation now skips its scan, but sparse renames/structural churn still rebuild full identity snapshots, and entity-table synchronization remains unchanged. F4 still needs domain-specific deltas, explicit subsystem ownership, cache consumers, dirty roots/membership, snapshot publication/lifetime work and measured performance gates. Do not mark those complete based on this slice.

Permanent tests cover membership/value distinctions, all facade write routes, no-op and rejected access, multiple outstanding guards, unwinding, cross-thread guards, slot reuse, world identity and counter exhaustion. Compile-fail tests enforce read-only queries and the bulk write lifetime boundary. Existing scene, physics and core suites qualify caller migration.

## Component stamps and identity synchronization (second F4 slice)

`component_stamp<T>` combines the world identity, component type, entity membership revision and that type's last possible write revision. Typed insert/remove/write guards advance the affected type. Bulk queries enumerate their declared borrow types through the hecs adapter and advance only unique (mutable) borrows. Optional mutable components participate; types used only as membership filters do not. The adapter remains inside `fynkel_ecs::internal`, with regression coverage for these query forms. Reads and unrelated component writes leave a settled component stamp unchanged. Any outstanding shared-access write guard still prevents obtaining a settled stamp, conservatively including unrelated guards.

Core owns a world-local identity/name cursor derived from the Name stamp. Zero is reserved for a consumer's first read. ECS spawn/despawn changes membership even for unnamed entities; direct Name replacement/removal, guarded writes and bulk writes all invalidate the cursor. Changes aborted by a gameplay transaction never reach these storage revisions.

The additive C ABI v5 function `fynkel_world_gameplay_state_since` takes that consumer's last cursor. A successful response begins with little-endian revision u64 and changed u32. An unchanged response ends there (12 bytes); a changed response appends the existing full identity/name snapshot. The existing full-snapshot API and conditional API use one implementation. SDKs using the new symbol require the corresponding runtime build; existing full-snapshot callers and wire formats remain compatible. C callers must keep each cursor with its originating world, use zero for an initial/forced read, and consume or copy the borrowed response before the next gameplay API call on that world.

Each SDK World owns its cursor independently of other native consumers. It advances only after successful reconciliation, which precedes the next gameplay edit; an abort of that later edit does not undo already observed native changes. Unchanged responses skip native entity traversal/name encoding and Dart map decoding/object reconciliation. Behaviour dirtiness notifications still run because activity and execution state have separate invalidation rules. Native state reads still reject active sessions, unsupported authoring worlds and poisoned worlds; they do not step simulation or drain events.

The SDK exposes successful snapshot and response-byte counters beside its existing bridge counters. The callback benchmark records those counters as well as latency. This qualifies the unchanged-state fast path only, not sparse identity deltas, full entity-table publication, rendering or whole-game performance.
