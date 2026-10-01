# F2 gameplay session wire protocol, version 1

This is the internal binary migration boundary for ADR-0046. It is not a second
world-state implementation or the final ordinary gameplay API. The C exports are
in `crates/fynkel_native/include/fynkel.h`; the Dart client lives in
`packages/fynkel/lib/src/native/gameplay.dart` and `NativeWorld.editGameplay`.

## Ownership and lifetime

`fynkel_world_gameplay_begin(world, out_sequence)` transfers the core World into
an owned GameplaySession. Its journal is the same one used by `World::edit_gameplay`.
Every exchange creates a temporary borrowed accessor and drops that borrow before
returning through FFI. No Rust borrow or host input pointer survives a call.
The sequence is a checked, never-reused counter local to this world; the session
identity is the pair (world namespace, sequence).

An open session excludes the normal guarded native mutation/query entry points.
Attempting one rejects the call and fails the transaction even if the caller
ignores the error. Committed snapshot, clock, contains, diagnostic/statistics
reads retain their explicitly committed meaning. Audio playback/statistics and animator-state queries
also read committed state through the owned-session boundary; other legacy query
families retain their guards. Snapshot budget configuration
is host diagnostics, not gameplay mutation. Direct Rust hosts must not request
`core_mut()` while an edit owns the world; doing so panics instead of exposing a
second mutable World reference. Existing raw Rust ECS access has interior-mutation
capabilities and remains an advanced, untracked escape hatch; it must not mutate
committed data during a session. Closing supported raw-access bypasses remains F4. Authoring preview/stage worlds reject session begin with
Unsupported because their authoring session owns mutation.

`end(sequence, commit=0)` aborts and returns the world without changes.
`end(sequence, commit=1)` checks final references and atomically publishes, then
returns the world even on validation failure. Destroying the native world drops
an unfinished session without publication. Rust panics at the bridge boundary
restore ownership, abort, and poison the world. Threading remains the existing
single-thread-at-a-time world contract.

The Dart callback must be synchronous. Exceptions, caught operation failures,
and Future results abort it. Saved edit accessors close in `finally`; later calls
cannot enter FFI. Responses are copied before the next exchange invalidates the
native buffer. This transport deliberately establishes correctness first; it
allocates request/response buffers and does not claim bulk-loop performance.

## Framing

All integers and f32 values are little-endian. Exchange inputs are limited to
32 MiB and are borrowed only during the call. Every input begins with `u32 version`
(currently 1), then `u32 operation`. The parser checks all fields and exact end of
input before invoking a mutation. Malformed framing fails the session. Success
responses contain operation-specific bytes without the normal FynkelBufferHeader.
Failure responses contain UTF-8 diagnostic text. Begin has no response view.
A reply view expires at the next edit call or world destruction.

Primitives:

- `string`: u32 UTF-8 byte length, then those bytes.
- `entity`: existing 16-byte F1 layout: u64 world, u32 slot, u32 generation.
- `reference`: u32 tag, u32 pending index, entity-shaped 16-byte payload,
  u64 sequence (32 bytes total). Tag 0 is None and all remaining bytes are zero.
  Tag 1 is live, with zero index/sequence and the full entity. Tag 2 is pending,
  with this world's namespace, zero slot/generation, this sequence, and an index
  into this session's pending table. Pending references never enter Entity fields.
- `references`: u32 count followed by that many references.
- `component body`: u32 body length, generated scalar body bytes, u32 string-tail
  length, generated string-tail bytes, then references in the built-in catalogue's
  field order. Object-reference fields in the scalar body must be None; conflicting
  representations are rejected. Body size and reference count must match the
  catalogue. The existing component schema fingerprint remains unchanged.

Owner references cannot be None. Live validity and component rules are enforced
by the core journal; pending namespace/sequence/index are validated by the parser.
Unchanged stale stored references remain visible as handles; they are not rebound.
Authoring UUID metadata is not part of the existing runtime wire representation.

