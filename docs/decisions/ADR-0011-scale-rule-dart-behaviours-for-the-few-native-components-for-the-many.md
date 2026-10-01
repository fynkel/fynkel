# ADR-0011: Scale rule Dart behaviours for the few native components for the many

**Status:** Accepted  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

**Direction amended 2026-09-30 by [ADR-0042](ADR-0042-ecs-centered-foundation-program.md):** behaviours remain convenient local logic, but custom bulk systems and a native extension route are now foundation work rather than a permanent core-feature-request limitation. The decision below records the original scope and existing implementation. [ADR-0044](ADR-0044-components-systems-and-gameplay-access.md) proposes the replacement component/access/scheduler details.

The performance ambition is thousands of rendered objects. Per-object Dart callbacks do not scale to thousands per frame regardless of ECS speed (managed-language cost, bridge encode/decode, GC), the same wall Unity's MonoBehaviour and Godot's GDScript hit. Users must not need to write Rust.

## Decision

- Dart `Behaviour` per object is the primary model, sized for hundreds of ticking behaviours.
- Fynkel ships **data-driven native components** for uniform bulk behaviour, configured from Dart/scene data: `Rotator`, `Oscillate`, `Follow`, `LookAtCamera`, `LifetimeDespawn`, simple emitter — plus physics/animation which already are such systems. The catalogue grows only from reference/pilot needs.
- Users never write Rust in 1.0; bulk logic the catalogue cannot express is a Fynkel feature request.
- **Batched Dart typed-data views** are the post-1.0 path for "thousands from Dart", added on measured need.
- Renderer V1 includes instancing and frustum culling at M3. A `benchmarks/stress` scene (~5k instanced meshes, 500 bodies, 300 behaviours) is tracked from M3, separate from the small reference game.

## Consequences

The reference game stays small; the stress scene carries the scale claim. Documentation must teach the split explicitly.
