# ADR-0003: hecs behind fynkel_ecs

**Status:** Accepted  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

An ECS is needed for storage and iteration; writing one is a classic scope trap.

## Decision

Use `hecs` as the 1.0 ECS implementation, fully hidden behind `fynkel_ecs`. Fynkel owns handles (world id, slot, generation), scheduling, metadata, commands, snapshots, and serialization. `hecs::Entity` never crosses a public or serialized boundary. A custom ECS may replace `hecs` only via the §4.3 gate (feature impossibility, or ≥20% relative and ≥0.5 ms p95 absolute win on a frozen workload), and only after a prototype preserves the `fynkel_ecs` API.

## Consequences

No custom ECS work before M6 at the earliest. Contract tests verify `hecs` types are absent from public APIs and documents.