| Operation | Request after version/op | Success response |
| --- | --- | --- |
| 1 spawn | generated Transform (40 bytes), parent reference, name string | pending reference |
| 2 destroy | owner reference | empty |
| 3 set component | owner, u32 component ID, component body | empty |
| 4 get component | owner, u32 component ID | u32 present, component body if present |
| 5 entities | u32 count, required component IDs (maximum 1024) | references |
| 6 name | owner | u32 present, name string if present |
| 7 set name | owner, name string | empty |
| 8 parent | owner | reference |
| 9 set parent | owner, parent reference, u32 keepWorld (0/1) | empty |
| 10 children | owner | references |
| 11 activity | owner | u32 activeSelf, u32 activeInHierarchy |
| 12 set activity | owner, u32 activeSelf (0/1) | empty |
| 13 world matrix | owner | 16 f32 values, column-major |
| 14 contains | owner | u32 boolean |
| 15 remove component | owner, u32 component ID | empty |
| 16 find | name string | reference |
| 17 instantiate cooked prefab | UUID string, 40-byte Transform, parent reference, name string | root reference, u32 entity count, repeated (authoring UUID string, reference), metadata JSON string |
| 18 cooked asset document | UUID string | cooked JSON string |
| 19 body/character effect | owner reference, 48-byte generated BodyCommandRecord with nil entity | empty |
| 20 one-shot sound | 40-byte generated PlaySoundCommandRecord, u32 tail length, string tail | empty |
| 21 audio source control | owner reference, 24-byte generated AudioCommandRecord with nil entity | empty |
| 22 animator control | owner reference, 88-byte generated AnimateCommandRecord with nil entity, u32 tail length, string tail | empty |
| 23 controller parameter read | owner reference, name string | u32 kind (0 unavailable, 1 float, 2 int, 3 bool, 4 trigger), followed by u32 value bits when available |
| 24 clone logical subtree | source reference, parent reference, u32 hasTransform (0/1), optional 40-byte Transform, u32 hasName (0/1), optional name string | root reference, u32 count, repeated (source reference, pending copy reference) |
| 25 set audio bus | u32 bus (0 master, 1 music, 2 sfx), f32 gain, u32 global mute (0/1) | empty |
| 26 read audio bus | u32 bus | f32 logical gain, u32 global mute (0/1) |
| 27 bind actions | string JSON object mapping action names to string arrays | string JSON load report describing the staged table |
| 28 runtime clip | string UUID, u32 payload length, ANIM payload bytes (zero length removes) | empty |
| 29 runtime PBR material | string UUID, u32 payload length, MATL bytes (zero removes) | empty |
| 30 runtime mesh | string UUID, u32 payload length, MESH bytes (zero removes) | empty |
| 31 runtime shader material | string UUID, u32 payload length, UTF-8 material document (zero removes) | empty |
| 32 identity delta | none | u32 count; repeated reference, u32 hasName, optional string name; absent name means removed |
| 33 mark persistent root | owner reference | u32 accepted (non-root returns 0) |
| 34 persistent roots | none | u32 count, repeated references including pending roots |


Operation 19 shares the generated body-command layout and decoding with legacy
commands. Its embedded entity must be nil; only the scoped owner names the target.
Payload, module, owner and body/controller configuration validation are sticky and
run before publication, with final configuration checked again at commit. Commit
appends surviving requests to a native effect queue separate from legacy data
batches. The next fixed tick consumes requests in order after its legacy batch;
subsystem execution revalidates against the then-current configuration. Abort
publishes no effects, and destroyed targets cancel their requests. Committed
physics snapshots remain unchanged until simulation. A newly created character
requires its initial broad-phase placement tick; its move/jump intents survive
that preparation and execute on its first ready tick, with later requests taking
precedence. Effect requests do not promise that external/subsystem readiness will
succeed, and later invalid configuration can produce execution diagnostics.

Operation 5 returns a materialized membership list, not a live cursor. Another
call sees later additions/removals. Local transforms use the component operations.

A successful commit returns u32 resolution count, then (u32 pending index, entity)
for each surviving spawn. Canceled spawns have no resolution. A successful abort
returns an empty reply. End accepts only commit 0 or 1. Tokens expire at either end;
a bad sequence cannot operate on a newer session and fails that session if present.

## Migration boundary and remaining work

The Dart `NativeGameplayComponent<T>` pairs a generated record with tagged
reference fields. `ComponentCodec.mapObjectReferences` is generated from the same
component catalogue as the scalar wire layout. It visits every direct object
reference, including null, in schema field order, preserves scalar fields, and
returns unchanged records when there are no references. It constructs reference
records explicitly, so replacing a reference with null clears it (unlike the
existing `copyWith(null)` convention).

