# ADR-0043: Unified world state and frame contract

**Status:** Accepted through ADR-0046/0047 and clarified by [ADR-0051](ADR-0051-f2-boundaries-and-presentation-policy.md). F1 and the F2 unified gameplay-state migration are complete and [qualified](../foundation-f2-gate-report.md). F3–F8 remain separate milestones.
**Date:** 2026-09-30  
**Related:** ADR-0042, ADR-0004, ADR-0008, ADR-0013, ADR-0014, ADR-0035, ADR-0036; roadmap F1, F2, F4.

## Context

The SDK combines native snapshots, local overlays, entity tables, and implicit presentation routing. Name lookup, derived activity, and world transforms do not share the freshness of ordinary local writes. Native default worlds reuse world ID 1. Coroutine and tick-derived elapsed time account differently for dropped catch-up time. Dart TRS composition and native matrix composition can disagree for nested non-uniform scales.

## Decision and implementation scope

[ADR-0046](ADR-0046-transactional-world-state-replacement.md) accepts coherent mixed-data reads, whole-callback validation/rollback, sequential callback commit visibility, scoped access and staged structural rules. [The mixed-state comparison](../design/foundation-state-comparison.md) is the evidence. [ADR-0047](ADR-0047-identity-clocks-frame-order-and-readiness.md) resolves the remaining identity/clock/frame questions below; its exact contracts take precedence over the earlier alternatives. The [contract checks](../design/foundation-contract-checks.md) exercise candidate rules without claiming production migration.

Provide one coherent gameplay view for ordinary entity/component access, including names, hierarchy, and activity. Apply the same edits and rejection rules through behaviours, systems, object helpers, host callbacks, and editor Play. Explicit completed-simulation snapshots and physics results carry their tick/version; they do not masquerade as immediately edited gameplay state.

Maintain one logical mutation route with safe commit boundaries. Expose render-only adjustments separately and define whether they persist. A property setter does not silently change its semantic lane after querying whether the object has a dynamic body.

Allocate entity world namespaces in the engine. Define uniqueness across any runtimes whose handles may be exchanged, namespace retirement/reuse, slot generations, and exhausted namespace behavior. Keep temporary transaction references explicitly distinct from live handles.

Define real time, scaled gameplay time, and fixed simulation time centrally. Every wait, tween, animation clock, and public timing value names its clock. Use exact matrix composition for world-space geometry operations, with documented approximate rotation/scale decomposition and supported physics transform constraints.

## Questions resolved by ADR-0046/0047

1. Does a gameplay transaction become visible to other systems immediately or at the end of its phase? Specify reads within a transaction, dependent lookup, descendant changes, and cross-system reads in an executable visibility table.
2. Which changes are atomic together? How are failed validation, derived hierarchy state, pending spawns, and conflicting writers reconciled without corrupting the gameplay view?
3. How does gameplay outside a running frame commit changes? How do paused Step, hot reload, and scene replacement participate?
4. Does scaled gameplay time exclude dropped simulation time, or is it a separate advancing timeline? Both are defensible only with explicit consistent consumers. Input edge consumption remains specified independently.
5. Which handle layout and lifetime mechanism prevents aliasing across live and recreated worlds without unsafe namespace recycling? Record ABI and serialization implications; runtime handles are not persistent authoring IDs.
6. What explicit render transform API provides responsive cameras and visuals without a second simulation? Define physics ownership and render-to-simulation transfer, if any.

## Alternatives and consequences

A fully immediate native world simplifies reads but can multiply native calls and complicate iteration. A fully deferred API simplifies implementation but makes ordinary interactions awkward. A coherent gameplay view with phase commits is the preferred hypothesis; it adds synchronization work and must prove its cost.

ADR-0046/0047 supersede the specified state/lane/clock design portions of ADR-0008 for the replacement implementation. Old records and existing tests still describe shipping behavior until migration. Do not claim a compatibility shim has repaired semantic disagreement if it maintains separate mutable truth. Geometry decomposition and physics/effect details still require their named F1.3/F2.2 decisions.

## Acceptance evidence

The [September 30 prototype](../design/foundation-prototype.md) proves scoped custom-data access and the [mixed-state comparison](../design/foundation-state-comparison.md) extends rollback/visibility to supported native built-ins. [ADR-0047 checks](../design/foundation-contract-checks.md) exercise identity, timing and phase models, including actual native fixed-step planning. These support the selected design, but do not fix shipping handles/clocks/presentation or qualify all component families.

Roadmap F1/F2 tests cover multi-world/restart safety; dropped-time accounting; spawn/find, rename/find, active descendants, reparent, destroy, and invalid edits; exact nested transforms; zero/multiple-tick frames; physics and presentation; and parity between query/object access. A before/after example and migration note accompany each changed behavior.
