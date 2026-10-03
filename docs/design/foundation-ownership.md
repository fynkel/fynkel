# Foundation ownership inventory

Code inspection and diagnostic baseline, September 30, 2026. This is evidence for [F0](../foundation-roadmap.md), not an adopted replacement architecture. Status lives only in the roadmap. The runtime inspected is revision `87e3dec8afa33c2c439c6fe848a7f0599cc512aa` with the working-tree changes recorded in the [baseline report](../foundation-baseline.md).

## Current ownership reconciliation (F8, 2026-10-03)

The tables below retain the original findings, not present-tense defects. The
replacement owners and verification are:

| Domain | Current authority | Evidence |
| --- | --- | --- |
| Identity, clocks, affine transforms and bounds | Engine world identities; named real/scaled/simulation clocks; scene affine matrices; conservative bounds | F1 gate and permanent identity/clock/geometry regressions |
| Gameplay values, hierarchy, liveness and effects | `world/gameplay.rs` transaction journal; SDK `src/gameplay.dart` adapter; effects published at their declared boundary | F2 gate; current workspace/SDK suites |
| Custom components and systems | Registered schemas and ECS storage; owned query batches; compiled sequential system graph | F3 gate; generated combat slice; F8 native/JIT/AOT population measurements |
| Native subsystem state and outputs | Tracked native mutations, subsystem-owned execution state, incremental synchronization; owned external query results | F4 gate; native scale/static/sparse/churn/sleeping measurements. Trusted Rust `ecs_mut` remains an integration surface, not the ordinary gameplay API. |
| Asset activation, bindings and residency | Prepared resources, revision-aware bounded loading/upload, stable AnimationBinding keys | F5 gate and regression suites; F8 cold activation and 50 unload cycles |
| Rendering | Owned extracted snapshots, maintained membership, conservative visibility, spatial light selection and explicit GPU lifetime | F6 gate; F8 extraction/light/transparency captures and physical-device smoke |
| Code/editor/API | `gameplay.dart` and the inclusive `advanced.dart`; recipes and forest creation use the same transaction; authoring documents keep their own revision/undo authority | F7 gate and follow-up review; F8 removes the old umbrella and checks all maintained examples/generators |
| Persistence and release scope | Versioned game-owned saves with saved IDs remapped to new runtime identities; no general execution-snapshot claim | ADR-0059, combat save regressions and F8 qualification |

Original reproduced correctness findings now have permanent F1–F7 regression
coverage. Original performance hypotheses now have named F4/F6/F8 workloads;
50,000 fully rewritten registered rows remain an explicit performance limit,
not a passing 60Hz target. Physical iOS smoke and local macOS export are separate
from the unavailable second-Mac check. See the F8 gate for current results and
limitations; do not treat the historical table below as a release verdict.

The native host now transfers a stable `Box<World>` across owned edit sessions
([ADR-0060](../decisions/ADR-0060-stable-world-ownership-in-bridge-transactions.md)).
This removes repeated movement of subsystem storage while retaining one exclusive
owner and the existing transaction boundaries.

## September 30 owners and mutation routes (historical)