The internal Dart bridge accepts per-call `encodeReference` / `decodeReference`
resolvers. Encoding collects tagged references and clears the scalar fields before
encoding; decoding verifies the catalogue's reference arity and reconstructs the
ordinary `GameObject?` fields. No ambient resolver survives a call. Resolver
exceptions fail the transaction stickily. Supplying both a resolver and a nonempty
raw reference list, or supplying an unconverted GameObject without a resolver,
is rejected before sending the component. The raw tagged-list mode remains for
internal protocol tests. The ordinary SDK adapter now supplies these resolvers
from its live/pending GameObject identity maps.

The record-plus-tagged-reference representation remains internal bridge plumbing.
Ordinary `World.edit(() { ... })` hides it behind normal World/GameObject APIs.
The adapter reconciles handles and caches after publication while leaving the
completed snapshot unchanged. It adopts supported legacy queues before entry and rejects invalid adoption, nested explicit/async edits, unsupported effects/resource operations and frame reentry. Existing
outside-edit command queues remain during migration; do not mix the raw bridge
with high-level operations. See [ADR-0046](../decisions/ADR-0046-transactional-world-state-replacement.md)
for the scope, rollback and lifecycle contract. Callback routing now uses this adapter. Remaining resource/effect boundaries, visual-only presentation policy, bulk transport cost and removal of superseded state machinery remain open F2/F3/F4 work.

Behaviour attachment adds no native opcode. The SDK stages Dart scheduler bindings
alongside the native edit and publishes surviving attachments after commit. The
staged script's ordinary native data access uses this same protocol. Constructors
and order getters run before publication; lifecycle callbacks wait for the next
normal boundary. Abort leaves those script instances unbound and reusable.

Enabled-state changes and script removals also use the SDK's staged lifecycle
intent, without adding a native opcode. They publish only after native commit.
Logical removal is visible through ordinary lookups before deferred cleanup
callbacks run; requested enabled state and applied callback state are distinct.
See ADR-0046 for the state table and the temporary outside-edit legacy boundary.

Ordinary ObjectSpec trees and primitives now use the existing spawn/component/attachment
paths in explicit edits. A scoped SDK identity map resolves spec placeholders for
component codecs and publishes spec bindings only after native commit. No new wire
opcode is required for plain trees. Cooked prefab nodes now use operation 17;
script-field placeholders from existing ObjectSpec instances remain guarded.

Operation 17 uses the native `GameplayEdit::instantiate` implementation and
returns the entire authoring-id/pending-reference map before SDK script creation.
Metadata contains prefab id/path/name, asset count, preparation-time residency
statistics, native diagnostics and authored behaviour descriptors (authoring id,
component id, type/version/known, data and source pointer). Metadata JSON carries
no executable code. Entity identities are carried in the tagged binary map.

The owned session retains prepared CPU resource plans until end. End preflights
the union of new sound keys across all plans against committed audio capacity,
then commits the journal and publishes validated resource registrations. Failed
or aborted sessions drop their plans/leases. Successful registrations and leases
remain world-owned, including canceled instances, under the resource policy in
ADR-0046. No new ABI symbol or base component wire fingerprint is introduced;
older protocol-1 runtimes reject unknown operation 17 explicitly.

Dart builds fresh authored scripts and sets scalar/reference fields before commit,
using the returned instance-local object map. Ownership goes through the existing
attachment journal; lifecycle callbacks wait for the next normal boundary.
Invalid types, fields, setters and references fail stickily. A factory returning an
already attached instance is rejected before authored fields are written. Shared data-asset fields use the edit-scoped cache described below. LastInstantiate publishes only after successful commit; its native
diagnostics remain in the report without dispatching a diagnostic callback inside
the edit. Arbitrary constructor/setter side effects are outside rollback.

Operation 18 reads a cooked document through the owned session without mutable
world access. Invalid requests, missing stores/assets and document read failures
fail the edit stickily. It does not register subsystem resources or create Dart
instances. Decoded asset-store caches may warm; cache counters are not rolled back.

