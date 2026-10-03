# ADR-0046: Replace the gameplay state boundary with shared transactions

**Status:** Accepted and implemented. F2–F7 migration is qualified; F8 performs production qualification with explicit platform and performance limits. See the F2 gate and ADR-0051.
**Date:** 2026-09-30  
**Authority:** User instruction to extend mixed-state prototypes, compare strategies and finalize decisions from the evidence.  
**Related:** ADR-0042 through ADR-0045, ADR-0003/0004/0008/0012/0036; F0.3/F0.4, F1–F4, F7.  
**Evidence:** [Mixed-state report](../design/foundation-state-comparison.md), [capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-state.json).

**Current implementation (2026-10-02):** F2 is complete under ADR-0051. The dated migration/batch notes below preserve intermediate states and their evidence; their pending-work statements are superseded by the final gate report.

## Context

The shipping core applies commands independently: a valid name edit survives a later rejected reparent. A Dart custom-data sidecar can make its own records atomic but cannot give mixed built-in/custom edits that guarantee. Adding convenience wrappers alone leaves multiple state/mutation routes.

The mixed prototype places Name, Transform, hierarchy, activity, Health and DoorState in the existing core ECS. Object helpers, population queries and metadata field edits read one native transaction view. Two candidates pass the same supported-subset contracts: a full reconstructed preflight adapter and a change-journal validator sharing the native state boundary. Native frame/allocation measurements favor the shared journal at 1k/10k populations; the diagnostic JSON bridge does not demonstrate an end-to-end speed advantage at 128 entities and is unsuitable for shipping bulk access.

## Decision

Build a **focused replacement of the gameplay state, mutation and Dart bridge boundary**. Retain the existing ECS facade, scene geometry, native core and compatible subsystems. Migrate incrementally behind the replacement boundary; do not build a complete new engine or perpetuate conflicting overlay/read paths. A production adapter may forward into the replacement contract, but must not own competing mutable component truth.

Adopt the following contract for pure entity/component edits:

1. One callback transaction presents a coherent logical view. Direct object reads, queries, name lookup, hierarchy-derived activity and world matrices include its staged edits. Subsequent sequential callbacks see its committed result.
2. Validate the complete intended edit before publishing it. Failed validation marks the transaction failed, even if the caller catches that validation error. No earlier data edit from that callback survives rejection. Previously committed callbacks are not rolled back with it. Ordinary script execution state and external side effects are outside data rollback.
3. Successful callback commits publish atomically without running a simulation tick. Outside a running frame, explicit edits use the same boundary; paused editing does not implicitly advance time. Scope accessors expire on commit or abort. Native implementation panics poison the world; a poisoned world cannot expose a successful partial result.
4. Query candidate membership freezes when the query opens. Values read through the transaction. Destroyed/removed candidates not yet visited are skipped; new members appear in the next query. Structural commands are staged, not applied to borrowed storage during traversal.
5. Spawned references are explicitly temporary until commit, usable in that transaction's lookup, hierarchy and component edits. Commit resolves them to live references; abort retires them. Destroy retires the subtree in the logical view. Native runtime handles are not persistent authored IDs.
6. Compose exact matrices for world-space geometry from the same staged local transforms and hierarchy. Approximate TRS decomposition, keep-world reparenting under shear, singular inverses and physics transform restrictions require the F1.3 contract; they are not silently approximated here.
7. Each component type has one authoritative owner. Built-ins and native-registered gameplay data use native storage, with no mutable Dart echo store. Numeric/bool custom records have demonstrated native ownership with ordinary Dart logic. Prefer generated, scoped bulk access for supported data. The general custom registration/storage default, additional field types and package delivery still require ADR-0044/F3 qualification. Behaviour execution and noncomponent services can remain Dart-owned.
8. Object helpers, systems, host edits and inspector edits use the same registered validation/mutation contract. Schema generation supplies codecs and field metadata; hand-written prototype switches are not the production extension design. Preserve unsupported authored content or reject with actionable diagnostics; do not silently erase it.

Keep execution sequential with explicit dependency edges and stable tie-breaking initially. Later committed writers win in that schedule; a read/write declaration grants access, not automatic ordering. Authoring transactions, simulation-result snapshots and published render data retain their distinct ownership and explicit revisions/ticks.

## Implementation scope and deferred decisions

[ADR-0047](ADR-0047-identity-clocks-frame-order-and-readiness.md) now resolves the identity, clock, frame/presentation and readiness design questions in this table. Their production integration remains at the listed destinations. Other deferred questions remain open.

| Question | Required destination |
| --- | --- |
| Engine world namespace, handle layout, retirement/exhaustion and cross-runtime safety | F1.1 / ADR-0043 |
| Real/scaled/simulation clocks, dropped time and consumers | F1.2 / ADR-0043 |
| Exact conversion, shear/keep-world and physical transform restrictions | F1.3–F1.4 |
| Physics/body commands, animation/audio effects, resource requests and failure atomicity | F2.2; distinguish pure data from effect execution before migrating these families |
| Fixed/after-physics/update commits, pause/Step, input visibility and render-only adjustments | F2.3 / ADR-0047; frame placement is chosen, actual subsystem integration pending |
| Custom type registry, supported layouts, references, migrations, native package delivery and Dart-only types | F3.1/F3.2/F3.5 / ADR-0044; measure AOT, allocation and full-frame crossing cost before declaring the default available |
| Lifecycle/error continuation, disabled systems and parallel safety | F3.3/F3.4; sequential implementation first |
| Incremental indexes, revisions, change publication and safe borrowed native views | F4; no O(changes) performance claim from this prototype |
| Asset readiness and renderer/resource lifetimes | ADR-0045, F5/F6 |
| Actual inspector, generator, authoring workflows, ABI/schema bumps and migration | F7; preserve existing user content |

ADR-0047 accepts the identity/clock/frame/readiness contracts; ADR-0044's general registry/layout/delivery details remain proposed. This record settles pure-data/state-ownership direction, not shipping integration of those contracts. Existing accepted ADRs continue to describe the shipping implementation until migrated; ADR-0004/0008/0036 replacement paths must obey this contract.

## Alternatives and consequences

Retain independent commands and custom sidecars: low initial work, but ordinary edits continue to disagree on atomicity and freshness. Reconstruct supported state for every preflight: correct in the prototype subset, but expensive for sparse changes and unable to clone arbitrary subsystem state. Add a specialized incremental prevalidator to the old path: viable in principle, but once it shares validation, state visibility and registry ownership, it becomes the chosen replacement boundary rather than a separate truth layer. Its best possible implementation has not been benchmarked, so the full-copy adapter is not evidence that every incremental design is slow.

The shared boundary adds transaction/registry/view machinery. It must hide that machinery behind simple object APIs and generated access. The current journal still clones records and maps, and existing propagation/extraction scans remain; production efficiency is unproven. Generic per-record JSON RPC is rejected as the production bulk transport. The earlier batched typed-view experiment justifies testing generated binary views and revision/delta publication next.

## Verification and completion

The first F2.1 production-core increment adds `World::edit_gameplay` and scoped
`GameplayEdit` access for existing-entity Name, Transform, hierarchy and activity.
The journal holds edited fields over committed ECS storage. Validation errors
remain sticky, callback failure discards all edits, and publication refreshes
derived hierarchy/activity without a tick. A native publication panic poisons
the world. Final parent changes publish once; temporary detach operations do
not leak events. Completed snapshots retain their recorded simulation state.

The [native qualification capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-f2-native-gameplay.json)
records six passing gate commands: 847 workspace tests (8 ignored), including
ten new transaction regressions and a compile-fail lifetime check, plus 14
stripped-core tests; workspace check, formatting, strict Clippy and codegen pass.
This is working-tree evidence against `87e3dec8`, not a completed F2 gate.
At that first capture, pending spawns and subtree destruction were still open;
the structural follow-up below implements them. Remaining components/effects,
wire access and SDK callback routing remain open. Legacy queued commands currently prevent
entry into this API. Between-tick edits use the existing render-history snap
behavior; callback phase and explicit presentation integration remain F2.3.
Lookup and propagation costs have not been qualified as incremental or scalable.

The structural follow-up adds a distinct `GameplayEntity::Pending(PendingEntity)`
reference tagged internally with the world namespace, a checked monotonically
increasing transaction sequence, and a pending index. Tokens cannot be used in
another edit or world, including after commit/abort; identity exhaustion fails
permanently instead of wrapping. Live references keep the F1 handle contract.

`edit_gameplay` now returns `EditCommit<R>`: `value` is the callback result and
`resolve(pending)` supplies the live handle for a surviving spawn. A spawn
canceled before commit has no mapping and consumes no live slot. New roots are
allocated before existing data is changed; slot-exhaustion failure removes
unpublished allocations. Creation mappings are not encoded as legacy temporary
IDs or synthetic legacy Spawned events. Binary bridge integration must carry
this explicit distinction when it replaces the old path.

`destroy` retires the subtree visible in the current journal. Descendants moved
out earlier survive; live or pending descendants moved in earlier are retired.
Reads/edits of retired objects fail the transaction, while `contains` reports
false. Publication detaches changed/retired edges before removing entities, so
old physical links cannot delete survivors or emit duplicate destruction events.
Existing destruction cleanup removes subscriptions and presentation overrides.
Completed snapshots remain unchanged until the next simulation publication.

`query` captures membership when opened. Its cursor reads through the edit,
skips retired candidates, and excludes spawns made after opening. The next
query sees the new membership. Query cursors expire on commit/abort and cannot
cross worlds. `entities` is explicitly a materialized membership list, not an
automatically filtered traversal cursor.

The [structural qualification capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-f2-structural-gameplay.json)
records six passing commands: **860 workspace tests** (8 ignored) and **25
stripped-core tests**, including eleven new structural integration regressions
and two identity-exhaustion unit tests. Existing transaction regressions,
lifetime compile-fail validation, strict Clippy, formatting, workspace checks
and generated-code checks pass. This remains native-only working-tree evidence;
F2.1 and the milestone stay active until component and SDK migration completes.

The component follow-up extends the same journal to built-in configuration
without object references. Catalogue-generated reads, presence checks, insertion,
and removal keep the storage mapping in one place. Writes reuse existing value
validators and preserve their error messages; Transform goes through the same
validated field as `set_transform`. Name and hierarchy retain their dedicated
helpers. Transform, Name, and Hierarchy cannot be removed. Missing removals,
unknown IDs, and unsupported families fail the entire edit, even when caught.
Returned component values are owned copies; mutating a copy cannot alter storage.

`query_with` intersects component requirements against the current journal and
freezes membership. Replacing a value keeps membership and exposes its latest
value. Removing a required component invalidates that candidate; re-adding it
requires a new query. This is enforced with transaction-local membership revisions,
including for pending objects. A query opened later sees additions and re-additions.

Configuration data remains usable with optional modules disabled, matching the
existing storage contract. Publication does not run physics, audio, animation, or
catalogue systems. Their effects reconcile at the existing phases. AudioSource
removal shares runtime bookkeeping with the legacy path and clears AudioRun only
on committed removal. Removing and re-adding within one edit publishes a replacement
without intermediate cleanup; adding then removing publishes no component.

At that component increment, `Follow`, `ThirdPersonCamera`, and `Skin` remained
unsupported by the new boundary pending scoped reference remapping. The following
reference increment removes that limitation.

The [component qualification capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-f2-component-gameplay.json)
records all six native gate commands passing: **868 workspace tests** (8 ignored)
and **33 stripped-core tests**. Eight component integration tests exercise catalogue
round trips on live/pending objects, validation rollback across structural edits,
transform helper agreement, protected/unknown/unsupported access, query membership,
owned reads/canceled spawns, audio cleanup, and next-tick consumption. F2.1 remains
active. SDK/Flutter/editor checks were not rerun for this additive native increment;
no bridge migration, AOT, scaling, or complete F2 qualification is claimed.

The reference follow-up completes native catalogue coverage. `Follow<R>`,
`ThirdPersonCamera<R>`, and `Skin<R>` share their scalar fields and validators;
`R` defaults to the existing `ObjectRef` for storage, loading, and native APIs.
Their mapping methods preserve all configuration while changing reference type.
The schema derive resolves direct generic reference fields to their default type
and only implements the schema for that default instantiation. The generated-code
check pins the existing schemas; a compile-fail test prevents a scoped Follow from
being mistaken for a wire/storage schema. No ABI or wire fingerprint changes.