| Domain | Current owner and source | Mutation and derived state | Foundation implication |
| --- | --- | --- | --- |
| Entity identity and native components | [ECS facade](../../crates/fynkel_ecs/src/lib.rs), backed by hecs; [core World](../../crates/fynkel_core/src/world.rs) | World/generation/slot handles; core command catalogue validates built-in operations. Public mutable ECS access can bypass core bookkeeping. | Keep efficient storage; establish world allocation and a supported mutation boundary. ECS is already real, but custom gameplay is outside its component catalogue. |
| Hierarchy, activity, transforms | [scene](../../crates/fynkel_scene/src/lib.rs) | Local TRS, Parent/Children, local inactivity and inherited inactivity; derived world matrices and dirty state. Scene helpers maintain invariants. | One matrix-based geometry contract and tracked hierarchy mutations; distinguish authored values from derived values. |
| Tick and phase state | [clock](../../crates/fynkel_core/src/clock.rs), [stages](../../crates/fynkel_core/src/world/systems.rs), [scratch/state](../../crates/fynkel_core/src/world/state.rs) | Accumulator, capped catch-up, staged commands, temporary spawn map, teleport/override maps, dirty roots, animation caches and subsystem scratch. Sixteen fixed native stages. | Name clocks and commit boundaries; extend scheduling without adding a new central catalogue branch per game feature. |
| C ABI lifetime | [native](../../crates/fynkel_native/src/lib.rs) | Runtime and world pointers; world owns core, assets/residency, load tickets, prefabs and reusable output vectors. Panic guards poison a world. Runtime counts created worlds but does not allocate distinct world IDs. | Preserve panic containment; specify disposal and view lifetimes, allocate identity, and prevent stale or foreign access. |
| Gameplay facade and command visibility | [Dart World](../../packages/fynkel/lib/src/world.dart), [objects](../../packages/fynkel/lib/src/object.dart) | Simulation/presentation batches and targets; pending objects, touched overlays, subscriptions, snapshots, entity table and lookup indexes. Reads combine these mechanisms. | Replace undocumented freshness differences with one logical view. The large World facade currently coordinates unrelated owners. |
| Bridge memory and cached reads | [native bridge](../../packages/fynkel/lib/src/native/bridge.dart), [wire buffers](../../packages/fynkel/lib/src/wire/buffers.dart) | Native typed-data views borrow reusable output storage; address/length caching is not an ownership transfer. Retained component bytes are copied; entity-table bytes are compared and indexed. | Document invalidation per query family and mutating call; publish revisions/deltas rather than rediscover unchanged structure. |
| Behaviour execution | [behaviours](../../packages/fynkel/lib/src/behaviour.dart) | Dart instances own fields, lifecycle, coroutine/invoke state and a private elapsed clock; callbacks sorted by order/sequence. Generated metadata supports authored fields and references. | Retain friendly local scripts; separate execution state from registered gameplay data and make both follow world access rules. |
| Shared authored data and HUD | [data assets](../../packages/fynkel/lib/src/data_asset.dart), [model](../../packages/fynkel/lib/src/model.dart) | Shared Dart data-asset instances and synchronous model listeners. | These are legitimate separate domains. Do not force shared configuration, UI models or every service into entity storage. |
| Physics | [physics adapter](../../crates/fynkel_physics/src/lib.rs), core PhysicsStep | Rapier body/collider entries synchronized from ECS; pending commands/root motion; step emits events and writes poses back to ECS. Membership traversal/query preparation precedes the step. | Explicit configuration, simulation and published-result ownership; measure synchronization independently from the solver. |
| Animation | [core animation](../../crates/fynkel_core/src/world/animation.rs), [animation library](../../crates/fynkel_animation/src/lib.rs) | Clip/controller/skin caches and runtime state; poses write local transforms before physics. Name-based target lookup can select the first match. | Stable binding identity and explicit pose writers; preserve the current ordering where required. |
| Audio | [core audio](../../crates/fynkel_core/src/world/audio.rs), [audio library](../../crates/fynkel_audio/src/lib.rs) | Tick-side voice/clip state sends commands to the mixer; callback tables/ring and decoder workers have distinct owners. | Preserve the real-time boundary and no-allocation callback guarantees. A general scheduler must not move arbitrary ECS work into the audio callback. |
| CPU assets and asynchronous loading | [asset store](../../crates/fynkel_assets/src/lib.rs), native pending loads | Shared locked store, typed Arc caches, retain counts, warm queue, generations and cancellation flags. Worker owns store access, not mutable core world state. Preload discards some dependency errors. | Preserve off-thread decoding; required dependency failures must affect readiness. Separate retain/residency from scene lifetime. |
| Extraction and GPU state | [core](../../crates/fynkel_core/src/world.rs), [renderer](../../crates/fynkel_render/src/lib.rs), [preparation](../../crates/fynkel_render/src/prepare.rs), [residency](../../crates/fynkel_render/src/residency.rs) | Extraction scans entities and builds per-frame vectors/maps. Viewport renderer owns GPU caches and uploads; draw preparation can resolve/load missing resources synchronously. | Measure scan/allocation and first-use costs; maintain render membership, explicit publication and preparation budgets. |
| Authoring and live Play | [authoring session](../../packages/fynkel_authoring/lib/src/session.dart), [live Play](../../apps/editor/lib/src/play/live_play.dart) | Authored JSON documents have transactions, inverses and revisions; edit preview and compiled Dart Play use different runtime entry points. | Documents are authored authority, not a second live simulation. Inspector/Play edits need the same validated component contract. |
| Type/schema generation | [contracts](../../crates/fynkel_contracts/src/lib.rs), [schema derive](../../crates/fynkel_schema_derive/src/lib.rs), [generator](../../packages/fynkel_generator) | Built-in Rust catalogue generates wire/schema/Dart artifacts and fingerprint; Dart generator creates behaviour/data metadata separately. | Preserve generated consistency; custom component identity/schema must bridge both sides without hand-maintained parallel registries. |
| Host and modules | [Apple platform](../../platform/apple), [Flutter integration](../../packages/fynkel_flutter), crate feature manifests | Apple host drives viewport lifecycle; native feature inclusion and world runtime module flags have different purposes. | Separate module build capability from per-world activation. Portable compilation does not establish a supported non-Apple shipping host. |

## Current frame and dependencies

`World.frame` plans capped fixed ticks, runs each tick's Dart fixed callbacks, sends the simulation batch, steps native, then dispatches contacts/after-physics callbacks. After the tick loop it runs update/coroutines and late update, then flushes presentation commands. Zero-tick frames still have frame callbacks. Some pending structural writes must wait for a simulation commit while selected touched fields already read back through overlays.

Native tick order is Characters → RecordPrevious → Commands → Animation → Rotators → Oscillators → Lifetimes → Emitters → PhysicsStep → Propagate → CameraRigs → SettleRigs → Followers → Billboards → Settle → Audio. This is useful ordering evidence, not proof that all stages should survive unchanged. F0.3 must make equivalent required dependencies explicit in a registered sequential scheduler.

