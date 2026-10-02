# ADR-0055: Incremental synchronization and owned publication

Status: Accepted (F4). Extends [ADR-0054](ADR-0054-ecs-change-tracking-boundary.md); preserves the F2 transaction and F3 registered-data contracts.

## Ownership

`World` composes owners and orchestrates their phases. `GameplayState` owns the single tracked ECS, registration registry, edit epochs and committed effects. `GameplayEdit` stages and validates against that authority; it never creates a second committed component store. The SDK's `_BehaviourSystem` owns callback participation, lifecycle and coroutine execution, not native component data.

`SceneLifecycle` owns loaded-scene identities and persistent roots. Its fields are private; every mutable lease advances its metadata revision, including operations that change no ECS component. `SceneSync` owns derived transform/activity cursors and takes tracked ECS storage as input. Physics and render membership own their separate derived indexes and cursors.

Native `ResourceServices` owns the asset store, residency tokens, prefab preparation cache and cancellable load tickets. Store adoption retires store-dependent preparation. Core receives the `CommandResourceSource` capability; it does not depend on the asset loader, editor, C ABI or Dart. Resource handles remain service state, not gameplay components. Existing cancellation and residency drop contracts remain in force.

`Publication` owns subscriptions, outbound events/diagnostics and the completed snapshot. Native transport buffers and SDK/editor consumer cursors are derived publications, never mutation authorities. Public native extensions use `GameplayEdit` or the tracked ECS facade; no raw hecs world accessor is restored. Direct Rust writes still require valid scene invariants: hierarchy changes use scene operations, not arbitrary edits to reciprocal Parent/Children links. Component interior mutability remains outside plain-data revision tracking, as specified in ADR-0054.

## Revisions and bounded deltas

ECS writes append world-local identity/type changes to a bounded 16,384-event history. Consumers retain independent cursors. Reads do not drain history. Spawn/despawn include exact generations; guarded writes record the affected entity/type; bulk mutable queries conservatively reset their declared mutable domains. Outstanding write guards prevent a settled change set. Aborted transactions do not publish ECS changes.

Consumers receive a reset when first observing a world, using a foreign/future cursor, or missing relevant history. The last revisions of entity membership, structure and component types allow an unchanged domain to remain unchanged even when unrelated writes overflow the history. Reset is the specified lossless resynchronization contract, not silent truncation. Memory stays bounded independently of disconnected consumers.

Scene propagation marks directly written local transforms dirty and visits affected roots/subtrees. Activity refresh skips descendants already covered by a changed ancestor. Physics updates affected membership/configuration and dependent nested colliders; newly created bodies have a pending first-writeback set, and subsequent writeback uses Rapier's active bodies. Explicit asset invalidation also enters this owner. Render extraction retains active spatial membership. Output record construction/interpolation still processes that retained population; F6 owns broader render extraction and visibility work.

Entity-table revisions track structural/type presence, names, hierarchy, layers/tags, activity, registered dynamic membership and scene metadata. Ordinary transform or registered value changes do not invalidate the table. Unchanged native table queries reuse encoded bytes, patching only the request ID. The opt-in authoring delta response sends changed rows and dead handles; it sends ordering only when hierarchy/membership changes. Metadata/schema changes reset that consumer. Dart keeps a private cursor and derived row/order cache and returns immutable lists; earlier results remain stable.

The additive ABI v5 `fynkel_world_gameplay_state_delta` returns cursor u64, mode u32 (0 unchanged, 1 reset, 2 delta), then for nonzero modes a row count and entity/alive/name rows. C cursors belong to their originating world; use zero to force initialization. The SDK reconciles only touched canonical objects on sparse updates, including destruction and name changes, while preserving behaviour notifications. Existing full/conditional state APIs remain read projections of the same authority. They are not alternate mutation transports. The new SDK requires a runtime exporting the new symbol.

## Publication and thread handoff

Rust completed snapshots are `Arc<[SnapshotRecord]>`. A publication is built before replacement. `World::published_snapshot` returns a tick plus immutable owned allocation; readers may retain it across later ticks or World destruction and send that allocation to another thread. Empty unchanged publications reuse their allocation. Render extracts are separately owned values.

C output buffers remain borrowed read-only views with the header's lifetime contract: a mutating call, same-kind query reuse, panic/disposal or scene replacement can invalidate them. Raw pointers cannot be made safe after free; C callers must copy before retention. The Dart bridge now copies every exposed wire publication into unmodifiable owned bytes and decodes lazily from that owned memory. It retains no native pointer. Buffer growth, subsequent queries, scene replacement, poisoning and disposal cannot invalidate a previously returned SDK buffer. Copy counters include these output copies.

A live world is still driven sequentially. Do not concurrently call into it, transfer active edits, share mutable component borrows, or hand native borrowed views to another thread. An ownership handoff must exclude all outstanding view users and transactions. Only owned immutable publications may be consumed independently. This defines a future handoff contract; it does not introduce parallel simulation or rendering.

## Allocation choices

Registered bulk application borrows its validated native batch token instead of cloning the owned population. Each entity's staged registered writes use a compact sorted map, preserving deterministic order, replacement and all-or-nothing validation while avoiding an oversized tree node for a common one-component edit. Owned rows and transaction lifetime checks remain. Wire ownership intentionally adds copies for safety; dense queries and render extraction still allocate. The F4 gate report records both the savings and remaining costs; no zero-allocation frame or 60 Hz large-game budget is claimed.