`GameplayComponentValue` is generated from the same built-in catalogue. Component
reads always return this scoped value; `set_component` accepts it or converts a
native `ComponentValue` through the same path. Reference fields use
`GameplayObjectRef::Stored(ObjectRef)` or `Pending(PendingEntity)`. Stored values
retain resolved authoring IDs, including across read/modify/write. A pending field
never masquerades as a live handle. These are representations at the transaction
boundary, not separate committed component stores.

Newly written references must name a live entity in this world, a pending entity
in this edit, or None. Unresolved authored UUIDs are rejected: scene loading owns
authoring-ID resolution. Foreign/stale live references and expired/foreign pending
tokens fail stickily. Scalar validation still uses the original validators.

Before allocation/publication, all surviving staged component references are
checked again. Destroying a target after writing its reference aborts the entire
edit unless that field is cleared/retargeted, the component is removed, or the
owner is also destroyed. All pending owners/targets are allocated before reference
mapping, so forward, self, and cyclic references work. No unresolved pending field
can enter native storage. Existing *unchanged* stored references remain weak when
a target is destroyed: reads retain the old handle, `contains` reports false, and
simulation consumers ignore it. Rewriting such a value requires clearing or fixing
the stale reference. Target deletion does not silently rewrite other components.

Example native transaction:

```rust
let commit = world.edit_gameplay(|edit| {
    let target = edit.spawn("target", Transform::default(), None)?;
    let follower = edit.spawn("follower", Transform::default(), None)?;
    let config = Follow::default().map_object_refs(|_| target.into());
    edit.set_component(follower, GameplayComponentValue::Follow(config))?;
    Ok(follower)
})?;
let follower = commit.resolve(commit.value).unwrap();
```

The [reference qualification capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-f2-reference-gameplay.json)
records six passing commands: **880 workspace tests** (8 ignored), **44 stripped-core
tests**, strict Clippy, formatting, workspace checks, and generated-code checks.
Eleven reference integration regressions and the schema compile-fail test cover
reference conversion/validation/publication; the catalogue round-trip test now
includes all three reference families. Native Follow simulation consumes a resolved
pending target at the next tick. F2.1 remains active for binary SDK integration;
callback routing, effects, and presentation still need their planned migration.
SDK/Flutter/editor, AOT, and scaling qualification were not rerun or claimed here.

The bridge follow-up separates the journal from its borrowed accessor and adds
`GameplaySession`, which owns the World while a host performs multiple synchronous
exchanges. Both this session and the original Rust callback use the same journal,
validation and commit implementation. The native world slot exposes committed
read-only state during an edit but cannot supply mutable World access. No borrowed
Rust accessor or pointer to host input survives an FFI call.

The [binary protocol](../design/foundation-gameplay-wire.md) provides begin,
exchange and end, explicit versioned framing, tagged live/pending references,
catalogue-generated component bodies, and commit resolutions. Normal guarded
native mutations/queries reject during a session and fail it stickily. Authoring
preview/stage worlds explicitly reject session begin until their owner is integrated.
Native destruction drops uncommitted sessions. Malformed requests are fully parsed
before mutation. The Dart client copies replies and always closes the session;
callback errors, swallowed operation failures and async callbacks abort it.

`NativeWorld.editGameplay` and `NativeGameplayComponent` are internal migration
plumbing. They are not a second state implementation, and they are not yet the
ordinary World/GameObject API. That API's overlays, automatic behaviour/host
callback routing, effect/lifecycle bookkeeping, and cache retirement remain open.
The request/response implementation allocates and has no bulk-performance claim.
Its scalar-record-plus-reference-list representation must be hidden by the future
ordinary accessors. Committed snapshots retain their old tick throughout an edit.

The [bridge qualification capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-f2-gameplay-bridge.json)
records all nine commands passing: **887 workspace tests** (8 ignored), **44 stripped-core
tests**, **4 stripped-native protocol tests**, and **362 SDK tests**, plus strict
Clippy, Dart analysis, formatting and generated-code checks. Seven Dart regressions
use the actual FFI library; native tests reject every truncated spawn request,
trailing bytes, unsupported protocol versions, invalid outputs, stale sessions,
and interleaved legacy mutations. Owned-session regressions cover sequential
access, abort, callback panic and ownership recovery from final-validation panic.
The SDK gate explicitly uses the release CLI built by this suite, avoiding stale
cooker/runtime schema mismatches. Flutter/editor suites and performance qualification
were not rerun for this internal bridge increment. F2.1 and F2 remain active.

The next SDK-adapter increment generates `ComponentCodec.mapObjectReferences`
from the component catalogue. Internal per-call bridge resolvers convert ordinary
`GameObject?` fields to tagged pending/live references and back. Generated mapping
preserves every scalar field and can clear a reference explicitly; it does not
use `copyWith(null)`, ambient codec hooks, or a hand-maintained list of component
families. Resolver failures are sticky transaction failures, and ambiguous raw
reference plus resolver inputs are rejected. This keeps the scalar wire layout
and schema fingerprint unchanged.

Seven new real-FFI regressions cover all three reference families, pending and
committed identity, null clearing, string-tail/scalar preservation, reference-free
components, caught encode/decode failures, conflicting inputs, unconverted objects,
and expired tokens returned by a resolver. The ordinary World/GameObject API is
still not routed through the journal. That integration must reconcile pending
handle publication/rollback and invalidate cached names/components/hierarchy and
transform reads without rewriting completed snapshots. Automatic callbacks,
effects and lifecycle remain open; this increment does not close F2.1.

The [reference-codec qualification capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-f2-gameplay-reference-codecs.json)
records nine passing checks: **887 workspace tests** (8 ignored), **44 stripped-core
tests**, **4 stripped-native tests**, and **369 SDK tests**, with generated-code
verification, formatting, strict Clippy and Dart analysis. The first analysis
attempt found an unnecessary test import; after its removal the full SDK analysis
and tests passed again. Flutter/editor and performance gates were not rerun.

The ordinary SDK adapter now provides explicit synchronous `World.edit`:

```dart
final hero = world.edit(() {
  final hero = world.spawn(name: 'hero');
  final camera = world.spawn(name: 'camera');
  camera.set(ThirdPersonCamera(target: hero));
  hero.transform.localPosition = const Vec3(2, 0, 0);
  // These are the same handles and logical values before publication.
  assert(world.find('hero') == hero);
  assert(camera.get<ThirdPersonCamera>()!.target == hero);
  return hero;
});
// The surviving spawn has a real EntityRef now, without an extra tick.
final entity = hero.entity;
```

The [Dart adapter](../../packages/fynkel/lib/src/gameplay.dart) contains identity
maps and commit metadata, not component or hierarchy storage. Ordinary built-in
component reads/writes/removal, local transform and affine reads, names, parent and
child access, activity, find queries, spawn and destruction route to the same
native journal. Reference conversion uses the generated mapping from the preceding
increment. Name uses the journal's dedicated name operations. Component queries
are materialized lists; later queries see later changes. Ordinary weak object
fields read null when their stored target is no longer alive, preserving the
existing SDK convention; unchanged native stored references are not rewritten.

Before commit the adapter prepares surviving membership and names. After the
native commit it resolves pending handles, retires destroyed handles, clears
existing component/pose/hierarchy caches and marks behaviour lifecycle work for
the next safe boundary. Canceled or aborted spawns never receive a live handle.
Newly discovered *existing* entities retain their canonical SDK handle on abort,
with their name read lazily from committed state. No user callback or native query
runs during post-commit reconciliation. Destroyed objects are removed from pending
transform subscriptions so they cannot reject the surviving objects' subscription
batch. Destroy/enable/disable callbacks run once at the next normal lifecycle
boundary, not inside the edit. Rollback schedules no lifecycle work.

A commit invalidates snapshot-backed ordinary gameplay reads until the next fixed
tick; the completed snapshot buffer and its tick are unchanged. Partial legacy
transform writes after an edit also seed from the newly committed local value.
The adapter currently enumerates surviving native entities and invalidates all
attached handles, including for a read-only edit. This is correctness-first
migration work: no sparse-update, bulk-FFI or large-world performance claim is made.
Revision-based invalidation and transport cost remain F4/qualification work.

This increment requires a completed frame boundary, no unresolved legacy spawns,
and empty simulation/presentation queues. It does not flush old commands to make
an edit succeed. Nested edits, cross-world object access and pending `.entity`
access fail stickily. Configuration errors caught inside the callback still abort
publication. Async callbacks are rejected; a retained Dart Zone scope prevents
later continuations from escaping into legacy writes once the edit expires.
Frame stepping, scene/resource operations, behaviour attachment/mutation,
coroutine/tween scheduling, playback/physics effects, explicit presentation writes
and the public native escape getter reject inside the scope before queuing work.
Arbitrary user Dart fields, services and separately retained low-level native
handles are outside this rollback contract. Raw advanced native access remains
subject to its own restrictions; this is not a general sandbox for callbacks.

Automatic host/behaviour callback wrapping, transactional behaviour attachment,
effect policy, presentation separation and retirement of legacy overlays are still
open. The explicit edit path does not make the existing outside-edit legacy
queues transactional, and F2.1/F2 remain active.

The [ordinary SDK adapter qualification](../../benchmarks/foundation/results/2026-09-30-87e3dec8-f2-gameplay-sdk-adapter.json)
records **15 passing commands**: **887 workspace tests** (8 ignored), **44 stripped-core
tests**, **4 stripped-native tests**, **384 SDK tests**, **16 authoring tests**,
**39 Flutter tests**, and **91 focused editor Play/document/scene/hierarchy tests**.
The suite includes strict Rust/Dart/Flutter analysis, formatting and generated-code
checks. Fifteen new adapter regressions exercise the real native journal through
ordinary APIs. AOT, scaling and the complete editor suite were not rerun.

The behaviour-attachment follow-up adds `GameObject.addBehaviour` and
`addBehaviourFromData` to the explicit edit scope. Script instances still belong
to the Dart behaviour scheduler; native built-in component data still belongs to
the ECS journal. The adapter stages attachment intent and callback order only,
using identity-keyed maps so user equality/hash overrides cannot change ownership.
It does not mirror or roll back arbitrary user script fields or constructor side
effects.

```dart
final actor = world.edit(() {
  final actor = world.spawn(name: 'actor');
  final script = actor.addBehaviour(Player());
  // script.gameObject, behaviour<T>(), getComponent<T>(), child searches and
  // world behaviour searches all see this staged attachment.
  script.set(const Layer(layer: 1));
  return actor;
});
// No script callback has run yet. Creation/enable/start follow the ordinary
// lifecycle at the next frame boundary.
```

A staged script resolves its object through the synchronous edit scope, without
changing its committed `_gameObject` or `_system` binding. Abort leaves the
instance unattached and reusable. Attachments whose owners are canceled or
destroyed in the edit are omitted from publication and receive no lifecycle
callbacks. Duplicate/foreign ownership, factory exceptions and order-getter
exceptions fail the edit stickily, including when callers catch them.

Before native publication, the adapter prepares the scheduler list and stable
callback ordering. User `order` getters execute before commit; publication itself
performs no user getter, constructor or callback. Final native-reference validation
can still abort both the data edit and all attachments. Surviving scripts are
bound and subscribed only after successful native commit. `onCreate`, `onEnable`
and `start` run at the next normal lifecycle boundary, and inactive owners defer
creation until activation. Staged object-level queries preserve attachment order;
world-level behaviour queries use callback order with stable attachment ties.
Existing dying scripts stay in the scheduler until the normal destruction
callbacks have been delivered.

The [behaviour attachment regressions](../../packages/fynkel/test/gameplay_behaviour_test.dart)
cover ordinary lookups, native writes through staged scripts, callback timing,
rollback and reuse, final native validation failure, duplicates/foreign ownership,
canceled owners, stable ordering, throwing getters, inactive owners, subscriptions,
registered factories, async expiry, guarded unported operations and queries after
an earlier committed destruction. Behaviour enable/destroy edits, ObjectSpec and
prefab construction, effects/coroutines and automatic callback routing remain open.
F2.1/F2.2/F2 are not complete.

The [attachment qualification capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-f2-gameplay-behaviour-attachment.json)
records all **15 checks passing**: **887 workspace tests** (8 ignored), **44
stripped-core tests**, **4 stripped-native tests**, **395 SDK tests**, **16
authoring tests**, **39 Flutter tests**, and **91 focused editor tests**, plus
strict analysis, formatting and generated-code verification. Eleven new attachment
regressions use the real native library. The direct preliminary test run selected
an older cooker for two existing scene-load tests; the full qualification uses
the matching release CLI and passes. AOT, scaling and full editor qualification
are not claimed.

