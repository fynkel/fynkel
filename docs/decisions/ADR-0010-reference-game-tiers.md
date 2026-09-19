# ADR-0010: Reference game tiers

**Status:** Accepted  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

The v0.3 M3 gate required skinned animation, depth-tested world-space UI with text, and spatial audio — each subsystem-sized — before any editor work could begin.

## Decision

The §1.2 specification stays frozen as a whole; the milestone that proves it is split. **Tier 1 (M3):** static-mesh player, Flutter HUD, projected Flutter labels, 2D audio, physics, prefabs, scenes, settings. **Tier 2 (M5):** skeletal animation/GPU skinning, minimal native `WorldCanvas`, spatial audio. World UI in 1.0 = projected labels (Tier 1) plus a minimal `WorldCanvas` (Tier 2); arbitrary Flutter widgets in 3D remain out.

## Consequences

Editor V0 starts after a playable but visually simpler game. The M5 gate requires the full game.