Dependency direction to retain: contracts/wire and ECS/scene underpin core; core orchestrates simulation modules; native exposes core and resource services; renderer consumes extracted state and assets; platform owns surfaces/device integration; Dart facade wraps native while hosting gameplay execution. Authoring/import/cook tools belong outside the shipped runtime dependency graph. Check stripped builds with `tools/check_runtime_deps.sh`. Runtime resources, authoring documents and callback execution should have named owners even if they remain coordinated by World.

## Findings and evidence routes

"Reproduced" means an executable observation confirms the disagreement. "Code evidence" means an implementation property is visible but its gameplay impact or cost still needs a workload. Performance claims remain unmeasured until a capture includes workload and build mode.

| Finding and roadmap destination | Current source | Evidence or next experiment |
| --- | --- | --- |
| Cross-world and recreated-world handle alias, F1.1 | Core default WorldId(1); native world creation | **Reproduced:** diagnostic's two worlds return identical handles; foreign handle selects the second world's object; destruction/recreation aliases again. |
| Time accounting divergence, F1.2 | Core clock; Dart frame delta; behaviour private clock | **Reproduced:** .25s frame, four fixed steps; wait(.1) resumes while public elapsed advances .08333s. Choosing real/scaled/simulation time remains an ADR decision. |
| Nested transform disagreement, F1.3 | Scene matrix composition; Dart Transform composition | **Reproduced:** scale(2,1,1), child Z45°, grandchild offset(1,0,0); completed-tick positions differ by .7071m. |
| Bounds under shear, F1.4 | [culling](../../crates/fynkel_render/src/culling.rs) | Code evidence: maximum column length is not generally a conservative sphere multiplier under shear. Add a geometry containment/culling-boundary reproduction. |
| Name/activity/structural freshness, F2 | Dart World/object overlays and native queries | Code evidence: different read routes use different caches. Prototype a read/phase matrix covering same-frame rename/find, hierarchy activity, spawn and destroy. |
| Closed component and stage catalogue, F3 | Core components/catalogue/systems; Dart metadata | Code evidence: game-specific bulk components/systems lack a general registration route. F0.3 slice must demonstrate one custom data type and bulk system without new catalogue branches. |
| Mutable ECS escape and broad World ownership, F4.1 | Core ecs_mut; Dart World private state | Code evidence: direct mutation can bypass command bookkeeping. Inventory every supported public mutation route before replacement. |
| Full table/extraction/dirty/membership work, F4.2–F4.3/F6.1 | Dart entity table, core extract/scene propagation, physics synchronization | Unmeasured cost: release headless capture is an initial anchor, not a sparse-change or bridge benchmark. Measure 1k/10k/100k static, sparse and dense worlds with separate phase counters. |
| Borrowed view lifetime, F4.4 | Native output vectors and Dart bridge | Code evidence: reusable backing storage. Exercise query repetition/growth, world disposal, scene replacement and panic poisoning before threading changes. |
| Hidden dependency failures and draw-time loads, F5.1–F5.3 | Asset preload; renderer prepare/upload/residency | Code evidence: discarded load results; asset-sized warm operations and first-use fallback. Inject missing/corrupt dependencies and capture cold/warm frame cost and memory pressure. |
| Name-based animation binding, F5.4 | Core animation | Code evidence: first matching name; test duplicate names, rename and reimport before selecting stable imported/authored IDs. |
| Light selection and GPU lifetimes, F6.2–F6.4 | Renderer lights/culling/targets/passes; Apple viewport ring | Code evidence: 64-light pool/eight per object, per-object assignment. Measure dense-light scenes, boundary culling, resize/reload and preparation/GPU cost. Do not infer scale from larger constants. |
| Low-level exports and one-use descriptions, F7.1–F7.2 | [barrel](../../packages/fynkel/lib/gameplay.dart), [ObjectSpec](../../packages/fynkel/lib/src/object_spec.dart) | Code evidence: bridge APIs share gameplay barrel; descriptions can retain behaviour instances. Test building the same description twice and separate advanced API deliberately. |
| Schema drift across examples/workflows, F7.3–F7.4 | Editor project tests and reference assets | **Baseline failure:** reference hero prefab format5 against current7, plus empty-string skeleton references. Record before migrating; preserve authored content. |
| Save/replay and shipping claims, F8 | [recording](../../packages/fynkel/lib/src/recording.dart), host manifests | Code evidence: input recording is not a complete gameplay-state checkpoint or rollback proof; Apple host is the concrete implementation. Define persistence separately and qualify physical-device shipping paths. |

The initial diagnostic is [foundation_probe.dart](../../packages/fynkel/tool/foundation_probe.dart). It reports present behavior without asserting that these defects must remain. F1 must replace each agreed contract with a regression that fails on the old implementation and passes on the fix. Performance, platform and failure-injection experiments above remain outstanding F0/F8 evidence, not completed qualification.
