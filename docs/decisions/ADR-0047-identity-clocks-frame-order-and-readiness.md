# ADR-0047: Identity, clocks, frame order and readiness

**Status:** Accepted contracts for implementation; live clocks and identity/host authority verified.  
**Date:** 2026-09-30  
**Authority:** User authorization to resolve the remaining foundation contracts with executable checks.  
**Related:** ADR-0043, ADR-0045, ADR-0046; roadmap F0.4, F1.1/F1.2, F2.3, F5/F6.  
**Evidence:** [Contract checks and limitations](../design/foundation-contract-checks.md).

## Context

The shipping 64-bit entity value has an eight-bit world ID and a wrapping 24-bit slot generation. Default native world creation reuses ID 1; callers can also choose colliding IDs. The existing probe demonstrates cross-world and restart aliasing. Assigning eight-bit IDs sequentially would postpone collisions or exhaust normal editor sessions after 255 worlds.

The native clock correctly reports completed simulation time. Dart derives public elapsed from tick plus interpolation remainder while its coroutine clock accumulates clamped frame delta. The probe demonstrates a wait finishing before public elapsed reaches its deadline after catch-up drops. Transform setters also choose a command lane based on phase and body type. A consistent transaction contract needs explicit frame placement.

Background scene preparation retains a dependency closure, but `Assets::preload` discards errors and `PendingLoad::start` can publish a successful prepared result afterward. Dart `SceneLoad.isReady` treats both successful and failed background completion as ready to activate. Completion, success and device residency need distinct meanings.

## Identity decision

Use an opaque **128-bit entity reference**, represented at the ABI as `world: u64, slot: u32, generation: u32`, with an explicit generated wire codec. Zero world/generation is invalid. This is a breaking ABI change; bump the contract version and regenerate Rust/C/Dart records together in F1.1. Do not truncate it into a Dart int or JSON number. Diagnostic JSON uses lossless strings or named fields; disk scenes continue to use authored identifiers.

One engine identity authority allocates world IDs monotonically from 1 for the entire host process/session. All runtime instances whose references can meet use that same authority. The host authority outlives world disposal, Play restart and native hot reload. Callers cannot choose world namespaces. A separately loaded library must receive the host authority rather than instantiate a private allocator if it exchanges references. Independently running processes exchange authored identifiers or an explicit import/remap protocol, never raw entity references.

Never recycle world IDs. Fail world creation with an explicit exhaustion diagnostic at `u64::MAX`. Despawn increments the slot's generation; at `u32::MAX`, retire the slot permanently instead of wrapping. If no slot can be allocated, fail spawn without partially committing its transaction. Runtime constructors and raw lookup validate the full identity, not just the Dart wrapper owner. Low-level ECS constructors become internal or require an authority-issued namespace.

Temporary transaction references are a different tagged type, including their transaction scope. They are not encoded as live handles with a special bit pattern. Commit resolves them; abort and scope expiry retire them (ADR-0046). Authored object references resolve through the scene's identity map and do not persist temporary or runtime handles.

This chooses extra wire width over small namespace limits and wrapper-only protection. F1.1 measures the resulting table, extraction and FFI cost before broad migration; performance is not established by the identity model.

## Clock decision

One world timing service owns these timelines. Consumers read absolute deadlines/start times from it rather than accumulating private copies.

| Timeline | Delta and elapsed | Consumers |
| --- | --- | --- |
| Real | Finite nonnegative host delta, without gameplay scaling/clamp; invalid delta is zero | Pause UI and explicitly unscaled waits/tweens |
| Gameplay | `min(realDelta * timeScale, maxGameDelta)`; zero when scale is zero; elapsed sums accepted deltas | `Time.elapsed`, seconds-based waits/invokes/tweens, variable update logic |
| Simulation | Fixed delta; elapsed is completed tick count times fixed delta | Fixed simulation, physics and simulation animation; explicitly tick-based waits |

`timeScale` must be finite and nonnegative. Sample it at frame begin; a callback's scale change applies next frame. Apply the gameplay clamp once before fixed-step planning. Default `maxGameDelta` remains 0.25 seconds. Report time excluded by the clamp separately from simulation time dropped by the catch-up cap. Both elapsed counters and the current plan come from the same service across the bridge.

