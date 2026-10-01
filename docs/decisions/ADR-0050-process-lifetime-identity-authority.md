# ADR-0050: Process-lifetime identity authority

**Status:** Accepted and implemented; qualification in the linked report.  
**Date:** 2026-09-30  
**Authority:** User authorization to finish F1.1 before F2.  
**Related:** ADR-0047; roadmap F1.1/F7.  
**Evidence:** [Authority qualification](../design/foundation-identity-authority.md).

## Context

The 128-bit identity migration prevented reuse across worlds and runtimes inside one loaded engine. Its namespace counter was a library static. Independently loaded engine images could start at the same world ID, and replacing an image could reset its counter while old references survived.

## Decision

Keep one shared, monotonic u64 counter in process-lifetime storage independent of engine code. The first engine bootstraps the opaque `IdentityAuthority`; the host retains its pointer and attaches it to every additional/replacement engine module before creating runtimes, authoring sessions or worlds. All existing ECS allocation routes read the module's selected authority.

The authority's internal ABI 1 is a version tag plus an aligned atomic u64 counter. It contains no vtable, callback, lock implementation pointer or owning module reference. Selecting an authority uses a thread-safe one-time cell. Reattaching the same pointer succeeds; selecting a different or incompatible authority fails. Selection cannot be changed after bootstrap, even before the first world is allocated.

The initial authority allocation intentionally remains until process exit. There is no free/reset API: entity references can outlive the worlds and modules that created them. This costs one 16-byte payload plus allocator overhead per bootstrapped authority. Correct multi-module hosts bootstrap once and attach subsequent modules to it. Repeated world/runtime creation does not allocate new authorities. Allocator exhaustion remains permanent and never wraps.

Add two C entry points: `fynkel_identity_authority` to bootstrap/read the selected authority, and `fynkel_identity_attach` to attach host-retained storage. This is an additive runtime ABI 3 extension; existing entity and wire layouts stay unchanged. The SDK exposes `NativeLibrary.identityAuthority` and an optional authority argument to `NativeLibrary.open` for embedding hosts. Normal single-code-asset applications keep the same open call.

## Host obligations and boundaries

Attachment is the first identity operation on an additional module. Calling its authority getter, creating its runtime, or allocating a world first selects an independent authority; late attachment is rejected. Modules that exchange raw references must use the same host-retained authority. The API cannot recover the intended namespace domain if a host forgets the token and independently bootstraps every image.

The pointer belongs to this process. Do not serialize it, send it to another process, edit its storage, or free it. Hosts must destroy a module's runtime/world objects before unloading its code; retaining an entity value does not retain its old world. The authority enables safe rejection of those old values in replacement worlds, not migration of live object pointers.

The ordinary SDK/editor loads one native code asset and does not introduce an automatic dynamic-module reload manager here. Embedders doing replacement own the bootstrap/handoff sequence. Platform-specific packaging, live gameplay-state migration and user-facing hot-reload orchestration remain F7. Cross-process communication uses authored IDs/import mappings.

## Qualification

The gate uses independent copies of the native cdylib, checks shared allocation and foreign lookup/destruction rejection, rejects incompatible late attachment, closes both originating library handles and creates ten replacement images using only the host-retained authority pointer. A separate independently bootstrapped image must have a different authority address, proving that the loader did not collapse the test copies into one engine.

Unit tests cover concurrent allocation, sticky attachment, incompatible version rejection and permanent exhaustion. Existing generation retirement, 300 world recreations, SDK, authoring, editor and full workspace tests remain part of qualification. The report distinguishes this machine's loader evidence from future platform release gates.