The lifecycle follow-up now includes `Behaviour.enabled` and `Behaviour.destroy`
in explicit `World.edit` scopes, for both existing and newly attached scripts.
Identity-keyed enabled/removal intent is staged beside attachment intent and is
published only after the native journal succeeds. No script field or scheduler
flag is changed on abort, including when final native-reference validation fails.

| API | During an edit / after its commit | Lifecycle boundary |
| --- | --- | --- |
| `enabled` | Reads the last staged requested value; successful commit publishes it. | The scheduler compares the final request with applied state. |
| `isAlive` and gameplay lookups | A transactionally removed script is immediately absent/dead and remains absent after commit. | Cleanup does not make it visible again. |
| `isActive` | Continues to report applied callback state. | Changes when enable/disable callbacks are applied. |
| `isDestroyed` | Continues to report completed destruction, not a staged request. | Becomes true when destruction cleanup runs. |
| `gameObject` | The binding remains available for cleanup of an existing script. | Callbacks can still identify the owner. |

Repeated enabled writes coalesce; disable followed by enable back to the applied
state produces no intermediate callbacks. Repeated destruction is idempotent.
Trying to change enabled after removal fails stickily instead of resurrecting the
script. Removing a script and attaching a different replacement in one edit makes
only the replacement visible through ordinary object, component, child and world
lookups. Removing an existing script preserves any earlier enabled request in the
same edit for cleanup to read. A script attached and removed in the same edit is
never published, receives no callbacks, and retains its original fields for reuse.

Committed logical retirement is recorded separately from completed callback
cleanup. The scheduler retains the private binding until its existing safe-boundary
pass delivers `onDisable` (when active) and `onDestroy` (when created), once. Public
lookups filter the logically retired script during that interval. Callback errors
after commit are reported by the existing behaviour error handler; they do not
roll back committed native state or prevent the remaining destruction cleanup.

The [lifecycle regressions](../../packages/fynkel/test/gameplay_lifecycle_test.dart)
cover requested/applied state, coalescing, immediate lookup consistency, exactly-once
cleanup, native-validation rollback, initially disabled attachments, canceled
attachment reuse, replacement, writes after removal, final requested state during
cleanup, cross-world edits, expired async continuations and callback exceptions.

Outside explicit edits, the old `Behaviour.destroy()` request still has its
existing deferred lookup semantics until automatic callback routing is migrated.
Transactional removal remains absent even when read outside the edit after commit.
This compatibility boundary is temporary and does not close the F2 visibility gate.
ObjectSpec/prefab construction, effects/coroutines, automatic callback routing,
presentation policy and legacy-overlay retirement remain open. Arbitrary user
script fields and external side effects remain outside rollback.

The [lifecycle qualification capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-f2-gameplay-lifecycle.json)
records all **15 checks passing**: **887 workspace tests** (8 ignored), **44
stripped-core tests**, **4 stripped-native tests**, **406 SDK tests**, **16
authoring tests**, **39 Flutter tests**, and **91 focused editor tests**. Strict
analysis, Clippy, formatting and generated-code verification pass. Eleven new
lifecycle regressions run against the real native library. F2 remains active;
AOT, scaling and full editor qualification are not claimed.

The ObjectSpec follow-up routes ordinary `World.build` trees and
`createPrimitive` through the same native journal and attachment intent. It
allocates all tree objects before writing components, so catalogue-generated
reference mapping resolves forward `ObjectSpec.object` component targets without
writing placeholder bindings early. A scoped identity map supplies `spec.object`
during the edit; successful publication updates the committed binding, while
abort preserves the prior build (or original unbuilt placeholder).

Each spec identity is allowed once per edit, including across separate build
calls. Cyclic trees and shared/duplicate nodes fail stickily before construction.
Repeated builds in separate edits remain supported. A committed build whose root
is subsequently destroyed in the same edit binds to the resulting dead handle,
consistent with canceled transactional spawns. Behaviour attachment, activity,
components and hierarchy use their existing transactional paths; construction
introduces no native opcode and executes no lifecycle callback before commit.

This increment deliberately does not invoke the legacy cooked-prefab loader or
rewrite registered script fields holding spec placeholders. Both are rejected
stickily within explicit edits. The loader mutates native scene/resource state
outside the journal; script setters are arbitrary user code and cannot safely run
after native publication or be assumed reversible. Ordinary direct references to
live objects or scripts retain the existing construction contract. The next
construction work must define these two publication boundaries before enabling
them. Outside explicit edits, existing construction behavior is unchanged.

[Seven construction regressions](../../packages/fynkel/test/gameplay_spec_test.dart)
cover forward component references, hierarchy, deferred creation, binding rollback
and script reuse, native reference validation failure, cycles/duplicates, guarded
prefab/script-placeholder paths, primitives and inactive trees. F2 remains active.

The [construction qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-gameplay-spec.json)
records all **15 checks passing**: **887 workspace tests** (8 ignored), **44
stripped-core tests**, **4 stripped-native tests**, **413 SDK tests**, **16
authoring tests**, **39 Flutter tests**, and **91 focused editor tests**, plus
strict analysis, Clippy, formatting and generated-code verification.

The native prefab prerequisite adds `GameplayEdit::instantiate` for an already
prepared `SceneDescription`. It reuses the existing loader's structural validator,
then stages hierarchy, root placement, activity and components through the ordinary
journal methods. `GameplayPrefab` returns authoring-id/pending-handle pairs in
hierarchy order; only `EditCommit` can resolve surviving handles to live identity.
The operation allocates all logical objects before mapping component references,
so forward, cyclic and self references use the same pending identity mechanism.
Multiple instances map the same authored ids independently.

Authored references must resolve inside the instance. A missing authored id fails
the edit even if its stored runtime handle happens to be live. References without
an authored id use ordinary world/transaction validation. Invalid structure,
components, references, parent or placement fail stickily and discard earlier
writes as well as the instance. This is intentionally stricter than the legacy
loader's partial component acceptance and missing-reference warnings: the new
transactional construction API promises all-or-nothing configuration. Destroying
an entire staged instance cancels it; removing only a referenced child still goes
through final reference validation.

[Nine native prefab regressions](../../crates/fynkel_core/tests/gameplay_prefab.rs)
cover hierarchy/placement/activity, pending parents, all reference families,
independent copies, invalid structure/components, missing ids, rollback, canceled
instances, external runtime references, foreign/expired parents and invalid poses.
The qualification runner includes this suite in the stripped-core check too.

This prerequisite performs no asset loading, subsystem registration, residency
change, behaviour construction or lifecycle dispatch. The C/Dart cooked-prefab
entry point remains guarded inside explicit edits. The existing loader currently
registers physics, nested prefabs, animation and audio and retains asset residency
before instantiating; that preparation/publication boundary is the next native
integration task. Registered script-field placeholder rewriting also remains
guarded because arbitrary Dart setters are not a rollback mechanism. No new wire
opcode, completed SDK prefab support or F2 completion is claimed.

The [native prefab qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-gameplay-prefab-core.json)
records all **15 checks passing**: **896 workspace tests** (8 ignored), **53
stripped-core tests**, **4 stripped-native tests**, **413 SDK tests**, **16
authoring tests**, **39 Flutter tests**, and **91 focused editor tests**, plus
strict analysis, Clippy, formatting and generated-code verification.

The cooked-resource prerequisite introduces `Assets::prepare_prefab`. It decodes
physics meshes/materials, animation clips/blends/controllers/skins, sounds and
nested emitter prefab descriptions into a private owned resource plan, without a
mutable world. A residency lease is acquired before decoding; failure or dropping
the plan releases that lease, preserving any independent existing leases. Store
caches can warm during preparation; this does not promise to undo cache statistics.
Renderer/GPU resource upload remains outside this CPU subsystem resource plan.

`PreparedPrefab::commit` preflights audio clip capacity, runs a callback through
ordinary `World::edit_gameplay`, and publishes resources only after successful
native data commit. The callback receives the prepared description and can
instantiate it under pending parents or compose additional edits. The exclusive
world borrow prevents resource capacity from changing between preflight and
publication. Publication performs no file reads, decoding or user callbacks.
Validated physics meshes publish directly; material/controller validation has
already succeeded and sound-slot capacity has been checked. The returned
`PublishedPrefab` carries the description and residency lease for the owner to
retain. Resource-only `publish` is available for preparation ahead of an edit;
it must not be used to pretend that an independently fallible edit is atomic.

Resources intentionally remain registered after a successful resource publication,
including when an edit did not retain prefab entities. World registrations retain their
own decoded data until explicit removal or world disposal. Dropping the residency lease
evicts unretained store payloads; it does not unregister world subsystem resources.
This policy needs the later F4 resource-lifetime work rather than implicit entity
counting in the edit journal. Repeated publication reuses existing clip registrations;
audio slot assignment uses sorted asset keys, not randomized map iteration.

`PreparedMesh` also unifies collision input validation between full and stripped
physics. Both reject incomplete triangle indices, out-of-range indices and
non-finite vertices before touching the mesh registry. Previously the full adapter
ignored trailing indices while the stripped adapter checked their bounds; neither
path supplied the new preparation/publication contract.

[Eight cooked-prefab resource tests](../../crates/fynkel_assets/tests/prepared_prefab.rs)
cover untouched worlds on prepare/drop, independent residency leases, publication
without cache files, late decode failure, audio-capacity rejection before any
publication, repeated registration, combined native commit, native rollback and
imported skin resources. [Two mesh tests](../../crates/fynkel_physics/tests/prepared_mesh.rs)
cover deferred publication and malformed input; both suites are also included in
stripped qualification. The legacy loader remains unchanged. The owned C/Dart edit
session must still adopt this resource plan and authored behaviour construction;
SDK cooked-prefab and script-field-placeholder guards remain in place. F2 is active.

The [cooked-resource qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-prefab-resources.json)
records all **17 checks passing**: **906 workspace tests** (8 ignored), **53
stripped-core tests**, **4 stripped-native tests**, **8 stripped prefab-resource
tests**, **2 stripped mesh tests**, **413 SDK tests**, **16 authoring tests**,
**39 Flutter tests**, and **91 focused editor tests**, plus strict analysis,
Clippy, formatting and generated-code verification.

The cooked-prefab bridge follow-up adds gameplay operation 17. It prepares CPU
resources in `fynkel_assets`, stages the description through the existing native
journal and returns pending identities for every authored object plus behaviour
metadata. Both `World.instantiate` and `ObjectSpec.prefab` now use this route
inside explicit edits, including under pending parents. It is an additive internal
protocol operation, not a new ABI symbol or component wire layout.

Owned sessions keep prepared plans until end. `PreparedPrefab::finish_session`
preflights the union of sound keys across the entire session, rather than checking
each request separately or counting shared sounds twice. Capacity or journal
failure returns the restored world and drops all preparation leases. Successful
commit publishes validated resource data and transfers leases to native world
ownership. Unexpected resource-publication panic restores world ownership and
poisons it for the bridge crash contract. Successfully registered resources stay
world-owned even when the edit cancels all instances; resource lifetime and
incremental resource-cache optimization remain F4 work.

The SDK registers all returned pending handles before constructing authored
scripts. Fresh scripts receive scalar fields, then instance-local object/script/
native-component references through their registered setters before commit. Their
bindings use the existing attachment journal and callbacks run at the next normal
boundary. Unknown types, invalid fields, setter failures and unresolved references
abort stickily. A registered factory returning a bound or already staged script
is rejected before its type/fields can overwrite that existing instance. Arbitrary
user constructor/setter side effects remain outside rollback; these are fresh
script instances, not automatic rollback of arbitrary pre-existing Dart fields.

`lastInstantiate` remains unchanged on abort and updates after successful commit,
with resolved surviving handles and attached-script counts. A fully canceled last
instance reports a nil root and zero surviving attachments. Native diagnostics
and preparation-time residency statistics are retained in the report; diagnostic
callbacks are not dispatched inside the edit. The explicit-edit path rejects
invalid authored data rather than reproducing the legacy partial-load warnings.
Outside explicit edits, legacy loading behavior remains unchanged.

[Nine SDK regressions](../../packages/fynkel/test/gameplay_prefab_test.dart) exercise
real cooked assets and the native bridge: pending parents, authored values and
references, deferred callbacks, abort/report preservation, ObjectSpec prefab nodes,
independent instances, setter failures, factory reuse, canceled instances, final
native-reference validation and guarded shared data-asset fields. Two additional
[resource-session regressions](../../crates/fynkel_assets/tests/prepared_prefab.rs)
cover union capacity and capacity rejection before entity/resource publication;
a native protocol regression covers missing-project sticky failure.

