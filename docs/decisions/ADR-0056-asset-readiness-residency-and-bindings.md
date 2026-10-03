# ADR-0056: Asset readiness, residency and animation identity

Status: Accepted (F5). Extends ADR-0038 and ADR-0055. The F2 transaction remains the sole gameplay publication boundary.

## Readiness and publication

A successful scene load requires its complete cooked dependency closure to decode and its simulation resources to prepare successfully. The report declares `readiness.cpu = ready`, `simulation = ready`, `gpu = deferred`, and `required = [cpu, simulation]`. A missing/corrupt required dependency fails preparation with asset attribution; it cannot become Ready or replace the live scene. Cancellation stops between bounded object/chunk operations and releases preparation ownership. Runtime loading opens cooked cache files only.

GPU readiness is a separate renderer observation. Normal draw preparation requests CPU payloads without waiting, then admits available GPU work within the frame's limits. Missing/pending geometry and materials use the renderer's explicit fallback policy; errors remain observable. `assets_ready` never treats a failed fallback as successful readiness. `prepare_ready` is the deliberate blocking loading/export boundary, with a timeout and failure result. An idle editor keeps requesting frames while assets are pending; capture waits for readiness and refuses an incomplete export. Apple viewport ABI 13 exposes these boundaries without changing normal render into a blocking operation.

Scene activation is an atomic host-thread operation bounded by existing world/entity/component capacity, not a time-sliced partially visible scene. CPU decode runs on the preparation worker for async loads. Prepared simulation resources publish only after the same core preflight that admits entity data. Prefab instantiation uses this plan too; no permanent native prepared-prefab cache remains. Runtime asset edits reserve their entire final memory requirement before gameplay commit. A rejected reservation aborts the entity/resource edit together. Publication after successful commit performs no asset I/O or fallible resource admission.

## Work bounds

Each asset store has one preparation worker and admits at most 64 outstanding jobs. Requests deduplicate by asset/subasset and payload kind. Visible requests outrank scene preparation, which outranks prefetch; equal priorities remain FIFO. Running jobs are not preempted, and a scene job can prepare a whole bounded closure. Existing cooked reader limits cap an individual chunk at 256 MiB. Cancellation is cooperative between reads/decode operations; this is not a hard latency guarantee for one large decode.

The renderer defaults to 16 asset operations and 32 MiB uploaded per prepare call. Visible work and warming share those counters, including shader compilation and uniform rewrites. A resource larger than the configured indivisible upload limit fails explicitly; callers must raise the limit or recook smaller assets. The warming time budget is soft between operations. Driver compilation, first-use pipelines and fixed frame resources can take longer than the soft budget. No hard millisecond bound or production frame-rate claim follows from these limits.

Requests and payload publication carry generation checks. Clear/replacement cannot let a late worker resurrect stale data. Material/mesh change histories compact at 4,096 entries and publish an epoch reset so renderer caches rebuild safely.

## Memory and ownership

Decoded cooked cache payloads, explicitly owned runtime payloads and renderer GPU assets each default to separate 512 MiB budgets. CPU admission includes mesh/skin/animation/audio/image payloads, controller and prefab documents, material/shader data and decoded metadata. Unowned cache entries may be evicted; pinned entries force an attributed admission error. Material representation replacement removes the old charge atomically, with rollback on refusal. Budget reduction cannot silently discard pinned resources.

Runtime payloads survive cooked-cache clearing and cache-store replacement until explicitly removed. Store transfer is fallible and happens before scene/editor publication; it preserves the source runtime budget and rejects conflicting reservations. Replacement/deletion are included in transaction admission, allowing an equal-cost replacement at a full budget.

Native ownership includes loaded scenes, live entity resource components, registered asset-reference fields, prepared edits, runtime prefab instances, pending effects, animation fades/one-shots and active audio voices. New owners acquire before old owners release. Simulation registries are pruned after their owners disappear, including removed runtime clips; audio remains registered until queued/active playback no longer needs it. Renderer ownership follows visible extracts and their material dependencies, including extra worlds. GPU eviction never silently discards current extract ownership; insufficient budget produces an explicit fallback/error.

These are **resource-owner budgets**, not a cap on process RSS. They exclude allocator/container overhead, immutable cache indexes, independent external Arc owners, core playback copies, driver/pipeline allocations and fixed frame attachments. GPU estimates are accounting estimates, not driver heap measurements. External Rust callers retaining payloads own that memory. Qualification checks cache bytes and simulation registrations returning to baseline, with fixed GPU frame resources remaining allocated.

## Animation binding and migration

`fynkel.AnimationBinding` (builtin component 28) supplies a stable string key. Empty is unbound. Clips, skin joints, controller layer roots, mask nodes and IK nodes resolve by this key within their animator/skeleton hierarchy. Missing or duplicate keys produce `ANIMATION_BINDING_UNAVAILABLE`; no name-based fallback exists. `Name` is display/search metadata and may duplicate or change without retargeting playback. Clones preserve keys and resolve within their separate instance scopes. Binding/hierarchy changes invalidate derived target, rest-pose, mask and IK caches.

Imported glTF/FBX keys are `<asset UUID>/node/<source node index>`. Authored rigs may use persistent UUID strings or other explicit stable keys. Imported prefab, skin and clip payloads are regenerated coherently. Source node-table reordering can change these keys: recook the model/rig and review/regenerate **external authored** controller roots, masks and IK references. We do not claim persistent source IDs across arbitrary exporter reindexing. Offline rig remapping may match unique display names to compile destination keys; ambiguous source/destination names are rejected rather than choosing the first match.

Migration is deliberate and breaking:

1. Rebuild native runtime, SDK, renderer/viewport and generated bindings together (component fingerprint `0x65d9f39b4aa79409`, viewport ABI 13).
2. Run `fynkel project lock`, `fynkel codegen --project .`, and `fynkel cook --force`. Cooked cache version is 12, intermediate format 7, glTF importer 9, FBX importer 5. `ANIM`/`SKIN` carry the `BND2` header; old payloads fail explicitly.
3. Imported prefab instances receive bindings automatically. For manually authored imported rigs, assign the matching source-node keys to their joint entities. For entirely authored rigs, choose persistent keys and use those in `ClipBuilder` channels.
4. Replace controller `root`, mask `node`, and IK joint fields with binding keys. The editor shows display names but persists keys in mask/IK picks. Search APIs such as `findChild('head')` still use names; read that object's `AnimationBinding.key` when constructing a clip.
5. Keep authored entity/component UUIDs and unknown data. This migration never authorizes deleting a project or silently guessing an ambiguous rig mapping.

The reference game, Showcase, SDK fixtures and wire fixtures are migrated with the engine. Broader editor authoring ergonomics remain F7; render extraction, visibility and driver/resource lifetime scaling remain F6; production/platform qualification remains F8.