`World.data`, `World.resolveData` and authored prefab data fields share the SDK's
edit-scoped data cache. Existing world instances are reused; newly constructed
instances are inserted into the staged cache before recursive data fields resolve,
so cycles and lists share identity. Commit publishes cache entries alongside
attachments; abort invalidates newly staged instances without publishing them.
Malformed references and type mismatches reject the edit rather than dispatching
legacy warning callbacks. Factories must return fresh unbound instances and cannot
reenter construction of the same incomplete asset. Arbitrary user field writes
are outside rollback. `applyDataEdit` and `DataAsset.destroy` remain guarded inside
edits; ObjectSpec script placeholders are still a separate open migration.

Operation 20 stages a simulation sound in the same ordered journal as body effects.
The generated record preserves asset sub-keys, bus, gain, pitch and signed priority.
Malformed records, disabled audio and invalid gain/pitch fail the edit stickily.
The SDK rejects priorities outside signed 32-bit range before encoding. Abort
publishes no sound; successful effects-only edits also enqueue their requests.
Playback happens once at the next simulation tick, after any prepared resources
publish. Missing clips are execution diagnostics; voice-budget refusal retains
normal playback semantics. Presentation sounds and device changes remain guarded inside edits. Bus configuration now participates through operations 25/26.
Source controls are covered by operation 21 below. No ABI symbol or component fingerprint changes.

Operation 21 uses scoped owner references and the existing generated audio record.
Invalid owners, missing sources, disabled audio and invalid gain/pitch reject the
edit stickily. Play/stop update the staged `AudioSource.playing`; gain/pitch update
the staged component values. Playback transport (restart/stop/pause/resume) also
stages an ordered intent. Commit publishes configuration first; at the next tick,
transport applies without rewriting it. Thus a later component write wins even
when an earlier request still awaits execution. Gain/pitch use normal source
reconciliation and never enqueue an old component value. Removed owners cancel
intents; surviving transport owners must retain a source at final validation.
Subsequent source removal is checked once when transport executes.

Audio playback and statistics queries can read immutable committed world state
through the owned-session panic boundary. They only mutate bridge output buffers
and counters. Pending sources have no playback. An active voice reports its own
clip, duration, gain, pitch, looping and bus, not newer source configuration; when
there is no voice, the existing source-configuration fallback remains. Pause is
retained until a newly started voice exists, so play then pause in one batch is
well defined. Play/stop clear that pause request; the shared execution helper also
fixes this ordering for legacy command batches. Other legacy query families retain
their existing edit guards except for the animator-state query described below;
this is not general read-lane migration.

Operation 22 accepts configuration controls (pause/resume, speed and weight) and
clip transport (play/restart, crossfade, stop/rewind, seek and blend parameters).
`GameplayEdit::animate_control` validates the owner, module, component and payload
against the logical view. Clip-specific operations reject controller-owned
animators. Surviving runtime intents are revalidated against final configuration;
owner destruction cancels them, while component removal fails the edit. Later
component removal or incompatible controller changes reject once at execution.

Play/crossfade publish clip, speed, looping and playing configuration; stop clears
playing. Other configuration controls use their ordinary fields. Runtime intents
carry the prior logical animator configuration needed to identify a crossfade's
outgoing clip. At the next tick, the shared runtime helper applies clock/fade/seek/
parameter changes without writing that prior configuration back. Crossfade uses
resolved runtime targets and timing and the captured preceding configuration;
if that outgoing motion has not resolved, there is nothing to fade. The final
committed target configuration wins over earlier requests. Multiple requests keep
journal order, and effects are consumed once. Prepared motion resources can publish
before execution; unavailable motion retains normal runtime diagnostics.

Legacy command execution uses the same scalar validation, configuration mapping,
record decoding and runtime helper, with its existing tick-time configuration
writes. Typed controller parameters now also use ordered runtime intents.
Controller state transitions, one-shots and IK use the same ordered runtime-intent
queue. Scalar validation is shared with legacy execution. Registered controller
definitions validate state, slot and goal names before commit. Unregistered
definitions and one-shot motion readiness are checked at execution, allowing
prepared resources to publish first; such execution failures do not roll back
committed data. Requests bind the selected controller identity, checked again
at final commit and execution. State layers accept only -1 or a nonnegative
signed 32-bit index. SDK clip/controller mismatch and invalid target-type
errors also fail the enclosing edit even if caught.

