# ADR-0045: Asset readiness and render resources

**Status:** Accepted readiness/ownership contract through ADR-0047; detailed scheduling, budgets and render design remain proposed for F5/F6. Shipping migration pending.  
**Date:** 2026-09-30  
**Related:** ADR-0042, ADR-0006, ADR-0012, ADR-0029, ADR-0030, ADR-0032, ADR-0033, ADR-0038, ADR-0040, ADR-0041; roadmap F5, F6.

## Context

Fynkel already cooks dependency closures, retains CPU resources, reconciles GPU residency, prepares scenes in background threads, and warms GPU assets within a budget. These mechanisms are worth preserving. Dependency preloading can discard errors, draw preparation can load assets on demand, and scene-level retention alone does not describe fine-grained streaming or budget pressure. Animation binding by first matching name is ambiguous, and render extraction reconstructs broad world state each frame.

## Accepted readiness and planned resource work

[ADR-0047](ADR-0047-identity-clocks-frame-order-and-readiness.md) resolves required/optional dependencies, CPU/simulation versus per-device GPU readiness, failure/cancellation, boundary activation and retention transfer. Its exact rules supersede the general readiness proposal below. [Executable contract checks](../design/foundation-contract-checks.md) cover the candidate state transitions and ownership model; real store/renderer migration remains F5/F6.

Define resource states with explicit requests, CPU readiness, simulation preparation, GPU readiness per device, failure, and eviction. Readiness is device/domain-specific: a headless scene never requires a GPU. A scene activation policy identifies required dependencies and allowed optional fallbacks. Required preparation errors propagate into activation diagnostics instead of being discarded.

Schedule decode/preparation/upload work with deduplicated requests, cancellation, priorities, and bounded normal-frame work. Rendering consumes available resource state; a synchronous fallback is an explicit exceptional policy with observable cost. A single large upload may exceed a time budget unless split; select byte and time budgets with this limitation recorded.

Retain ownership tokens where useful and introduce CPU/GPU budgets and resource-level residency where workloads require it. Live entities, persistent objects, runtime resources, cached prefabs, and in-flight frames have explicit retention/release policies. Separate authored asset identity from device resource identity and generation.

Bind animation targets and skeleton joints by stable imported indices or authored identifiers, with remapping for changed imports. Names remain display and search data. Render extraction uses maintained membership and immutable published frames with reusable capacity; structural and component revisions drive invalidation.

## Questions to settle before implementation

- F0 readiness questions are resolved in ADR-0047; F5/F6 must verify them through actual workers, resources and renderer consumers.
- F5: eviction when resources are referenced, load request deduplication, cancellation granularity, missing-type diagnostics, recook/migration, and a budget policy for large resources.
- F6: spatial visibility/bounds strategy, lighting budgets versus tiled/clustered alternatives, pass/resource lifetime tracking, and whether a render graph is necessary. Decide on measured workloads rather than adopting every advanced rendering technique.
- F4/F6: snapshot handoff and device-loss/recreation protocol before threaded simulation/render work. Keep real-time audio callback guarantees while changing resource services.

## Alternatives and consequences

Lazy synchronous loading is simple but can cause frame spikes. Eagerly loading whole scenes bounds some failures but can exceed memory budgets. Explicit readiness and scheduled preparation combine predictable activation with incremental residency, at the cost of a resource state machine and more lifetime testing.

This builds on ADR-0038/0040 and preserves ADR-0041's cache-only, cook-validated runtime. Cook validation does not guarantee successful runtime I/O or device upload. Any accepted change to existing async activation or animation-binding contracts is recorded here with affected sections and migration before implementation.

## Acceptance evidence

F5 tests corrupt/missing dependencies, cancellation, additive/persistent scenes, duplicate-name/reimport binding, and bounded repeated unload. F6 measures extraction, upload spikes, CPU/GPU memory, dense lights, conservative culling, and device recovery. Numerical performance budgets come from F0 baseline and supported targets, not from this proposal.