Gameplay time advances at frame begin, even when no fixed tick runs. A 0.25-second frame advances gameplay elapsed/waits/tweens by 0.25 seconds, while a four-step simulation advances by approximately 0.0667 seconds. An ordinary wait uses the gameplay time at its creation as its deadline origin. Interpolation alpha describes simulation remainder; it is never added to gameplay elapsed. `Time.delta` remains fixed delta in fixed callbacks and gameplay delta in variable callbacks; add explicit simulation/real elapsed accessors so elapsed never silently changes timelines by phase.

Paused frames still capture input and run variable callbacks with gameplay delta zero. An editor Step at a frame boundary runs exactly one fixed sequence, advances simulation time and tick-based waits, and leaves gameplay/real elapsed unchanged. An explicit tick-clock tween/animation can therefore advance during Step; default gameplay-clock waits cannot. Scene replacement does not reset world time. A newly created world starts at zero. F1.2 audits every timing consumer, including test contexts and animation ownership; ordinary waits and tweens must lose the private coroutine clock.

## Frame decision

Start sequentially. Use ADR-0046 transactions for each gameplay callback, committing before the next callback. Each committed edit has a revision; a simulation result also has a completed tick. A failed data validation rolls back that callback, records its diagnostic, and subsequent scheduled callbacks see the last committed world. Native panic poisons the world and stops further execution. Script error continuation/lifecycle details remain F3.3; external effects remain F2.2.

| Boundary/phase | Work and visibility |
| --- | --- |
| Frame boundary | Finish prior scopes; activate successful prepared scenes in request order; apply lifecycle edits through the same transaction route; take the clock plan and input capture |
| Before each fixed tick | Publish that tick's input view; sequential host/fixed-system/behaviour callbacks commit before native simulation; pending edges consumed only by this tick |
| Simulation | Native systems run in their declared order using committed state; no concurrent gameplay write scope; complete physics/animation results and advance the completed tick |
| After physics | Deliver this tick's immutable contact/result data; callback edits commit immediately for later gameplay callbacks and the next fixed tick |
| Update | Once per host frame, including zero-tick frames; callbacks and gameplay-clock routine continuations commit sequentially |
| Late update | Once per frame after update; same persistent gameplay edit semantics |
| Publish | Build an immutable render frame from committed state, applicable simulation interpolation and explicit visual overrides; retain its resources until render consumers release them |

Input held state is available immediately. Fixed pressed/released edges wait for the next tick and are consumed exactly once; catch-up ticks see held state only. Variable callbacks see the host frame edges independently. A paused Step consumes buffered fixed edges once. This preserves the useful input contract in ADR-0008.

Ordinary transform edits always persist through the gameplay transaction. They do not change lanes according to whether a dynamic body is attached. A committed edit to a physics-owned pose is an explicit pending teleport/synchronization at the next simulation boundary; read-only physics queries/results still name their completed tick. F2.2 decides body-command precedence and effect failures before those commands migrate. Pose revisions newer than the completed simulation result render from the committed pose rather than blending it with obsolete interpolation history. F2.3 tests interpolation reset and subsequent physics feedback with actual bodies.

Provide a separate render-only override API for responsive camera/visual adjustments. It overrides extraction for the current frame, is invisible to gameplay transforms and physics, expires at the next frame begin, and never becomes next-tick state. Persisting a pose requires an ordinary gameplay edit. Initially these are per-entity geometry/camera adjustments with hierarchy/bounds validation, not a second writable component world. Exact descendant composition, conservative bounds and rejection of incompatible skinned/physical targets must be tested before exposing the API in F2.3/F6. The integer phase model proves ordering/expiry, not these geometry rules.

Out-of-frame explicit transactions commit immediately without stepping. Scene activation, Step, snapshot publication and hot reload require a boundary with no outstanding borrowed access. A host cannot start another frame while planned ticks or a gameplay scope remain open. Published frame memory remains immutable until its consumers release it; threaded handoff remains F4/F6 qualification.

## Readiness decision

A load request identifies the asset revision, dependency path, preparation domain, required/optional policy and eventual diagnostics. Required is the default for dependencies in a cooked scene closure; optional fallback must be explicitly declared by schema/import policy. Absence of an enabled module can remove its domain requirement; it cannot conceal a dependency required by another enabled consumer. Unknown required payload types fail with actionable diagnostics.

CPU decode, simulation preparation, and GPU readiness are separate states. Scene activation requires successful CPU and simulation preparation of every required dependency. It does not require a GPU, so headless activation is supported. GPU readiness is keyed by asset revision/subresource and device generation. First-visible-frame readiness is a separate presentation barrier: hosts may wait for it before revealing a scene; ordinary rendering uses declared placeholders/skips while scheduled upload proceeds. A GPU error is observable and can fail that presentation barrier without falsely marking CPU/simulation preparation failed.