`QUERY_ANIMATOR` may read immutable committed state during a session. Its scalar
configuration fields are committed component values; time, fade and root-motion
fields are completed runtime results. During an edit, staged configuration is
available through ordinary component reads, not this query. On commit,
configuration changes immediately but animation poses/timing wait for the next
animation phase. `QUERY_ANIMATOR_CONTROLLER` also reads completed layer/state/one-shot results
during edits. SDK one-shot pending caches are untouched by edit requests and
ignored by queries inside edits. `QUERY_ANIMATOR_PARAMETERS` likewise reports completed values. These
changes do not complete F2's separate gameplay/simulation/presentation views.

Operation 23 reads a parameter's native logical value without mutating runtime:
registered default or matching completed runtime value, followed by committed
requests and current-edit requests in order. Only requests for the selected
controller identity contribute. An unavailable definition with no request returns
kind 0; with requests, the latest request supplies its value and provisional kind.
Registered definitions enforce names/types through shared runtime validation;
unknown-resource validation remains deferred to execution. Floats must be finite,
including while resources are unavailable. The SDK rejects integers outside the
signed 32-bit range before encoding. Float values use IEEE-754 bits, signed integers
use two's complement, and bool/trigger values use 0 or 1.

Typed controller setters and getters now use the native journal in every context. Outside an existing callback/edit they open a synchronous edit; inside one they join it. Getters use operation 23 and type mismatches poison the scope. The SDK `_known` and `_pending` parameter caches are removed, so committed intents are readable before the next tick without a separate Dart value store. The completed parameter list remains available separately.

Opening that scope adopts earlier legacy queued work first, including resolving pending spawns. A getter can therefore publish older queued data, and invalid earlier queued work can reject it; it does not advance simulation time. This transitional boundary and its whole-world reconciliation cost remain subject to broader outside-callback migration and F2.4 optimization.

One-shot play/stop also use the shared edit boundary. `isPlayingOneShot` reads completed layer state in all contexts, including slot aliases, without an SDK `_oneShots` request cache. A request committed without an animation tick cannot claim playback already occurred.

### Callback boundaries and committed physics reads

Explicit synchronous `World.edit` may run within host frame and behaviour lifecycle
callbacks. Entry now adopts queued legacy simulation work before opening the edit session.
Presentation replay adopts in the same ordered journal. Automatic callbacks are atomic units, and an explicit edit inside an automatic callback joins that unit: a later callback failure rolls back both. Prior legacy adoption remains a separate commit. Ordinary Dart fields are never rolled back. Frame reentry, further nested explicit edits, asynchronous escape and unsupported operations retain their guards.

`QUERY_BODY` and `QUERY_CHARACTER` now use the immutable owned-session boundary,
as playback queries do. Spatial physics queries also use that boundary. Outside
edits they retain lazy committed-collider refresh; during a session they skip
refresh and read the physics scene as it stood at entry. Pending spawns, staged
moves/removals and requests cannot change those hits or completed body/character
results. A pending exclusion has no existing collider to exclude. SDK exclusions
are checked for edit/world ownership. Malformed native physics buffers make an
open edit fail stickily; SDK read/encoding failures likewise poison the edit.
These reads do not provide a query of hypothetical staged collision geometry.

Automatic callback migration must still define how legacy queued work is adopted
or replaced before entry, preserve exception reporting and callback order, and
complete presentation routing. No conditional fallback from transactions to legacy
writes is introduced by this increment.

### Legacy-queue adoption bridge

`fynkel_world_gameplay_adopt(world, out_view)` adopts the native simulation queue
atomically, outside an owned session. It is an additive C entry point, not an edit
opcode. Null output is rejected before adoption; calling within an edit poisons
that edit. Unsupported preview worlds reject adoption. Success bytes are:

- `u32` spawn count, then `(u64 temp ID, 16-byte entity)` pairs; null entity means
  the spawn was canceled.
- `u32` live count, then `(16-byte entity, u32 name byte count, UTF-8 name)` pairs.

Failure returns UTF-8 error text and retains the queue. The reply uses the existing
gameplay reply buffer, expiring at the next gameplay bridge call. Adoption emits
no legacy Spawned events and does not advance a clock or simulation.

