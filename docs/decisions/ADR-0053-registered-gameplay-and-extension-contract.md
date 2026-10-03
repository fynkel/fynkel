# ADR-0053: Registered gameplay data, owned bulk access and sequential systems

**Date:** 2026-10-02

**Status:** Accepted. Extends ADR-0052's native slice to F3's complete vertical slice.

**Related:** ADR-0044, ADR-0046, ADR-0047, ADR-0052; F3.1–F3.5.

## One data authority and immutable schemas

Stable namespaced component IDs, positive versions and ordered fields identify data. The world owns registrations. Native codecs store their T directly in the existing ECS. Runtime-defined layouts store an owned map of registered records in an ECS component on each entity. Both use the same gameplay journal; neither keeps an authoritative Dart sidecar. This layout permits ordinary Dart schemas without rebuilding native code. It is an owned-record implementation, not a columnar archetype implementation or zero-copy API.

The supported flat kinds are finite f64, i64, bool, UTF-8 text, asset UUID plus subresource, and nullable engine-managed object references. Empty markers, vectors, collections and nested records are rejected. Object defaults are null. Dynamic layouts resolve pending IDs at commit, remap internal clone references, preserve external live references and read destroyed targets as null. Native T codecs currently reject object-reference fields: silently retaining unresolved IDs or running a fallible codec during commit would violate the transaction contract. Runtime-defined layouts are the supported managed-reference route.

Components are pure data. Behaviour execution state, coroutines and runtime services remain separately owned runtime machinery. A returned Dart record is a value copy; mutation needs an explicit write. `addData` validates the declaration and decodes defaults inside the owned gameplay guard; catching either failure cannot allow other writes in that edit to commit. A component codec must be pure and round-trip its values. Native code is trusted code, not a sandbox.

Exact same-schema registration is idempotent, including a Dart declaration adopting an already installed native codec. Conflicts cannot replace live storage. Native installation precedes host metadata registration. Re-registering an equivalent Dart codec is allowed; changing schema/version requires a new world and explicit authored migration. There is no implicit migration or live reinterpretation of old fields.

## Authored and editor boundaries

`@FynkelComponent` generates ComponentType codecs, registration and schema metadata. Exposed field discovery visits the class, its mixins from right to left, and then its superclass; this shared traversal also handles behaviour/data declarations and nested records. Mixin `on` constraints are not field implementations. `ObjectSpec.data` and the project writer use that declaration. The cooker classifies registered data separately from behaviours. Scene/prefab activation resolves forward references and validates versions and layouts through the same journal. An unknown required runtime registration rejects activation while retaining the old scene. Unknown authored JSON remains preservable by the document layer; preservation is not permission to run it as valid component data.

Generated schemas live under `schemas/registered` and carry registration metadata. The editor creates dynamic metadata-backed storage without invoking game constructors or native extension code. Generic Inspector transactions, preview synchronization, undo, Play inspection and live component changes use those schemas and the native journal. There is no Health-specific editor or core branch. Play inspection includes custom component versions. F7 still owns broader package discovery and migration of all examples and workflows.

## Bulk ownership and invalidation

Single-object typed reads/writes and bulk access address the same registered storage. Bulk queries return owned snapshots with frozen, sorted entity membership. There is no borrowed native pointer to retain across an FFI call. A batch belongs to one edit in one world. An intervening registered write/removal or entity spawn/destruction invalidates it. Applying a batch validates every row; one invalid row rejects the whole edit, including unrelated staged changes. Batch tokens disappear at edit end/abort and cannot be adopted by another callback.

A read-only Dart population query uses one native read. The update loop uses one native query and one native apply. Validated population handles avoid per-entity liveness calls; accesses the user's callback explicitly makes can still add calls. The guarantee concerns the bulk operation itself, not arbitrary user code. Snapshot copies and native preparation allocate. F3 measures that cost rather than claiming zero allocation or production throughput. The gate report records JIT, AOT, copied bytes, VM-counter limitations and isolated native allocator traffic. Large-world performance remains an explicit F0/F4/F8 qualification target.

## Scheduling and behaviour integration

Native built-in stages are registrations in a compiled graph, retaining their existing execution order. Packages register systems without adding a core catalogue branch. Native extension callbacks run in Simulation or PostPhysics; PostPhysics follows propagation and precedes final settling. Input, Physics and Audio are reserved engine boundaries. Host callbacks have fixedUpdate, afterPhysics, update and lateUpdate phases. The native tick executes between host fixedUpdate and afterPhysics. This is a hierarchical schedule, not arbitrary Dart/native callback interleaving. Cross-boundary ordering uses those fixed boundaries; dependency names must belong to the relevant schedule.

Both schedulers validate names, dependencies, backwards phase edges and cycles before atomically installing a batch. Lexical IDs break otherwise unconstrained ties. Component read/write sets and structural flags are audit declarations; built-ins conservatively declare wildcard access. They are not runtime capabilities or an analysis sufficient for parallel execution. All execution is sequential. Native systems declare module requirements, and explicit dependencies inherit them; disabled modules cannot silently satisfy a required dependency.

Each custom callback owns one existing gameplay transaction. It may create and destroy entities within that boundary. Rejection rolls back its edits before the next system runs. A normal callback exception/panic is reported by system name; publication failure poisons the world and stops execution. Named timing/failure records are exposed. Built-in stage timings remain available for existing tooling.

Behaviours keep their existing lifecycle, contact and coroutine implementation under registered host phase nodes. Generated metadata records the effective overridden callbacks, including inheritance and mixins; known-unused per-frame dispatch is skipped. Manual registrations without metadata remain conservative. Registry revisions invalidate type/reference caches even for same-count replacements; generated Dart type metadata avoids constructing behaviours merely to identify their class. Systems and behaviours receive the same host phase tick captured by the frame driver. FixedUpdate and afterPhysics share the zero-based label of that simulation interval (0 for the first interval); update and lateUpdate use the completed tick count captured at presentation phase entry. The scheduler receives this label explicitly rather than rereading the native clock or applying an offset. Native tick completion counters retain their existing meaning.

## Native package route and compatibility

Extension API 1 is a build-time Rust source API. A game host links a crate, uses the host's pinned engine revision/lock/toolchain, and installs its manifest/component/system declarations before gameplay. Runtime ABI is 5; generated C/Dart bindings and runtime locks change together. The existing built-in schema fingerprint does not change. Version checks do not imply stable Rust binary layout. Dynamic loading and automatic SDK package linking are not this route.

Installation validates namespace, API/ABI and required modules. Registration has no mutable world handle. A failed component or schedule batch leaves no partial installation; manifest requirements also apply to its systems. Editor worlds reject installation before calling package code. The external `examples/native_combat` crate and headless host exercise the route with core default features disabled. Ordinary Dart games need no Rust.

## Lifecycle participation correction (F5 qualification)

Generated callback metadata covers only periodic dispatch (`fixedUpdate`,
`afterPhysics`, `update`, `lateUpdate`). It must never suppress `start` or
other lifecycle notifications. An event-only behaviour with an empty
periodic set still starts exactly once when active. The SDK regression
`empty periodic participation still starts an active event-only behaviour
exactly once` preserves this boundary; the reference and code-only games
exercise it through their collectible behaviours.
