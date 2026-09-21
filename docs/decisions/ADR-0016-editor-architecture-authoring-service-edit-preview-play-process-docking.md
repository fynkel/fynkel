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

## Resolutions at the start of M4 (2026-09-21)

Recorded here rather than in the blueprint because each one fixes a reading of the decision above; the backlog is `docs/blueprint.md` §19.2e.

- **"Through the same C ABI as the runtime"** means: the authoring entry points are exported by `fynkel_native` (which already links `fynkel_document`) behind the cargo feature `authoring`, declared in `include/fynkel_authoring.h`, prefixed `fynkel_authoring_*`, obeying every ABI v1 rule. The prebuilt runtime artifact games download (ADR-0015) is built without the feature, so authoring code never enters a game build. The Dart surface is `package:fynkel_authoring`; games never depend on it.
- **The session layer** between the ABI and `fynkel_document` is the Rust crate `fynkel_authoring` (`AuthoringSession`: project, schemas, asset database, metadata, one `Editor` per open document, edit-preview sync, picking). The CLI may link it later; the editor UI never holds project truth.
- **The edit-preview world** is a `fynkel_core::World` with `WorldConfig::edit_mode` (commands, propagation, snapshot; no systems). It is loaded from an in-process cook and kept in step with the document by mapping committed mutations to world commands, with a full reload as the fallback; a test proves both paths agree.
- **Play's diagnostics channel** (left open above) is JSON lines on the game process's stdout, emitted by `package:fynkel` only when `FYNKEL_EDITOR_CHANNEL=stdio` is set: `log`, `diagnostic` (the §14.2 record), `stats`, `state`. One-directional in V0; runtime inspection is designed on it and built after V0.
- **Docking package:** chosen by a recorded spike in M4 item 2 against written criteria (tabs, drag to re-dock, splits, hidden tabs keep state, a live `Texture` survives moving between tabs, layout serialisation, keyboard focus). Default `docking`; fallback `multi_split_view`. The result is appended here when known.
- **Extension model:** ADR-0018 — declarative Inspector hints in 1.0, no custom widgets, windows or editor scripts; a compiled plugin SDK after 1.0.
- **Parallel development:** M4 runs on branch `editor` in its own worktree while M3 finishes on `main`, under the rules in §19.2e (new directories only; shared-crate touches as separate commits merged to `main` first, announced by name).