At explicit `World.edit` entry, the SDK first rejects pending presentation work,
then submits its simulation bytes exactly once and transfers queue ownership to
native. A successful adoption report reconciles pending/canceled handles, existing
names/liveness, transform subscriptions and caches; lifecycle callbacks remain
scheduled for normal boundaries. Rejected adoption prevents the callback from
running. The native queue remains available for retry or an ordinary legacy tick;
the SDK must not resubmit transferred bytes. Handles from a previous manual step
still require `drainEvents` before callback entry; the handoff does not consume
those already emitted events. Adopted data is a separate preceding
commit: a later callback abort cannot undo it. Presentation replay and global bus commands now participate in ordered adoption; automatic callbacks use the same boundary.

The current report enumerates live entity names for safe cache reconciliation.
This establishes correctness and has linear cost in live entities; performance
qualification and cache/revision optimization remain part of F2/F0 work.


## Automatic callback and presentation integration

Ordinary SDK callbacks use owned sessions; an explicit edit within an automatic callback joins the existing session. No native borrow crosses FFI. `fynkel_world_gameplay_tick_prepare` captures interpolation history before fixed callbacks; `fynkel_world_gameplay_tick_cancel` clears an abandoned interval without undoing committed edits. Both reject mutation during an owned session. The native step consumes the prepared interval once.

Legacy presentation replay now participates in atomic adoption in original queue order. Persistent SDK presentation setters write the active journal, while completed simulation queries retain their recorded state. Live-reference protocol records with zero world/generation are rejected before constructing a native Entity, without poisoning the world.


## Transactional subtree cloning

Operation 24 freezes the current logical source subtree before allocating pending copies. Staged component additions/removals, hierarchy, own activity and names are included. Internal references remap through the complete source/copy map; valid external references retain their identity. Newly copied invalid/stale references fail the whole edit through standard reference validation. The optional name flag distinguishes no override from an explicitly empty name.

Without a destination parent, default root placement uses the logical world transform; with a parent, it uses the source local transform. A supplied transform overrides both. An affine world pose that cannot be represented as TRS fails atomically. Cloning under a source descendant cannot recursively copy newly created clones because source membership is frozen first.

Dart creates all pending handles before copying registered scripts. Script membership and enabled state are frozen before user factories run; field accessors are user code executed within the edit, and their external/Dart side effects remain outside rollback. Registered field failures abort the clone; unregistered scripts are omitted with warnings emitted only after commit. Internal object/script/native-component references remap, while data assets remain shared. Creation callbacks wait for a safe boundary inside automatic/explicit edits; the outside-callback convenience delivers them after commit before returning.

Configuration and same-world asset references copy; derived physics/audio/animation state does not. LifetimeDespawn copies its current remaining countdown because that value is component data. Clones receive no source scene/persistence ownership. The older native clone entry point also uses the same journal, with preceding legacy queue adoption as a separate commit and existing report order preserved.


## Global audio configuration

Operations 25/26 stage and read bus gains plus global mute through the native journal. Bus indices and boolean encodings are validated before access; nonfinite/out-of-range gains and writes with audio disabled fail stickily. Configuration reads remain available with audio disabled. Each bus keeps its final staged gain; the last bus write supplies the single global mute value. The journal owns only transient overrides; Audio remains the committed owner.

Successful data/resource validation and allocation precede publication of all final audio configuration. Failed edits send no bus commands. Commit requires no simulation tick; the physical device consumes mixer commands asynchronously, so this does not promise sample-atomic device updates. Bus configuration is not queued for a later tick and cannot replay over newer values. Legacy queued bus writes adopt through the same validator in command order.

SDK `setBus` and `applySettings` join the active edit or open one. `applySettings` stages all gains in one scope and stays a no-op when audio is disabled. `busVolume` and `muted` read the logical view; standalone reads use the existing adoption boundary without advancing simulation. Device open/close/suspend and presentation one-shot playback retain their explicit guarded boundaries. SettingsStore document/file changes are external side effects, while input rebinding participates through operation 27. Rollback applies to engine configuration rather than the saved settings document.


### Input binding publication

Operation 27 compiles the project defaults plus the supplied override map through the same compiler as project loading and legacy rebinding. The journal stages only the compiled table; the last binding request wins. Commit publishes the table and native persistent override metadata together; abort preserves both. Invalid binding strings remain nonfatal report entries and are skipped, while malformed protocol JSON and a missing loaded project fail the edit stickily.