Shared data-asset behaviour fields and pre-existing ObjectSpec script placeholders
remain guarded. Automatic callback routing, effects/presentation policy and
legacy-overlay retirement remain open. This increment does not complete F2.

The [cooked-prefab bridge qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-prefab-bridge.json)
records all **17 checks passing**: **909 workspace tests** (8 ignored), **53
stripped-core tests**, **5 stripped-native tests**, **10 stripped prefab-resource
tests**, **2 stripped mesh tests**, **422 SDK tests**, **16 authoring tests**,
**39 Flutter tests**, and **91 focused editor tests**, plus strict analysis,
Clippy, formatting and generated-code verification.

The shared-data follow-up adds gameplay operation 18 for read-only cooked document
access while the native world is owned by the edit session. It checks the session's
failure state and reads from its loaded asset store; it does not register resources
or mutate ECS state. Read failures are sticky. CPU document caches may warm, so
rollback does not claim to restore asset-store cache statistics.

`World.data`, `World.resolveData`, nested data fields and cooked prefab behaviour
data fields now share one staged SDK cache. Committed instances remain the same
objects and keep their runtime values. Newly constructed instances enter the
edit-scoped cache before nested references resolve, preserving cycles and shared
list entries. `loadedData` exposes the combined logical view inside the edit. Only
successful publication adds staged instances to the world's cache. Abort invalidates
newly staged instances (`isAlive == false`), including ones user code retained,
and leaves committed cache membership and existing instances untouched. A retry
constructs fresh instances. A successful read commits its cache entry even when a
prefab using it is subsequently canceled, consistent with world-lifetime data assets.

Factories must return fresh unbound, non-destroyed instances. Reusing a bound asset
or a previously created instance in the same edit fails before authored field writes.
Reentry into an asset's constructor before its staged instance exists also fails
stickily; ordinary cycles through nested data fields remain supported. Malformed
references, wrong data types and invalid sub-asset aliases reject the entire edit,
including when caught. No load-diagnostic callback runs during this strict path.
Legacy loads outside edits retain their warning-and-null behavior.

This makes data **loading and cache ownership** transactional, not arbitrary Dart
field mutation. Setters run on fresh instances before commit; arbitrary constructor
or setter side effects remain outside rollback. `applyDataEdit` and data-object
destruction remain guarded inside edits. Rewriting script fields that already hold
ObjectSpec placeholders is still open because it would mutate pre-existing user
instances rather than construct new ones.

[Nine new SDK regressions](../../packages/fynkel/test/data_asset_test.dart) cover
cyclic identity, staged lookup/publication, abort and escaped instances, existing
shared values, invalid references/types, prefab data fields/lists, final native
validation failure, factory reuse/reentry, cached sub-asset aliases and guarded
mutation helpers. A new native protocol regression verifies that a document read
without a project fails stickily and aborts earlier native writes. F2 remains active.

The [shared-data qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-data-assets.json)
records all **17 checks passing**: **910 workspace tests** (8 ignored), **53
stripped-core tests**, **6 stripped-native tests**, **10 stripped prefab-resource
tests**, **2 stripped mesh tests**, **431 SDK tests**, **16 authoring tests**,
**39 Flutter tests**, and **91 focused editor tests**, plus strict analysis,
Clippy, formatting and generated-code verification.

The ObjectSpec identity follow-up removes the script-placeholder restriction by
promoting the first build's placeholder to the canonical `GameObject`. The script
field already holds that object: neither explicit edits nor legacy builds rewrite
caller-owned script fields. The legacy `_resolveSpecReferences` setter pass and GameObject forwarding pointer
are removed. The spec binding table alone tracks the latest build, so an earlier
handle does not retain or forward to a later build. The same identity rule applies to ordinary objects and cooked prefab roots.

Inside an edit, pending identity and world access come from the scoped adapter;
committed fields remain unbound until publication. Ordinary name, transform,
hierarchy, lookup, behaviour attachment and lifecycle-intent helpers therefore see
the staged object through the original placeholder. Commit binds surviving handles
to their real entities. Abort leaves first-build placeholders unbound and reusable,
with original fields and script ownership unchanged. This intentional retry rule
applies to declaration placeholders; other aborted pending handles remain retired.
A successful build that is canceled publishes a dead handle, which is never revived.

Captured references are stable object identities, not aliases for the latest build.
`spec.object` returns the latest successful build (or the staged build inside the
edit), but a captured handle continues to name its original object when the same
spec is built again. Use a function that creates a fresh spec tree for each copy
whose internal references must point to that copy. This also removes the old
difference between script fields rewritten once and native component placeholders
that could follow later builds. Authoring still recognizes unbuilt placeholders.

Registered script references are checked without invoking their setters. Forward
object/script references within a tree and references to earlier staged builds or
attachments are supported. Foreign/dead object owners, unresolved specs and removed
script targets fail stickily. These checks occur during construction; subsequent
script field writes remain ordinary Dart writes, and script references are not a
new native strong-reference graph. Existing engine lifecycle semantics still apply
when a referenced object is destroyed later.

[Construction regressions](../../packages/fynkel/test/gameplay_spec_test.dart) and
[prefab regressions](../../packages/fynkel/test/gameplay_prefab_test.dart) cover
canonical identity, setters that must never execute, committed and staged access,
abort/retry, stable references across rebuilding, cancellation, legacy construction,
forward and earlier script attachments, foreign native-component owners, sticky
invalid-reference failures and unpublished-entity escape rejection. Automatic
callback routing, effects/presentation and legacy-overlay retirement remain open.
F2 is still active.

The [ObjectSpec identity qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-spec-identity.json)
records all **17 checks passing**: **910 workspace tests** (8 ignored), **53
stripped-core tests**, **6 stripped-native tests**, **10 stripped prefab-resource
tests**, **2 stripped mesh tests**, **442 SDK tests**, **16 authoring tests**,
**39 Flutter tests**, and **91 focused editor tests**, plus strict analysis,
Clippy, formatting and generated-code verification. After removing the obsolete
forwarding pointer, all affected Dart analysis/test commands were rerun; the capture
retains their earlier results and records hashes of the final tested sources.

The physics-effect follow-up distinguishes data publication from one-shot runtime
intent. `GameplayEdit::body_effect` and gameplay operation 19 stage rigid-body and
character commands under the same commit boundary as component edits. SDK force,
impulse, torque, velocity, kinematic-target, teleport, move and jump helpers route
through it inside `World.edit`. An effects-only edit also publishes its requests.

The journal checks module availability, scoped owner identity and body/controller
configuration, using the same pure validator as physics execution. The validator
also rejects non-finite vectors and non-normalizable pose rotations. Final commit
rechecks surviving targets after later component edits. Requests for targets
removed in the same edit are canceled. Aborts, including caught validation errors,
publish neither data nor effects.

Successful commits append resolved intents to a native queue that does not hold
shadow component state and does not block subsequent edits. The fixed-tick command
phase consumes it once, in request/commit order, after the remaining legacy batch.
Later destruction cancels requests; later component or module changes are checked
at execution and can produce diagnostics. Inactive targets consume their physics
requests without executing them. Completed body/character results and transforms
remain unchanged until simulation; commit does not pretend to have integrated a
force or teleport. Physics readiness, including unsupported hierarchy/resource
conditions, remains the subsystem's responsibility. A fresh character's initial
broad-phase placement no longer discards explicit move/jump intent: execution waits
for the first ready tick and subsequent requests retain the existing last-value
precedence.

[Native effect regressions](../../crates/fynkel_core/tests/gameplay_effects.rs) cover
pending owners, ordered execution, effects-only edits, abort, final configuration
failure, cancellation, invalid payload/type, inactive objects, later revalidation
and disabled modules. [SDK regressions](../../packages/fynkel/test/gameplay_effects_test.dart)
exercise the actual owned bridge, canonical spec handles, teleport visibility,
initial character readiness and once-only impulses at 30/60/120/240 Hz. A native
protocol regression covers conflicting owners, bad opcodes, truncated records and
sticky rejection without publication. This does not complete F2: audio/animation
and scheduling effects, automatic callback routing, presentation and retirement of
legacy state paths remain open.

The [body-effect qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-body-effects.json)
records all **17 checks passing**: **919 workspace tests** (8 ignored), **54
stripped-core tests**, **7 stripped-native tests**, **10 stripped prefab-resource
tests**, **2 stripped mesh tests**, **451 SDK tests**, **16 authoring tests**,
**39 Flutter tests**, and **91 focused editor tests**, plus strict analysis,
Clippy, formatting and generated-code verification. The stripped core gate now
includes the new effect test target.

The scheduling follow-up stages coroutine, invoke and tween ownership in the SDK
adapter. `startCoroutine`, `invoke`/`invokeRepeating`, script/world/object tweens,
`Coroutine.stop`, `stopAllCoroutines` and `cancelInvoke` now participate in explicit
`World.edit`. The scheduler remains the single owner of committed routines; the
edit holds only start/stop intent. Existing routines are not advanced, rewound or
copied to implement rollback.

A staged start does not even acquire a user iterable's iterator. Publication adds
surviving starts to the scheduler without running user code. Ordinary coroutines
and real/gameplay-clock tweens start in the next variable scheduler phase;
simulation-clock tweens start in the next fixed scheduler phase. Tween duration
starts with its first execution. Invoke deadlines use their chosen clock at the
scheduling request, including zero-delay calls at the next eligible phase; they
are armed through engine bookkeeping rather than advancing a generator to its
first yield. Legacy starts outside edits retain immediate coroutine/tween startup
and existing invoke deadlines. Coroutine bodies, nested iterators and tween setters
still execute through the existing scheduler; automatic transactional callback
routing is separate remaining work.

Inside an edit, `isRunning`, `isInvoking` and bulk cancellation include staged
starts and stops. Abort invalidates newly scheduled handles and releases their
iterators/callback closures without executing them, while existing cancellation
requests are discarded. On commit, newly scheduled work whose owner/target was
removed is canceled. New non-invoke behaviour routines on an inactive hierarchy
are also canceled; invokes retain their existing disabled/inactive behaviour
semantics. Existing owner destruction still settles at the lifecycle boundary.
Canceled unpublished handles complete their `done` futures asynchronously without
waiting for a future frame. Published cancellations complete during ordinary
scheduler cleanup. World disposal now retires all scheduled work and completes
its futures, including when disposal occurs inside a running callback.

Invalid durations, unsupported tween values, unbound/removed owners and foreign
world access fail the edit stickily. Escaped async callbacks retain the expired
scope and cannot fall back to ordinary cancellation. Iterator acquisition failures
after commit are reported through `onBehaviourError`; arbitrary user side effects
are not rolled back. The [scheduling regressions](../../packages/fynkel/test/gameplay_scheduling_test.dart)
cover deferred startup, abort, reversible cancellation, bulk operations, owner
cancellation, clock deadlines, invalid/cross-world access, async expiry, execution
errors and disposal. The previous guarded scheduling checks now verify supported
commit/defer behaviour.

The [scheduling qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-scheduling.json)
records all **17 checks passing**: **919 workspace tests** (8 ignored), **54
stripped-core tests**, **7 stripped-native tests**, **10 stripped prefab-resource
tests**, **2 stripped mesh tests**, **464 SDK tests**, **16 authoring tests**,
**39 Flutter tests**, and **91 focused editor tests**, plus strict analysis,
Clippy, formatting and generated-code verification. F2 remains active.

The one-shot audio follow-up extends the native effect journal to an ordered sum
of body requests and fire-and-forget sounds. `World.audio.play` inside `World.edit`
now stages a sound with the same commit/abort boundary as data. Effects-only edits
publish too, subsequent edits need no legacy flush, and playback runs exactly once
at the next simulation tick. Gain/pitch validation is shared with legacy command
execution; disabled modules, malformed records and invalid SDK priorities fail
stickily. Protocol operation 20 reuses the generated sound command record.

Clip availability remains an execution check so resources prepared by the same
successful edit can become available before playback. Unknown clips report the
normal execution diagnostic; voice-budget refusal keeps its existing policy.
This is a request to play, not a promise of audibility. Immediate presentation
sounds remain guarded, as do bus/device changes and source controls. Source
controls need an explicit component visibility/ordering contract before migration;
this increment deliberately does not silently change their timing.

Native and SDK regressions cover effects-only commits, next-tick/once-only playback,
abort, caught validation failures rolling back earlier data and sounds, disabled
modules, unknown clips and malformed wire records. F2 remains active: source and
animation effects, automatic callback routing, presentation semantics and legacy
state retirement are still open.