Any required preparation error makes the request failed, with original asset/path/domain diagnostics preserved. It cannot activate or become successful from a late completion. Optional failure emits diagnostics and uses its declared fallback. Retry creates a fresh request/revision. Progress is work completed, including failures; reaching 1 is not readiness. Public `isReady` means successfully prepared; expose terminal/failed state separately and complete failed loads with errors without trying to activate them.

Prepare and validate the incoming scene's data before replacing the old scene. Failed required preparation or invalid scene data leaves the old scene playable. Successful data activation transfers ownership at the boundary; lifecycle callbacks then use their own transactions. A rejected lifecycle callback does not undo the already activated scene or prior callback commits. F5/F7 must verify this separation through actual native/SDK scene activation.

Cancel before activation invalidates the request, prevents late worker publication/activation and completes it as cancelled. Cancellation releases that consumer's retention; another consumer of a deduplicated job can still finish. Workers may finish a bounded noninterruptible operation before releasing their own token. Unloading an activated scene is a separate operation, not cancellation of its old ticket.

Retain the closure while preparing. On successful activation transfer its ownership token to the scene without an eviction gap. Persistent entities and other scenes hold their own required resources; published frames retain CPU/GPU resources through their last render fence/consumer. Failure/cancellation/unload releases only the owning consumer's tokens. Unreferenced resources become eviction candidates under F5 policy; do not immediately free resources pinned by an in-flight frame. Device loss retires that device generation's GPU resources/readiness and triggers scheduled reconstruction from retained CPU data without discarding gameplay readiness.

Normal draw preparation does not perform synchronous cache I/O/decode. Explicit exceptional blocking policies must be observable. F5 selects deduplication keys, queue priorities, cancellation granularity, retry policy and memory/upload budgets; F6 selects device recovery/fence implementation and spatial/render scheduling. No numerical budget or render graph is accepted here.

## Migration and remaining gates

This replaces the identity/clock/frame design questions in ADR-0043 and the readiness questions in ADR-0045. It supersedes ADR-0008's implicit presentation routing and phase-dependent elapsed-time interpretation for the replacement implementation; the old API still behaves as its source/tests describe until migrated. It amends ADR-0038's completion/readiness distinction and dependency failure policy. ADR-0046 remains authoritative for pure-data transactions.

Do not mark production tasks complete from the contract model. F1.1 must migrate every handle carrier and exercise actual native worlds; F1.2 must drive actual waits/tweens through the canonical clock. F2.3 integrates actual host phases, interpolation, physics snapshots, scene lifecycle and render overrides. F5/F6 integrate real corrupt-cache, cancellation/unload, allocation/residency and device-loss tests. General registry/package delivery, geometry/effect policy, editor/animation/AOT qualification and unfinished baseline checks remain in their existing roadmap destinations. F0 as a whole has not passed.


## Identity implementation evidence (2026-09-30)

The live ECS, native ABI, generated wire records, Dart SDK, authoring bridge,
editor and Apple selection boundary now use the 128-bit reference. See the
[migration report](../design/foundation-identity-migration.md) for regression
coverage, measurements and remaining gates. This is working-tree evidence,
not a release certification.

[ADR-0050](ADR-0050-process-lifetime-identity-authority.md) implements the
process-lifetime host authority. The host retains its opaque token and attaches
it to every additional/replacement module before use. The
[authority qualification report](../design/foundation-identity-authority.md)
records independent-engine allocation, rejected authority switching, and
library-close/replacement tests. Modules exchanging references must perform
this explicit handoff; independently bootstrapping each module is unsupported.
The old queued-command temporary encoding remains transitional until F2
introduces scope-tagged transaction references; live lookup rejects it.
Clocks, transactional gameplay, frame order and readiness have not migrated
from the contracts merely because identity has migrated.


## Clock implementation evidence (2026-09-30)

The live native clock now owns real/gameplay/simulation timelines; Dart timed
routines use absolute deadlines on the selected timeline. Editor Step advances
one fixed sequence, and Flutter forwards real delta without a second clamp.
See the [clock migration](../design/foundation-clock-migration.md) for ABI 3,
consumer audit, actual-world/host regression evidence and host-suspension policy.
This implements timing; it does not certify F2 frame transactions, transform
interpolation/reset, readiness or the overall F0/F1 release gates.