`World.bindActions` joins the current edit or opens one. Its returned report describes the staged table, including typed `bindingErrors`. `WorldInput` remains a phase sample: rebinding does not refresh it in the middle of a callback, consume edges, reset held controls, or advance clocks. Successful publication invalidates metadata for the next normal input refresh. SettingsStore engine audio and input changes share one commit when called inside an edit or callback; standalone store application still invokes the two operations separately. Settings documents, disk writes and application listeners remain external side effects.

## Raw command compatibility after mutation-path retirement

The older command wire remains accepted. Its simulation and immediate data
operations now adapt into the same native gameplay journal as owned sessions;
they cannot bypass reference, component or transform validation. Raw commands
keep their original indexes, tick scheduling and independent acceptance, rather
than gaining whole-batch atomicity implicitly. Duplicate temporary spawn IDs in
one tick reject without replacing the successful mapping. Presentation name,
component and bus changes also use the shared journal; transform overrides remain
the separate compatibility presentation policy.

Subsystem effect delivery uses a typed committed-effect queue and cannot replay
an old component value over a later edit. The old direct audio/configuration and
animation fallback writers are removed. The transport ABI is still supported. Raw asset registration follows the
prepared-resource boundary below; scene operations and the visual-only
presentation policy remain explicit F2 work. See ADR-0046's raw native
command mutation adapter decision for the exact retained boundaries.

### Raw asset registration boundary

Raw submission no longer eagerly populates world asset registries. When a command
executes, its cooked dependencies prepare into the same owned resource plan used
by gameplay sessions. Capacity and journal validation precede publication;
resources publish after data commit and before playback delivery. Invalid or
wrong-lane commands cannot register their dependencies. Explicit queue adoption
uses a single deduplicated plan and preserves both queue and registries on failure.
Native project/preview/stage asset-source installation uses the shared setter.
Preparation remains synchronous; asynchronous loading and resource budgets belong
to the readiness milestone. The remaining compatibility transport and presentation
transform overrides have not been removed.

### Native producer retirement (2026-10-01)

Preview sync and live component/transform previews, animation-stage control, and
the headless/standalone demo no longer submit command transport. Native spawn
callers resolve scoped pending entities from successful commits instead of
listening for wire temporary-ID events. Preview component dependencies use
prepared resources with commit-controlled publication. Diff rounds and live
preview batches reject atomically; preview sampling steps and animation-stage
sampling steps remain explicit. The demo prepares interpolation history before
its fixed edits.

The wire ABI is unchanged by this slice. Remaining transport removal order:

1. Replace immediate SDK sound transport with a typed immediate-effect boundary,
   preserving validation, resource publication, once-only delivery and its
   rejection inside an open gameplay edit.
2. Replace SDK adoption with lifecycle reconciliation that does not mutate a
   legacy queue; preserve callback-entry failures and stale-handle behavior.
3. Remove external raw submission/adoption and queue/temporary-ID machinery with
   the corresponding ABI/schema migration. Move relevant behavior regressions
   to journal tests rather than deleting their coverage. Keep schema records
   still used as journal payloads or snapshots.

Sibling ordering, whole-scene/resource operations, and the visual-only
presentation policy remain separately tracked F2 work.

### SDK transport removal and ABI 4 (2026-10-01)

Removal-order steps 1 and 2 above are implemented. Ordinary SDK entry now reads
`fynkel_world_gameplay_state`: live count u32, then repeated entity/name-length/
UTF-8-name entries. There are no temporary-ID resolutions in this response, no
queue publication, no tick and no event drain. Entry rejection preserves pending
script lifecycle state; reconciliation of committed native state survives a
later callback abort.

`fynkel_world_audio_play` accepts one 40-byte `PlaySoundCommandRecord` followed
by its string tail. The record is a typed immediate-effect payload; there is no
command header, batch or lane selector. It validates and prepares sound assets,
publishes them only for an accepted request, and plays once without consuming
simulation effects. Immediate playback is unavailable during an owned edit.