The [one-shot audio qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-one-shot-audio.json)
records all **17 final checks passing**: **924 workspace tests** (8 ignored),
**55 stripped-core tests**, **8 stripped-native protocol tests**, **10 stripped
prefab-resource tests**, **2 stripped mesh tests**, **466 SDK tests**, **16 authoring
tests**, **39 Flutter tests**, and **91 focused editor tests**. Initial failures
were test expectations (wire error category and fixture startup); the capture
retains those runs alongside successful complete workspace/SDK reruns.

The audio source follow-up separates logical configuration from playback transport.
`GameObject.audio` now participates in explicit edits through protocol operation
21. Volume/pitch and play/stop's `playing` flag update the native component journal,
so ordinary component reads see them immediately. Restart, stop, pause and resume
publish ordered transport intents. The next audio tick applies them without
writing component configuration again; later component writes in this edit or a
later edit therefore cannot be overwritten by replay. Volume/pitch need no effect
record: the normal audio phase reconciles the committed source. Aborts publish
neither data nor transport. Final source removal fails a surviving transport
request; owner destruction cancels it. Removal in a later committed edit produces
the existing execution diagnostic once. Module and scalar validation remain
shared with the normal execution path.

The audio runtime now retains pause requests for voices started later in the same
phase. This fixes `play(); pause();` for both transaction and legacy batches;
play/stop reset the pause request. A live voice's query now reports its actual
clip, duration, gain, pitch, looping and bus. Previously the query combined live
voice timing with newer component values, falsely reporting a not-yet-applied
configuration. Without a voice, the source-configuration fallback remains explicit.

Audio state/statistics queries now use the owned-session panic boundary for
immutable committed reads, allowing playback observation inside an edit without
aborting it. They mutate only bridge buffers/counters; no source or journal value
is published. Other legacy query families retain their existing guards. Narrow
30/60/120/240 Hz source-control regressions do not close the full F2 visibility or
presentation qualification gate. Animation controls, automatic callback routing,
presentation/bus/device policy and legacy state retirement remain open.

The [audio source qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-audio-controls.json)
records all **17 checks passing**: **933 workspace tests** (8 ignored), **56
stripped-core tests**, **10 stripped-native protocol tests**, **10 stripped
prefab-resource tests**, **2 stripped mesh tests**, **470 SDK tests**, **16 authoring
tests**, **39 Flutter tests**, and **91 focused editor tests**. Permanent regressions
cover configuration/transport ordering, pause before voice creation, actual voice
queries, rollback, final source removal, disabled modules, malformed records,
committed read-only queries, legacy compatibility and the four render rates.

The animator configuration follow-up migrates pause/resume, speed and weight to
ordinary native component edits. Protocol operation 22 reuses the generated
animation record and shared record-to-operation decoding. Both legacy execution
and the journal use the same configuration mapping and `Animator` validator.
Logical reads see writes immediately, abort restores the prior configuration,
and commit queues no old component values for later replay. Pending objects and
existing animators use the same path. Animation clocks and poses advance only at
their normal simulation phase.

Animator-state queries now use the owned-session boundary for immutable committed
reads. Their configuration fields describe committed components; timing, fade
and root-motion fields describe completed runtime work. Inside an edit, ordinary
`get<Animator>()` reads staged values while `animation.state` excludes them. This
is an explicit migration contract, not a claim that the mixed legacy state record
is the final F2 published-view API.

The remaining animation operations stay guarded: play/restart, stop/rewind,
crossfade, seek, blend/controller parameters, state transitions, one-shots and IK.
Crossfade in particular must preserve the outgoing runtime state without replaying
stale configuration, so it cannot safely be implemented as a delayed component
write. The native protocol also rejects unsupported operations stickily. Full
animation transport, automatic callback routing, presentation policy and legacy
state retirement remain open. Narrow 30/60/120/240 Hz pause/resume tests qualify
this increment only.

The [animator configuration qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-animation-configuration.json)
records all **17 checks passing**: **938 workspace tests** (8 ignored), **57
stripped-core tests**, **11 stripped-native protocol tests**, **10 stripped
prefab-resource tests**, **2 stripped mesh tests**, **474 SDK tests**, **16 authoring
tests**, **39 Flutter tests**, and **91 focused editor tests**. The new
[SDK regressions](../../packages/fynkel/test/gameplay_animation_test.dart) cover
pending configuration, ordering, rollback, invalid/unsupported operations,
missing/foreign owners, committed reads and four-rate pause/resume timing.

The animation transport follow-up extends operation 22 and the single native
effect journal to play/restart, crossfade, stop/rewind, seek and blend parameters.
The entry point is now `GameplayEdit::animate_control`. Clip selection, speed,
looping and playing publish through the ordinary component journal. Runtime
intents retain the preceding logical animator configuration, solely to identify
the outgoing side of a crossfade. They do not hold a second mutable runtime or
replay old component values. At the next tick, the shared playback helper uses
the existing resolved targets and runtime time to form the fade, applies runtime
changes, and leaves final committed component configuration untouched. A later
component write therefore wins. No resolved outgoing motion means no outgoing
fade, matching legacy semantics. Intents run in order and are consumed once.

Scalar validation, generated record decoding and runtime execution are shared
with legacy playback. Final validation requires surviving owners to retain a
compatible animator; owner destruction cancels requests. Source removal or a
controller change in a later edit is checked at execution. Missing motion uses
existing runtime diagnostics, allowing resources prepared by the same edit to
publish before playback. SDK invalid target types and clip/controller mismatches
now reject the edit stickily even when caught. Configuration-only controls still
queue no effect.

Regressions compare transactional and legacy crossfade poses/timing, verify
no runtime or pose publication on abort, exercise ordered play/seek/stop and
parameter requests, ensure later configuration wins, and cover destruction,
final removal, invalid payloads and four-rate crossfade timing. Controller
parameters/states, controller one-shots and IK remain guarded. Automatic callback
routing, presentation and legacy-state retirement remain open; this is not the
full F2 timing/visibility qualification.

The [animation transport qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-animation-transport.json)
records all **17 checks passing**: **942 workspace tests** (8 ignored), **57
stripped-core tests**, **11 stripped-native protocol tests**, **10 stripped
prefab-resource tests**, **2 stripped mesh tests**, **478 SDK tests**, **16 authoring
tests**, **39 Flutter tests**, and **91 focused editor tests**. Four new native
animation regressions and four new SDK regressions cover this transport increment;
existing unsupported-operation regressions now target controller operations or
invalid seek values rather than newly supported stop/seek requests.

Controller state transitions, one-shots and IK now use the same native runtime-intent
journal as clip transport. They publish no playback or pose changes until the next
tick, preserve request order and disappear on abort. Shared scalar and definition
lookup helpers validate registered state, slot and goal names. Requests bind the
selected controller identity, with checks both at final commit and execution, so
a controller replacement cannot redirect an older request. Unknown controller
resources and one-shot motion readiness remain execution-time dependencies;
prepared resources may publish after data commit and before playback. Execution
failures report diagnostics without rolling back already committed data.

Completed controller layer queries are now permitted through the owned-session
boundary. SDK one-shot pending caches are neither written by edit requests nor
consulted inside edits. Typed controller setters still fail stickily before their
parameter caches can change; migrating those caches is a separate remaining step.
The wire rejects state layers below -1, and the Dart encoder also rejects values
outside signed 32-bit range rather than truncating them.

Native regressions cover legacy parity, state/slot/goal validation, rollback,
controller replacement at commit and execution, ordered one-shots, delayed
resource registration and IK pose rollback. SDK regressions cover pending owners,
completed queries, one-shot cache isolation, sticky validation and state-fade
timing at 30/60/120/240 Hz. Automatic callback transactions, presentation and legacy
overlay retirement remain open; this increment does not complete F2.

The [controller transport qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-controller-transport.json)
records all **17 checks passing**: **948 workspace tests** (8 ignored), **57
stripped-core tests**, **11 stripped-native protocol tests**, **10 stripped
prefab-resource tests**, **2 stripped mesh tests**, **483 SDK tests**, **16 authoring
tests**, **39 Flutter tests** and **91 focused editor tests**. The native protocol
regressions also cover malformed controller layers and completed controller queries
inside an open session.

Typed controller parameters now participate in explicit edits. Shared
`ControllerRun::validate_value` checks names, types and finite values without
constructing or mutating a runtime. Requests use the existing native animation
intent queue and controller-identity validation. SDK integer writes also reject
out-of-range values before encoding, preserving exact signed 32-bit values.

Operation 23 reads a parameter from the selected controller's native logical view:
matching completed runtime values (or registered defaults), then earlier committed
requests and current-edit requests in order. Pending owners use the same path.
Unregistered resources preserve deferred validation; absent values are unavailable,
and staged requests supply their provisional type until the definition is known.
Completed parameter lists remain available through the immutable session query.
Typed getters/setters inside edits bypass legacy `_known`/`_pending` caches entirely;
wrong getter types poison the edit just as invalid setters do.

Native regressions cover ordered reads across commits, integer extremes, validation,
abort, trigger consumption/reset, pending owners, unavailable definitions and
controller replacement. SDK regressions cover typed read-your-writes, completed
queries, cache isolation, sticky getter/setter errors and defaults before a pending
owner's first tick. A native protocol regression proves failed parameter reads
abort earlier writes. Outside explicit edits, legacy getter/cache behavior remains:
transaction intents become visible there at the next tick, while a subsequent edit
can read them immediately. Automatic callback routing and removal of these remaining
legacy caches are still required before F2 is complete.

The [controller parameter qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-controller-parameters.json)
records all **17 checks passing**: **953 workspace tests** (8 ignored), **57
stripped-core tests**, **12 stripped-native protocol tests**, **10 stripped
prefab-resource tests**, **2 stripped mesh tests**, **486 SDK tests**, **16 authoring
tests**, **39 Flutter tests** and **91 focused editor tests**. This is affected
correctness qualification, not completion of the broader performance/AOT gates.

Callback migration now has a safe explicit transaction boundary within synchronous
host and behaviour callbacks. The previous SDK phase-only restriction was stricter
than the owned native session requires: no borrow survives an FFI call, commit
publishes no user callbacks, and frame reentry is already guarded. Empty legacy
queues and resolved legacy spawns remain required. Each explicit edit commits or
aborts independently; callback failure after a completed edit does not undo it.
Automatic wrapping remains open because silently selecting a legacy fallback when
queues are nonempty would preserve two conflicting mutation contracts.

Body/character queries now read committed simulation results during edits. Spatial
queries use the same owned-session boundary and skip mutable physics refresh while
a session is open. They therefore observe the physics scene at entry, not staged
collider, transform or lifetime changes. Outside edits, existing lazy refresh
semantics remain. SDK read guards enforce expired/cross-world scope ownership and
sticky failures; malformed native physics buffers abort the enclosing session.

Regressions cover lifecycle order, behaviour and host error propagation/rollback,
pre-tick versus post-tick body intents, preservation of pending legacy work,
zero/multiple-tick frames at 30/60/120/240 Hz, physics query isolation and rollback,
foreign exclusions, and direct C session preservation/protocol failures. These
changes are callback and simulation-read prerequisites, not automatic callback
routing or completion of F2. The next step is the legacy-queue handoff, followed by
automatic wrapping and presentation/overlay retirement.

The [callback boundary qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-callback-boundary.json)
records all **17 checks passing**: **954 workspace tests** (8 ignored), **57
stripped-core tests**, **13 stripped-native protocol tests**, **10 stripped
prefab-resource tests**, **2 stripped mesh tests**, **494 SDK tests**, **16 authoring
tests**, **39 Flutter tests** and **91 focused editor tests**. Eight new SDK
regressions and one new native session regression cover this increment. This does
not close the broader performance/AOT/scaling qualification gates.

The native core now exposes `World::adopt_gameplay_commands` as an explicit
migration boundary. It stages the complete queued simulation batch through the
existing gameplay journal, in order. Successful adoption commits data without
advancing clocks or running systems, appends runtime intents to the existing
commit-controlled queue, and consumes the legacy queue exactly once. Failure
restores the complete original queue; no automatic fallback or partial consumption
is introduced. Existing ordinary tick submission retains its independent-command
rejection semantics. Adoption deliberately opts the whole batch into atomic
transaction semantics and final-reference validation.

