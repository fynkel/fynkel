# ADR-0016: Editor architecture authoring service edit preview Play process docking

**Status:** Accepted  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

The editor, CLI, and AI tooling must share one project model; the editor must render scenes without running game code; Play must not risk authoring data.

## Decision

- The authoring command service is the Rust crate **`fynkel_document`** (validation, transactions, revisions, undo data, diagnostics). The editor calls it through the C ABI; the Rust CLI links it; agents use the CLI.
- The Scene view renders a **real Rust world in edit mode** inside the editor process: native components only, no Behaviours ticking, gizmos via engine debug primitives.
- The Inspector is generated from committed codegen metadata; the editor never executes game code or codegen.
- **Play launches a separate game process/window.** The editor never hosts game Behaviours. This is the 1.0 design.
- Editor V0 uses a pub.dev docking/multi-split package (candidates: `docking`, `multi_split_view`); chosen in M4.

## Alternatives considered

- Dart-side authoring service: duplicates loader/validation logic already in Rust.
- Local JSON-RPC daemon: multi-client-friendly and browser-ready, but a process to manage; reconsider post-1.0 for the browser editor.
- In-process Play: Unity-like but needs crash isolation and in-editor hot reload of game code.

## Consequences

One validation implementation. Editor crashes cannot corrupt play state and vice versa. Runtime inspection during Play uses the game process's diagnostics channel, to be designed in M4.
