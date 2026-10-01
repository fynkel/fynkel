# ADR-0042: ECS centered foundation program

**Status:** Accepted for architectural direction. F1/F2 contracts are implemented and qualified; see the [active roadmap](../foundation-roadmap.md) for accepted later decisions and remaining milestones.
**Date:** 2026-09-30  
**Authority:** User agreement with the foundation review and ECS direction, followed by the request for a roadmap and ADRs.  
**Related:** ADR-0003, ADR-0004, ADR-0007, ADR-0008, ADR-0011, ADR-0012, ADR-0036; [foundation roadmap](../foundation-roadmap.md).

## Context

Fynkel uses native ECS storage while custom gameplay is primarily per-object Dart behaviour. The SDK bridges deferred simulation with selective read-your-writes overlays and presentation overrides. A source review identified conflicting freshness rules, world namespace allocation concerns, divergent clock accounting under dropped ticks, closed native system registration, and whole-world synchronization costs. Performance conclusions require measurements; correctness findings require reproductions.

The user wants quick game development, scalable execution, one coherent API model, and foundation work before more features. Rebuilding a layer is acceptable where it removes structural problems.

## Decision

- Make components and entities the shared gameplay foundation. Keep `GameObject` as a convenient entity facade, hierarchy and prefabs as authoring concepts, and behaviours for local logic.
- Object access and bulk system access use the same logical component state and mutation rules. A convenience interface must not own a competing copy of gameplay truth.
- Give every component type an explicit authoritative owner. Decide custom Dart/native storage through a measured prototype; do not assume all custom fields can become Rust data or that serializing all fields each frame is acceptable.
- Support registered systems and a native extension route for heavy workloads. Rust remains optional for ordinary gameplay. Built-in bulk components remain useful but are not the only extension mechanism.
- Preserve fixed simulation, render extraction, generated contracts, cooked assets, and transactional authoring where they fit. Begin execution sequentially; parallelism and new storage are justified by evidence.
- Prioritize the foundation roadmap before broad product expansion. Track acceptance criteria and evidence, with named limitations rather than unsupported competition or scale claims.

## Relationship to earlier decisions

This amends ADR-0011's direction: per-object behaviours are no longer the exclusive primary custom gameplay path, and unsupported bulk logic must not permanently require a core-engine feature request. The registered access and extension implementation is specified by ADR-0044 after its prototype.

ADR-0003's `hecs` facade and ADR-0007's library/host separation remain accepted. ADR-0036's convenient object references remain compatible with an entity facade; its current detailed APIs describe the implementation pending migration. ADR-0004 and ADR-0008 retain their current implementation contracts until accepted replacement details are recorded. This ADR does not choose a new handle layout, commit model, ABI, scheduler syntax, or threading arrangement.

## Alternatives

Continue expanding the built-in catalogue: low migration cost, but custom population logic stays constrained. Expose only a pure ECS API: fewer facades, but unnecessary friction for local interactions and authoring. Rewrite the entire engine immediately: may simplify some layers, but discards working tooling before a replacement contract is demonstrated.

The chosen direction combines one component/world model with ergonomic and bulk access. F0 selects incremental replacement or focused rebuild from executable evidence.

## Consequences and verification

Breaking changes are acceptable with explicit migrations, versioning, and retirement plans. A smaller API is a design objective, not a reason to remove familiar scene workflows. Cross-language ownership remains a design problem to solve, not a benefit automatically supplied by ECS.

The foundation program starts at zero implementation milestones completed. Its prototype must mix behaviours and bulk systems, support authored data, and show consistent reads. Completion requires the roadmap's gate evidence; accepting this ADR does not mark those gates complete.