Earlier spawn commands define transaction-scoped pending identities. Targets,
parents and catalogue-declared component references resolve through that mapping;
forward/missing or duplicate temporary identities reject adoption. The commit
returns original temp IDs paired with pending tokens and normal live resolutions.
Canceled spawns resolve to none and allocate no live identity. Adoption emits no
legacy Spawned events; the bridge must reconcile SDK pending handles using this
report rather than wait for a tick. Unnamed spawns use the journal's empty name.
Per-command failures retain their submitted command index; final-validation errors
remain transaction errors. Decode failures also preserve the original queue.

Presentation replay and global audio bus commands are refused without consumption;
their semantics belong to the remaining presentation/global-operation migration.
The operation is currently native-core only. The C/Dart bridge and callback runner
must explicitly integrate queue ownership, spawn reconciliation and failure
reporting before automatic wrapping is enabled. Seven new native regressions cover
temp/reference lowering, no-tick publication, canceled spawns, rollback and queue
preservation, decode/presentation rejection, final-reference validation and
exactly-once next-tick body effects. This is not completion of F2.

The [native legacy adoption qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-legacy-adoption-core.json)
records all **17 checks passing**: **961 workspace tests** (8 ignored), **63
stripped-core tests**, **13 stripped-native protocol tests**, **10 stripped
prefab-resource tests**, **2 stripped mesh tests**, **494 SDK tests**, **16 authoring
tests**, **39 Flutter tests** and **91 focused editor tests**. This remains affected
correctness qualification, not completion of F2 or broader performance/AOT gates.

The native adoption primitive is now connected to the additive
`fynkel_world_gameplay_adopt` C entry point and a regenerated Dart binding.
The report carries original temp-ID/live-entity mappings (null for canceled
spawns), followed by live entity names. The SDK reconciles canonical handles,
retirements, names, subscriptions and caches without synthesizing Spawned events.
Lifecycle callbacks continue at their existing safe boundaries.

Explicit `World.edit` now transfers queued SDK simulation bytes once, adopts the
native queue, then starts the requested transaction. These are two distinct commits:
queued work predates the callback, so callback rollback does not undo it. Rejected
adoption prevents callback execution and keeps queue ownership native; retries do
not resubmit bytes, and ordinary stepping remains available. Presentation work is
still refused before transfer. Active-session adoption fails stickily; null output
and unsupported preview worlds cannot publish an adoption.

Six new SDK regressions cover canonical identity/references and deferred lifecycle,
callback rollback after adoption, retry ownership, canceled/destroyed handles,
manual-step spawn reconciliation and ordered exactly-once effects. Two native bridge regressions cover the report layout,
no-tick/no-replay behavior, null output, queue preservation and active-session
rejection. Existing adapter/callback tests now assert successful adoption instead
of the old empty-simulation-queue restriction. Automatic callback wrapping is not
enabled yet. The current live-name report is linear in entity count; this capture
makes no performance claim and does not close F2's cache retirement work.

The [legacy adoption bridge qualification capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-legacy-adoption-bridge.json)
records all **17 checks passing** in a clean final run: **963 workspace tests**
(8 ignored), **63 stripped-core tests**, **15 stripped-native protocol tests**,
**10 stripped prefab-resource tests**, **2 stripped mesh tests**, **500 SDK tests**,
**16 authoring tests**, **39 Flutter tests** and **91 focused editor tests**.
An earlier run caught an unnecessary cast under strict Clippy; that was fixed and
the full suite rerun, including the manual-step regression. Both raw runs are
retained. Broader performance/AOT/scaling qualification remains open.

The first coordinated callback/presentation/editor batch now applies the journal automatically to individual host callbacks, lifecycle and phase callbacks, contact/zone/animation/audio delivery, coroutine advances and wait predicates. Explicit edits inside an automatic callback join its scope; further nested explicit edits are rejected. Engine data publishes between callbacks, and error handlers observe rolled-back state. Ordinary Dart fields and external effects are not transactional. Callback-started routines begin on a subsequent scheduler pass; failed generator advances or predicates terminate the routine.

Persistent presentation setters participate in the same data journal and preserve dynamic-body ownership validation. Queued presentation replay can be atomically adopted in original command order. A host fixed-tick prepare/cancel boundary captures interpolation history before callback commits; the native step consumes it once. Explicit edits outside that interval snap to the committed pose. A distinct visual-only override API is still unresolved.

Editor live data writes now use an atomic native batch with shared validation. Legacy `apply_now` retains independent-command semantics; scene-aware creation, sibling ordering, resource writes, preview paths and built-in systems are not claimed migrated by this batch. Scene transitions/cancellation and persistence requests defer to successful callback publication, and timeScale participates in rollback. Scene state listeners remain synchronous boundary notifications. Immediate scene loading and cloning inside edits remain guarded pending their own transactional implementation.

Failed legacy adoption is a host boundary failure, reported once per frame rather than attributed to each script. Dependent callbacks, lifecycle starts and routines remain unconsumed for retry after the normal simulation step drains the preserved queue. Queued activity/destruction now adopts before lifecycle settlement. Hierarchy deactivation cancels routines even when their owning script was already disabled.

The old outside-callback state path still exists. This batch makes no scale/performance claim: adoption and publication still enumerate entities, so automatic callback cost requires measurement and reduction during F2.4.

The coordinated batch passed all 17 qualification commands: 975 workspace tests (8 ignored), 72 stripped-core tests, 16 stripped-native protocol tests, 525 SDK tests, 16 authoring tests, 39 Flutter tests, 91 focused editor tests, 10 stripped prefab-resource tests and 2 stripped mesh tests. [Archived evidence](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-first-parallel-batch.json) records the integrated source hashes. Regression coverage includes disposal from fixed/lifecycle callbacks: remaining callback/native work stops, scene futures settle, and prior deferred errors still surface.

The next F2 batch replaces both SDK and legacy native cloning data paths with the shared native journal. `GameplayEdit::clone_tree` freezes source membership/configuration before allocation, accepts live or pending sources, remaps internal references through pending identities and validates external references as new component writes. Invalid/stale targets, unrepresentable root TRS placement and failed registered field copies reject the entire edit. Root placement uses logical world space without a parent, local space with a parent, or an explicit override. Names distinguish no override from an empty name.

SDK script membership and enabled state are frozen before factories execute. Field accessors run as user code inside the transaction; arbitrary Dart side effects cannot roll back. Script/data-asset/native-component references retain their appropriate copied or shared identity. Unknown script types keep the existing omission policy, with warnings delayed until commit. Creation callbacks are deferred for clones made inside edits; the outside-callback convenience invokes them after successful publication. Copied configuration shares resources but resets physics/audio/animation runtime state and inherits no scene/persistence ownership. Lifetime countdown is copied component data. Legacy native cloning preserves report order and forwards adopted spawn resolutions through events; adoption is a separate preceding commit.

Transactional cloning passed all 17 qualification commands: 987 workspace tests (8 ignored), 81 stripped-core tests, 18 stripped-native tests, 542 SDK tests, 16 authoring tests, 39 Flutter tests, 91 focused editor tests, 10 stripped prefab-resource tests and 2 stripped mesh tests. [Archived evidence](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-transactional-cloning.json) records the final source. New permanent coverage comprises 10 core cloning cases (9 without physics), 2 native protocol cases, 2 Dart bridge cases, 9 SDK cloning cases and 6 resource/runtime cases.

The controller-authority batch retires SDK `_known`, `_pending` and `_oneShots` caches. Typed parameters use native logical reads/intents in every context; standalone calls open the same edit as callback calls. One-shot transport uses that edit boundary too, while `isPlayingOneShot` always reports completed layer state. Parameter defaults and selected-controller schema validation no longer depend on whether a tick populated an SDK cache. Completed parameter/layer APIs retain their explicit simulation meaning.

Standalone logical getters currently adopt preceding legacy work as part of edit entry; this may publish prior queued data or reject invalid queues, without advancing the clock. That scope has reconciliation costs and is an intentional transitional implementation, not a performance claim or completion of outside-callback state migration. The remaining F2.4 work covers other overlays/caches and efficient incremental revision tracking.

Controller-cache retirement exposed an Animator lifetime bug: after a committed removal and re-addition with the same controller identity, the old runtime and old queued intents could be mistaken for the new component's state. Actual committed removal now clears AnimatorRun and invalidates already committed animation intents. Logical reads ignore invalidated intents; the next tick rejects each once with MISSING_COMPONENT, even if an Animator was re-added. New requests for the replacement remain valid. Entity destruction still cancels requests silently. Removal that aborts changes nothing, and remove/re-add within one edit keeps the existing final-state coalescing semantics.

Controller authority/cache retirement passed all 17 qualification commands: 991 workspace tests (8 ignored), 81 stripped-core tests, 18 stripped-native tests, 553 SDK tests, 16 authoring tests, 39 Flutter tests, 91 focused editor tests, 10 stripped prefab-resource tests and 2 stripped mesh tests. [Archived evidence](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-controller-authority.json) records the final source. New coverage includes six SDK authority tests, two four-rate/clock tests, three one-shot state tests, and four native component-lifetime regressions.

Global audio bus configuration now participates in the same transaction as entity data. Journal-only optional overrides hold the final gain for each bus and the final global mute switch; Audio remains the committed owner. Shared validation rejects invalid gain/module writes, and successful allocation/data validation precedes mixer configuration publication. Abort sends no mixer changes. Commit requires no simulation tick, but physical device consumption is asynchronous rather than sample-atomic. No later tick replays these configuration writes.

The C/Dart bridge adds operations 25/26 for bus writes/logical reads. SDK setBus and audio-only applySettings join existing scopes or open one; all settings gains stage together. Disabled audio settings application remains a no-op, while config reads remain available. Legacy bus adoption preserves command order and failed queue ownership. Device management and presentation sounds remain guarded. SettingsStore documents/saved files stay outside rollback; whole-project input rebinding is still a host boundary. Standalone getters retain the migration-time queue adoption and reconciliation cost, with no performance claim.

Audio bus authority passed all 17 qualification checks after a recorded SDK-analysis formatting retry: 999 workspace tests (8 ignored), 82 stripped-core tests, 19 stripped-native tests, 565 SDK tests, 16 authoring tests, 39 Flutter tests, 91 focused editor tests, 10 stripped prefab-resource tests and 2 stripped mesh tests. [Archived evidence](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-audio-bus-authority.json) preserves both analysis attempts and final source hashes. New permanent coverage comprises six core bus tests, two native protocol tests, one Dart bridge test, six SDK bus tests and five settings-boundary tests.

Input binding configuration now participates in the native journal. A prepared binding table shares the existing input compiler and publishes only after final validation; raw controls, edges and clocks stay owned by Input. The last staged table wins. Native persistent override metadata publishes only after core commit, so an aborted rebind cannot leak through a later scene load. Legacy native rebinding shares preparation and journal publication, with queue adoption as its prior boundary.

SDK `World.bindActions` joins automatic/explicit edits and returns a staged report with typed binding errors. Input reads remain phase-sampled until the next normal refresh. Invalid strings preserve report-and-skip semantics; malformed protocol requests or a missing loaded project fail the transaction. SettingsStore audio and input updates are atomic when enclosed in a callback/edit; standalone apply still performs separate operations, and its documents/files/listeners remain external. Coverage includes core compiled-table rollback, native protocol and legacy load regressions, SDK callback visibility and combined settings rollback. See the input-binding qualification capture linked from the roadmap.

Scene-aware live editor creation now uses the gameplay journal through `World::spawn_now`. Invalid component values or references reject the entire creation, and the scene's authoring-ID mapping publishes only after successful entity commit. The previous partial-success behavior (a created entity with rejected components) is retired; the compatibility diagnostics list is empty on success, and failure returns `LiveSpawnRejected`. A failed request leaves its authoring ID reusable. Existing queued simulation work is neither adopted nor consumed by this editor boundary: the host must resolve it explicitly, matching other live data edits. Successful creation still requires no tick, emits no per-entity spawn event, and supports paused-world hierarchy and scene unloading. Scene-aware sibling ordering remains an authoring operation after successful creation; general scene loading and native systems remain separate migration work.

The native lifetime system now stages countdown changes and expired subtree removal in one journal per active pass. Invalid source lifetime data rejects the entire pass and reports a diagnostic; no timer or entity is partly changed. Inactive entities retain their countdown, while destruction still removes inactive descendants. Multiple expired ancestors/descendants retire once. The journal carries an internal native-phase context: event timestamps use the tick being computed without advancing the completed clock, and transform propagation/teleport settlement remain at their declared native stages. Public and owned-session gameplay edits retain their existing immediate settlement and current-tick event behavior. This is the first migrated catalogue system, not a claim that all native systems use the journal. Reused input scratch limits collection churn, but journal allocations and whole-world final validation still require scale qualification.

