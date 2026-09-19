# ADR-0017: AI and automation interface

**Status:** Accepted  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

AI-native means stable machine-readable contracts, not a chatbot in the editor.

## Decision

- The **structured `--json` CLI is the only agent interface in 1.0**; every automation command has stable exit codes and the §14.2 diagnostic record.
- Agents can observe a running game: `fynkel test --headless --steps N --seed S --json` runs the full simulation without a renderer or GPU; `fynkel state dump --json` returns world state; `fynkel capture` renders offscreen and **requires a GPU** (works on the developer machine, not on headless CI).
- Headless mode = full simulation (ECS, physics, animation sampling, scenes, behaviours, audio muted) with the renderer optional.
- `fynkel create` writes an `AGENTS.md` into every project: commands, schema locations, validate→codegen→build→test loop, do/don't list.
- An MCP server wrapping the CLI is a post-1.0 addition; no in-editor agent panel is planned.

## Consequences

The CLI is designed for two readers from day one. CI cannot run `fynkel capture` unless a GPU runner exists; document this.