The adoption C export is removed. This is runtime ABI 4; generated bindings,
schema metadata and source-build locks must agree. SDK raw submit methods,
command batch builder, lane/result exports and presentation flush are removed.
Native raw submission is excluded from generated SDK bindings but still exists
in C/Rust pending step 3. Command/effect schema records cannot all be removed:
several remain payloads for the journal or immediate audio operation.

### Native C transport removal (2026-10-01)

The earlier notes about retaining/excluding C raw submission are superseded:
`fynkel_world_submit_commands`, its decoder/batch writer, C command kind/lane,
submission result and raw transport counters are deleted. C hosts use owned
transactions and resolve pending identities from commit responses. Native clone
and input binding no longer adopt a queue. Core adoption and `apply_now` are
also deleted; atomic live editor batches publish prepared resources only after
commit.

Runtime ABI 4 now uses schema fingerprint `0x8da8b43e4b1f1c3e`. Ten raw-only
records are removed from the registry/codecs; shared effect payloads, components,
queries and events remain. Source locks and cross-language fixtures must match.
The C ABI behavior tests and leak/independent-library identity gates exercise the
owned protocol directly, including rollback, event retention and resource retry.

The Rust-only compatibility submission/queue/temp-ID/presentation-override path
is still present. Its removal is the remaining transport task; it must not be
confused with the now-deleted C transport or marked complete by this slice.


### Rust transport retirement (2026-10-01)

The remaining Rust compatibility transport described above is deleted.
`World::submit`/`submit_indexed`, compatibility adoption/queues, temporary
entity IDs and the spawned-event handshake no longer exist. Callers resolve
scoped pending tokens from successful transaction commits. Event ID 1 is reserved.

Extraction no longer has presentation overrides or a replay channel. Persistent
writes use the same journal, while fixed-tick preparation explicitly preserves
interpolation history. Typed committed effects retain their once-only delivery
at subsystem boundaries.

Schema fingerprint `0xbd96f0308508f61f` removes the obsolete spawn event record;
runtime ABI 4 and authoring ABI versioning are otherwise unchanged. Codecs,
bindings, current fixtures and locks are regenerated together.
[Qualification evidence](../../benchmarks/foundation/results/2026-10-01-5b66fb4f-f2-rust-transport.json)
records all 17 passing checks plus the separate dynamic-library identity/export
gate. This completes transport removal, not the remaining F2 work.


### Runtime animation clip publication (2026-10-01)

Operation 28 validates and owns decoded clip data within the native edit session.
Repeated writes coalesce by asset UUID. Missing asset stores, malformed payloads,
and protocol errors fail the edit stickily. Abort drops the resource intents;
successful gameplay/resource commit publishes the final runtime-store changes
and updates the core clip registry before the next animation phase.

SDK creation, replacement and removal enter the shared edit, including ordinary
standalone calls. Made-clip liveness reads staged intent and only updates its
committed membership after native success. The direct C setter shares decoding
and publication helpers and remains excluded during an owned session.
No new C export or schema record is introduced. ABI/schema are unchanged.
Removal retains the existing core playback copy; this is not a cache eviction
or asset-readiness redesign.

[Runtime clip qualification](../../benchmarks/foundation/results/2026-10-01-5b66fb4f-f2-runtime-clips.json)
passes all 17 checks, including the owned-protocol truncation/missing-store test
and six SDK clip transaction regressions.

### F2 closing resource and identity operations (2026-10-02)

Operations 29–31 share the owned edit's failure state, length bounds and exact
consumption checks. PBR/shader writes coalesce in one per-UUID intent map. Mesh
collision preparation happens before commit; publication invalidates built
colliders without fallible recooking. Renderer-store revisions advance only at
successful final publication, once per final resource intent. Direct native
setters share validators/publishers and retain owned-session exclusion.

Operation 32 returns journal identity changes without scanning the live world.
SDK commit metadata uses these deltas; callback entry still uses the read-only
full state snapshot. Delta preparation cannot drain lifecycle events. Operations
33/34 make persistence part of the same logical hierarchy; abort discards marks,
removed roots vanish and children are excluded. No new C export or schema record
is introduced; runtime ABI 4 and schema fingerprint bd96f0308508f61f remain.

[ADR-0051](../decisions/ADR-0051-f2-boundaries-and-presentation-policy.md) resolves
presentation persistence and explicitly separate host/editor/device boundaries.