Native emitter creation now uses the same prefab journal as gameplay instantiation. Each emitter's attempted burst is an independent atomic transaction: all instance components, mapped references and initial lifetime values validate before publication. Invalid prefab or launch data publishes no instances or effects; only committed roots enter the emitter's alive list. The emitter clock/fraction still consumes an attempted burst even on failure, preserving the prior no-backlog scheduling behavior; other emitters continue independently. Missing prefab reporting remains once per emitter. Physics-disabled worlds retain emitted body configuration without launch effects.

The internal native transaction entry is restricted to pre-physics systems after the Commands stage. Its validated body effects publish directly to the upcoming physics boundary, while ordinary callback/owned-session effects retain next-tick delivery. Teleport markers follow their normal settlement stage. This prevents emitted velocity arriving one tick late or replaying on the next tick. Permanent checks cover invalid prefab/velocity rollback, corrected retries, alive ceilings, independent emitters, disabled physics, and same-tick/exactly-once launch at four fixed rates. Resource preparation is unchanged: emitters use already registered prefab descriptions/resources. Other native systems and performance qualification remain open.

The remaining native-system write audit distinguishes authored data publication from subsystem runtime progression. Native authored outputs use `edit_native_data_at_tick`, which rejects runtime effects and preserves declared transform/interpolation stages. The pre-physics effect entry remains restricted to systems between Commands and PhysicsStep. Transform validity is shared through `Transform::is_valid`; physics and other world-pose outputs use the scene layer's pure parent-first preparation and all-or-none local-pose publication, avoiding a dependency from physics back into core.

| Native producer | Publication and failure boundary | Runtime/derived owner |
| --- | --- | --- |
| Rotator / Oscillate | One validated pose batch per pass; oscillator clocks and offsets publish only with successful poses. Committed Oscillate removal retires its runtime state. | Oscillation state, reconciled with component lifetime |
| Camera rigs / Follow / LookAtCamera | World-pose conversion validates all targets before pose publication; rig yaw/pitch and pose share a commit. | Propagated geometry remains at the declared settle stages |
| Animation / root motion / IK | Per-animator authored outputs and staged IK results validate before publication; native data failures report diagnostics. | Playback/controller clocks, marker/end events, skin caches and IK runtime remain owned by animation; rejected pose output does not rewind a completed playback step |
| Character input / physics | Driver commands use the shared typed effect validator; physics pose writeback validates the whole output batch before publication. | Rapier bodies, contacts, character state, query caches and ownership markers remain physics-owned; rejected output does not rewind the solver |
| Audio | Playback flags produced by the mixer publish as a validated data batch. | Voices, transport clocks and completion events remain mixer-owned; rejected flags do not rewind audio |
| One-shot trigger zones | Named entry events publish only when authored TriggerZone removal commits; rejected removal retains the component and suppresses its named event. | Raw contacts remain completed physics facts |
| Deferred destruction | One journal commit removes queued subtrees; failure retains the queue for retry and emits no destruction events. | Queue ownership stays with the world until successful commit |
| Transform history, propagation, snapshots | Derived bookkeeping remains in the declared tick stages; native data commits do not force an early propagation or interpolation snap. | Scene/world derived state |

These are explicit per-pass/per-producer boundaries, not one rollbackable simulation tick. In particular, a solver or playback event describes completed subsystem work, not a guarantee that a later authored-output batch succeeded. The legacy Commands stage still processes independently accepted commands and dispatches committed intents; retiring its external wire/temp/overlay compatibility path is a separate F2 task. This audit does not claim whole-scene/resource operations or all legacy ingress have migrated, and performance qualification remains required.

The earlier prototype evidence remains:

Three native tests and the two Dart strategy contract runs pass; native render extraction matches supported gameplay transforms/activity. Native and JIT frame captures include commands, source hashes, hardware, allocations or bridge bytes and checksums. No GPU, active physics population, skeletal animation, assets, full editor or AOT qualification is claimed. Acceptance is a design decision, not a claim that F0/F1/F2 have passed their gates.


### Ordinary SDK path and cache retirement (2026-10-01)

Ordinary SDK calls now join the active gameplay transaction or open one that
commits before returning. This supersedes the outside-callback command/overlay
behavior described in earlier migration notes. `World.edit` remains the explicit
way to make several calls atomic; automatic callback scopes remain unchanged.
Spawn, build, instantiate, clone, component writes/removal, name, pose, hierarchy,
activity, destruction, script membership, effects and scheduling use this route.
A chained sequence outside an edit is several commits, not one atomic batch.

An ordinary spawn returns a published handle. Only spawns inside an active edit
remain pending; abort invalidates those scoped identities. The SDK simulation
command buffer, wire-temp-id map, pending persistence list, transform/component
write echoes, component-presence masks, per-tick transform query cache, entity
name/hierarchy table and rejection-target bookkeeping are removed. Ordinary
logical lookups use native journal state. `componentsOf` explicitly reads committed
native data outside an edit and staged data inside it, without an SDK value cache.

Effects still execute at the next eligible native tick. Invalid ordinary writes
throw immediately, independently of `throwOnDiagnostics`; raw compatibility
commands retain their native diagnostic behavior. Script membership publishes
with data; lifecycle callbacks wait for their normal boundary. Thus a standalone
spawn can receive fixedUpdate in the first frame. Standalone coroutine/tween
creation now schedules first execution at the normal scheduler phase, matching
creation inside callbacks/edits. Duration begins when that work first executes.

Cooked dependencies previously registered by legacy submission now use an owned
`PreparedGameplayResources` plan. Mesh/physics material, skin, clip/blend,
controller and sound data decode before commit; world registration occurs only
after successful data publication and union audio-capacity validation. Prepared
controller definitions are visible to the owning journal's parameter reads and
validation, without registering them in the committed world. Missing resource
execution diagnostics remain supported; abort publishes neither resources nor
effects. Clip and controller dependencies prepare independently, so a stale
inactive clip cannot prevent loading the selected controller. Gameplay exchange
traffic now contributes to bridge byte counters.

Retained state has an explicit owner:

| Retained state | Owner and purpose |
| --- | --- |
| Entity-to-GameObject and typed component handles | SDK identity only; no authoritative component values |
| Behaviour objects, coroutine iterators and scheduler queues | Dart runtime ownership; transactional attachment/scheduling intent |
| Completed transform snapshots and subscriptions | Native completed-tick output and event delivery, not the source of logical reads |
| Last pose/name after disposal | Read-only diagnostic fallback, captured before native disposal |
| Prepared resources and scoped controller definitions | Transaction lifetime; discarded on abort, registries published on commit |
| Asset/data-asset caches | Resource ownership and shared identity with existing load/disposal rules |
| Input phase samples, physics/animation/audio runtime | Their declared phase/subsystem owners |
| Immediate presentation audio | Explicit device-effect boundary |

Raw `NativeWorld.submit` remains a compatibility ingress. Its queues are adopted
at the gameplay boundary; ordinary SDK calls no longer produce those bytes or
maintain matching speculative state. Wire Spawned events belong to that ingress;
journal spawn identity resolves through the commit report. Scene loading,
resource replacement, remaining editor mutation entries, separate visual-only
presentation policy and full native compatibility retirement remain explicit F2
work. This change does not claim that every external/native entry has migrated.

The SDK benchmark now batches its thousand-object writes and reads with
`World.edit`, exercising the production journal instead of a removed flush path.
It exposes substantial exchange/copy cost. Entity membership scans, per-value FFI
traffic, allocation/tail-latency budgets and scalable invalidation still need
performance qualification; correctness success is not a large-game performance
certification. See the roadmap's qualification capture for exact final evidence.


SDK cache retirement is qualified by [the final capture](../../benchmarks/foundation/results/2026-10-01-87e3dec8-f2-legacy-cache-retirement.json): all 17 checks pass, including 579 SDK tests. The immediate-construction `creating` helper and its collection state are removed: standalone clone now defers lifecycle exactly like standalone spawn/build/instantiate. Public liveness and attach validation consult native membership; projected anchors use native batch validation to avoid a per-anchor lookup. Qualification retries and their reasons remain in the capture. F2 remains active for the boundaries listed above.

### Raw native command mutation adapter (2026-10-01)

Raw `World::submit` / C `fynkel_world_submit_commands` and `World::apply_now`
retain their compatibility scheduling and per-command acceptance policy. Their
simulation mutation switch is retired: each resolved command publishes through
`GameplayEdit` using the same configuration, hierarchy, activity, reference and
effect validation as SDK edits. Presentation name/component/bus writes use that
adapter too. This is an adapter to the chosen authority, not a second component
store. A raw batch is **not** an atomic `World.edit`: earlier accepted commands
remain committed if a later command is rejected.

Simulation commands still execute at the next Commands stage; temporary handles
resolve in order and Spawned/ParentChanged/Destroyed events retain their tick.
Duplicate spawn IDs within a tick now reject without allocating or replacing the
first mapping. Immediate compatibility edits preserve future queued commands,
including on failure/unwind. Native physics requests retain the originating
command index through execution-time diagnostics. New stale/foreign component
references and invalid zero-length rotations reject before publication. An
already valid reference may subsequently lose its target; its subsystem's dead
reference behavior remains unchanged.

Committed effect storage now contains only typed body, audio, animation and
one-shot requests. It cannot carry structural/configuration commands back into
the legacy dispatcher. Playback delivery has no fallback configuration writer:
the journal has already published that data, and a later edit must not be
replaced by old gain/pitch or animator settings when a request executes. Raw
playback commands deliver their newly committed intents at the compatibility
boundary, retaining ordered play/pause behavior; they do not drain earlier SDK
requests prematurely.

The wire format, decoder, indexed queue, temp-ID translation and explicit queue
adoption remain compatibility transport. Presentation transform overrides/replay
remain until the separate visual-only policy is decided. Scene construction still
owns its component insertion and forward-reference resolution. Native automatic
asset registration at raw submission remains a resource-boundary migration item;
this change does not claim transactional resource publication for raw callers.
Removing the external command ABI itself would require migrating its remaining
hosts/tests and changing the versioned contract. F2 remains active.

### Raw command resource publication (2026-10-01)

The eager native `register_animation_assets` submission pass is retired. Merely
submitting simulation wire commands no longer registers clips, controllers,
skins, sounds, collider meshes or physics materials. Native project loads,
editor previews and asset stages install the cooked `Assets` source through one
setter. Core sees only a host resource-preparation/publication contract, keeping
the asset store and decoding implementation outside the core dependency graph.

A raw command prepares its dependencies at execution. The owned plan reuses
`PreparedGameplayResources`, including independent motion/controller preparation
and shared prepared controller definitions for journal validation. Audio capacity
is checked before data commit. Rejected data, effects or capacity checks drop the
plan with no world registry changes. Successful data commit is followed by
resource publication, then playback delivery; a one-shot sound can therefore
use its newly registered clip at that same boundary. Publication invariant panics
poison the world rather than claiming rollback after committed data.

Explicit queue adoption prepares one union of resources for the entire adopted
transaction. Duplicate sound keys count once and existing registrations consume
no new slots. A failed adoption retains the original queue and publishes neither
its data nor its resources. Independent raw command execution retains its
per-command policy: a later rejected command does not roll back earlier accepted
commands or their resources. Missing cooked dependencies retain the existing
readiness diagnostics; preparation does not fabricate placeholder assets.

Preparation is synchronous at execution/adoption and resolves against the asset
source installed at that boundary. This retires an eager-registration side effect,
not the F5 asynchronous readiness/budget work. The C ABI, compatibility queues and
presentation transform policy remain separate F2 items. Whole-scene/resource
replacement and performance qualification are not closed by this change.

### Compatibility transport retirement: native producers (2026-10-01)

Editor preview diffs, gizmo/Inspector preview writes, animation-stage controls,
and the shared headless/standalone demo now call the gameplay journal directly.
They no longer submit compatibility command batches. Preview diffs retain a
private, synchronous document-change representation; it has authoring UUIDs,
no wire IDs, no queue, and no independent mutation or validation implementation.
Spawn mappings come from the successful commit's scoped pending handles. An
unrelated compatibility `Spawned` event can no longer be interpreted as an index
into the preview document.

Each preview diff round or live edit batch is atomic. Rejection preserves its
previous data and does not advance the preview clock. Successful diff counts
are recorded after commit. The existing hierarchy-round/reload policy and
separate sibling-order reconciliation remain; an entire multi-round sync is
not claimed to be one transaction. A rejected round reports its error before
falling back to reload. Animation stages retain the steps needed to sample
controller/blend state after committed controls.

Prepared native resources share the same preflight/stage/commit/publication
contract as owned sessions. Preview edits prepare component dependencies from
the current asset store; rejection drops them. A publication panic poisons the
world. This prevents transport removal from silently removing asset preparation.
The demo captures interpolation history before fixed gameplay edits and resolves
its handles without waiting for events. Its queued-command count is now zero;
that counter does not measure journal mutations.

Keeping these callers on the queue would keep temporary-ID coordination alive
inside the engine. Replacing their queue with another queue would preserve the
same dependency. Direct journal calls remove it while retaining each consumer's
explicit sampling boundary.

This is the native-producer portion of transport retirement. The external raw
C/Rust command ingress, SDK adoption/liveness boundary, SDK immediate-audio
buffer, generated command transport and compatibility tests still exist. Their
removal must preserve lifecycle reconciliation, effect timing and invalid-input
coverage. F2 and full compatibility transport retirement remain open.

### SDK transport retirement and ABI 4 (2026-10-01)

The SDK no longer constructs or submits compatibility command batches. Its
`CommandBatch`, `CommandLane`, `SubmitResult`, raw submit methods, presentation
buffer and `flushPresentation` entry point are removed. Generated SDK bindings
exclude the remaining native raw submission function/result/lane. Fixed wire
records still used as typed journal/effect payloads are retained.

Immediate one-shot audio uses `fynkel_world_audio_play`, a dedicated operation
with one fixed sound record and its string tail, without a command envelope or
queue. Core shares one-shot validation with `GameplayEdit::play_sound`, prepares
sound dependencies through the installed asset source, preflights publication,
and delivers only this immediate request. It cannot consume previously committed
simulation effects, advance clocks or replay at the next tick. Invalid requests
publish no resources or voices. Capacity rejection is reported as a diagnostic;
publication panics retain the existing poison policy. Native owned sessions and
SDK edits reject immediate playback stickily. Successful calls do not re-report
old diagnostics from an earlier failed request.

Callback entry uses `fynkel_world_gameplay_state`, a read-only identity/name
snapshot. It reconciles handle liveness, names, subscriptions, observation and
behaviour scheduling without adopting gameplay or consuming lifecycle events.
Reconciliation describes already-committed native changes and survives failure
of the following SDK edit. Native destruction is delivered to script cleanup
once, even after that edit aborts. Full-world snapshot/enumeration cost remains
an explicit performance-qualification item.

The exported adoption operation is removed, so the runtime ABI advances from 3
to 4. Runtime/schema constants, C header, generated Dart codecs/bindings and
source/fixture locks move together. Old SDKs must rebuild against ABI 4; keeping
the old adoption symbol would preserve the mutation-on-entry contract this
change removes. Authoring ABI versioning is separate and unchanged.

This closes the immediate-audio and callback-entry portions, and the SDK's raw
transport surface. It does **not** close full native transport retirement:
`fynkel_world_submit_commands`, core `submit`/`submit_indexed`, compatibility
queues, temporary IDs and the internal adoption adapters used by legacy native
clone/input entry points still exist. Their regression coverage must migrate
before those remaining implementations are deleted. This work is tracked rather
than hidden behind a renamed queue or a test-only imitation of the old runtime.

### Native C transport retirement (2026-10-01)

The runtime C boundary no longer exposes `fynkel_world_submit_commands` or a
submission result/lane/kind. The native decoder, command buffer kind, batch
writer and raw transport statistics are deleted, rather than retained behind
FFI exclusions. Generated bindings come directly from the reduced header.
Owned gameplay transactions are the C host's mutation boundary; immediate sound
retains its dedicated operation. Runtime ABI 4 is the unreleased migration
boundary for both this removal and the earlier SDK/adoption removal.

Ten unused wire records (raw spawn/destroy/transform/parent/name/activity,
component set/remove, command envelope and bus command) are removed. Shared
component, query, event and typed body/audio/animation/sound payloads remain.
The schema fingerprint is `0x8da8b43e4b1f1c3e`; generated codecs, current protocol
fixtures and source/fixture locks move together. Historical fingerprinted
fixtures remain historical evidence, not an accepted submission protocol.

Native ABI, cooked-resource, leak and dynamic-library identity tests use the
actual owned C protocol. Spawns resolve scoped handles from commit replies;
there is no test-only command queue or synthetic spawn-event adapter. Tests
retain subsystem queries, contacts, animation markers/controller behavior,
audio lifecycle/device behavior, subscriptions, snapshots, malformed-input
rollback and foreign/stale identity coverage. The dynamic-loader gate also
asserts that both retired C transport symbols are absent.

`World::adopt_gameplay_commands` and per-command `World::apply_now` are removed.
Clone and input rebinding operate on committed state without adopting unrelated
work or draining events. Atomic live editor batches prepare the dependency
union, stage it through the journal, and publish only after successful commit.
Preview sun edits use the same journal and propagate failures to authoring.

This closes the **C/native bridge transport**, not all Rust compatibility
retirement. Core `World::submit` / `submit_indexed`, the Rust compatibility
adapter, staged queue, temporary-ID mapping and legacy presentation overrides
still exist for Rust callers/tests. Their deletion and migration of the remaining
Rust regressions remain required; the full F2 transport-retirement item stays
open. No second native C mutation protocol remains.

Qualification: all 17 checks pass, including 1070 workspace tests (8 ignored),
570 SDK tests and the full native ABI/leak coverage. The separate independent
library identity gate also passes and confirms retired symbols are absent.
[Evidence](../../benchmarks/foundation/results/2026-10-01-5b66fb4f-f2-native-transport.json)
archives commands, failed initial attempts, final results and 483 source hashes.


### Rust compatibility transport retirement (2026-10-01)

This slice completes the transport deletion left open above. Rust callers use
`World::edit_gameplay` or the atomic live-data adapter; there is no
`submit`, `submit_indexed`, raw adoption, per-command apply path, compatibility
queue, temporary entity namespace or spawn acknowledgement event. Pending tokens
remain scoped to a transaction, and only its successful commit resolves them.
The old spawn event ID and temporary-ID/lane diagnostic IDs are reserved.

Persistent presentation writes are ordinary committed data. The extraction
override map, replay path and `ExtractRecord::overridden` bit are removed.
Fixed-tick preparation captures history before gameplay; edits outside that
prepared tick snap history to their committed values. Rendering, skin palettes
and projection use the same extraction matrices.

Typed committed effects remain intentional subsystem work. They deliver once at
their existing phase, respect destruction/component lifetimes, and cannot replay
old authored configuration. The profiler's stable `Commands` slot now measures
this effect delivery only. Runtime diagnostics retain their phase field and use
the no-transport-index sentinel; failed edits return typed errors immediately.

The atomic live-data adapter prepares a dependency union and publishes resources
only on successful commit. It has no second mutation authority or deferred
submission state. Migration coverage checks rollback, stale/foreign references,
scoped identities, persistent poses, explicit interpolation, subsystem delivery
and resource rejection/retry through the real shared APIs. Native/Apple/render
fixtures and host examples no longer depend on synthetic spawn events. Preview
synchronization reconciles its identity maps against committed liveness instead
of draining the host event queue; unrelated lifecycle events remain available
exactly once.

The unreleased runtime ABI remains 4; removing the spawn event changes the schema
fingerprint to `0xbd96f0308508f61f`. Generated records, current fixtures and
locks move together. Authoring ABI is unchanged. Historical evidence/fixtures
remain historical. [Qualification evidence](../../benchmarks/foundation/results/2026-10-01-5b66fb4f-f2-rust-transport.json)
records all 17 passing checks (1070 workspace tests, 8 ignored; 569 SDK tests),
the separately passing dynamic-library identity/export gate, failed attempts and
560 final source hashes. Scene/resource replacement,
remaining editor entry points, visual-only policy and callback-scale/cache
qualification still prevent an F2 completion claim.


### Atomic scene activation and preview reload (2026-10-01)

Scene construction, immediate prefab construction, scene replacement/unload and
persistent-root destruction now use the shared gameplay journal. Invalid
component payloads reject the complete operation; the previous partial-component
acceptance policy is retired. Successful reports retain `rejected_components`
as zero for compatibility. Authored references resolve after every pending node
exists. Missing authored IDs retain the reported-null loading policy; explicit
live references must pass final transaction validation, including references to
objects a replacement would destroy.

Replace activation stages removal and creation together. A rejected activation
preserves entities, scene metadata, completed snapshots, clocks and events.
Successful publication emits all destruction events, then unload summaries in
reverse scene-load order, then SceneReady. Unload summaries keep their previous
per-scene subtree counts. Persistent roots and their current descendants survive.
Atomic replacement retains the old entities while preparing the new hierarchy and
resources; this increases peak memory compared with unloading first. Membership
and hierarchy scans remain subject to the later performance qualification.

Native activation prepares the dependency union, including emitter prefabs,
before opening the journal. Resource capacity preflight and staged controller
definitions precede commit; registration consumes the owned plan only after
success. Project gravity/modules, residency, input rebinding and loaded-asset
ownership change only after successful activation. Missing dependencies retain
the existing diagnostic policy; this is not the F5 readiness/streaming contract.

Editor preview reload uses that same prepared boundary and only replaces its
identity maps, remembered description and residency after success. Failure keeps
the prior preview available for correction/retry.

This slice does not make whole-project startup (including persistent-object
setup) one transaction, nor allow synchronous scene loading inside callbacks.
Scene requests remain commit-controlled scheduling operations. Runtime resource
creation/replacement, remaining editor resource/data mutation entries and
visual-only/cache/performance qualification remain open.

[Qualification evidence](../../benchmarks/foundation/results/2026-10-01-5b66fb4f-f2-scene-activation.json)
records all 17 passing checks: 1076 workspace tests (8 ignored), 128 stripped-core,
21 stripped-native, 569 SDK, 16 authoring, 39 Flutter, 91 focused editor,
21 stripped prepared-resource and 2 stripped mesh tests. Six added regressions
cover rejection/retry, retiring references, prefab atomicity, prepared resources,
capacity and preview identity recovery. The archive retains initial failures and
560 final source hashes. ABI and schema fingerprint are unchanged.


### Transactional runtime clips (2026-10-01)

Runtime animation clip creation/replacement/removal now participates in the owned
gameplay boundary through operation 28. Native sessions own validated decoded
clips, coalesce writes by UUID and discard them on abort. Publication follows the
successful gameplay/prepared-resource commit, before subsequent playback delivery.
Standalone SDK calls open the same boundary. Native direct setters share validation
and publication helpers and cannot mutate through an active owned edit.

Made-clip liveness is a transaction-aware SDK membership view, not a clip-data
copy: its committed set changes only after native success. This fixes rejected
requests changing bookkeeping before native validation. Aborted creations remain
dead; aborted replacement/removal preserves prior data and liveness. Create/remove
within one edit does not publish a clip. Removal retains the existing playback
copy, as before; resource-cache ownership/eviction remains later qualification.

Regressions cover explicit/automatic abort, create-and-play at four frame rates,
replacement and last-write behavior, removal with ongoing playback, canceled
creation, and sticky malformed protocol/resource failures. ABI/schema unchanged.
[Qualification evidence](../../benchmarks/foundation/results/2026-10-01-5b66fb4f-f2-runtime-clips.json)
records all 17 passing checks: 1077 workspace tests (8 ignored), 128 stripped-core,
22 stripped-native protocol, 575 SDK, 16 authoring, 39 Flutter, 91 focused editor,
21 stripped prepared-resource and 2 stripped mesh tests. Failed attempts and 562
final source hashes are retained. Materials, meshes, whole-project startup and
editor data/resource mutations remain open; this slice does not complete F2.

### F2 closing qualification (2026-10-02)

Owned resource operations 29–31 add runtime PBR/shader materials and meshes with
shared preparation/publication and coalesced per-ID intents. Shader document
copies and read-only returned parameter lists prevent descriptor aliasing.
Operations 32–34 supply identity deltas and transactional persistence. Callback
commit no longer fetches each entity name; callback entry remains conservative.
Required project startup retires old project roots in the scene transaction;
optional persistent prefabs retain an explicit independent warning policy.

ADR-0051 closes the presentation policy and catalogs intentionally separate host,
device and arbitrary-Dart authoring boundaries. The gate report audits cache
ownership/invalidation and records the callback workload. All 17 qualification
checks and the additional dynamic-library identity/export gate pass. F4 owns
complete revisions/incremental entry reconciliation; F3 owns custom component
registration, bulk access and systems. No large-game performance certification is
implied by F2 completion.
