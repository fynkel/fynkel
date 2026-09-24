# ADR-0016: Editor architecture authoring service edit preview Play process docking

**Status:** Accepted; its Play decision superseded by ADR-0021 (2026-09-23: Play runs in the editor, the separate process becomes Build and Run); amended 2026-09-24 (the editor may run `fynkel codegen` for Create → Behaviour, below)  
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
- **Docking package:** chosen by a recorded spike in M4 item 2 against written criteria (tabs, drag to re-dock, splits, hidden tabs keep state, a live `Texture` survives moving between tabs, layout serialisation, keyboard focus). Default `docking`; fallback `multi_split_view`. **Result (2026-09-21): `docking` 1.16.2 passes all seven criteria; picked.** The spike is `apps/editor/test/spike/docking_spike_test.dart`, one widget test per criterion, run with `flutter test` in `apps/editor`; it stays in the repo as the regression guard for the package's upgrades. What each test proved, and the findings the shell is built around:
  1. *Tabs* — a `DockingTabs` group renders its tabs, a click switches the visible body, `selectedIndex` and `onItemSelection` follow.
  2. *Drag to re-dock* — a real pointer drag of a tab (a `Draggable` in `tabbed_view`; `DragTarget` anchors over each panel's left/right/top/bottom halves and its tab strip) splits a panel (`R(IHierarchy,T(IConsole,IProfiler))` → `R(C(IHierarchy,IProfiler),IConsole)`), re-tabs onto a strip (dropping on a tab inserts *before* it), and collapses a one-child group; `DockingLayout.moveItem` is the same modifier for programmatic moves (Window menu commands).
  3. *Splits* — rows and columns honour weights to the pixel (divider 10 px), a divider drag resizes both sides and writes the new weights back onto the areas.
  4. *Hidden tabs keep state* — an item with `keepAlive: true` (a `GlobalKey`ed subtree kept `Offstage` behind the selected tab) keeps its `State` across tab switches; without it the State is rebuilt. **Finding: every editor panel is registered `keepAlive: true`.**
  5. *A live `Texture` survives moving between tabs* — the same `State` instance and the same `TextureBox` render object before and after: a move into a tab group, selection, hiding behind another tab, a pointer drag out into a split, maximise and restore (`created 1, disposed 0`). Without `keepAlive` a move disposes and recreates the subtree. The Scene view (item 6) binds its IOSurface to a keepAlive item and never loses it to a re-dock.
  6. *Layout serialisation* — `stringify`/`load` round-trip the §22.2 workspace (`V1:11:1(R;0;;;2,3,11),2(I;9;Hierarchy;0.2;F),…`) with weights (after a divider drag too) and the maximised area; an unknown version or garbage throws `StateError`; a panel id the registry no longer knows is the builder's decision. **Findings: the selected tab of each group is not in the string — the shell saves the selected item ids beside it; the string is the package's format, so the saved layout file carries a `dockingFormat: "V1"` field and a "Reset layout" fallback for a string that fails to load.**
  7. *Keyboard focus* — the hidden tab's descendants are `ExcludeFocus`ed (the field loses focus, `canRequestFocus` is false, Tab traversal never reaches it); a `CallbackShortcuts` binding above `Docking` (⇧⌘P) fires while a text field has focus and still fires after a re-dock; a re-docked field keeps its text and re-focuses; the drag itself does not move focus.
  Not exercised and not needed for V0: multi-window, floating panels (§22.2 defers both). `multi_split_view` 2.4.0 and `tabbed_view` 1.18.1 come pinned by `docking` (newer majors of both exist; `docking` owns that upgrade). No native code, nothing on the runtime path.
- **Extension model:** ADR-0018 — declarative Inspector hints in 1.0, no custom widgets, windows or editor scripts; a compiled plugin SDK after 1.0.
- **Parallel development:** M4 runs on branch `editor` in its own worktree while M3 finishes on `main`, under the rules in §19.2e (new directories only; shared-crate touches as separate commits merged to `main` first, announced by name).

## Amendment (2026-09-24): Create → Behaviour runs codegen

The decision says "the editor never executes game code or codegen". Create → Behaviour (roadmap Phase 2 item 1, Unity's Create → C# Script) needs the new type's metadata and schema before the Inspector can add it, so this one reading changes:

- The editor may **run `fynkel codegen --project <game> --project-only --json` as a subprocess**, and only on an explicit user action: New ▾ → Behaviour in the Project browser. It never runs on a timer or on save. The "run `fynkel codegen`" banner is unchanged; it still checks with `--check` and shows the command.
- **Still true:** the editor process never loads game code or `build_runner`. The CLI runs `build_runner` in its own child process on the game package, as it does from a terminal. The Inspector is still generated from the committed metadata; the session re-reads that metadata after the run (`registerBehaviour`, which also adds the type to the project's component registry as one undoable step).
- **`--project-only`** skips the native pipeline. An editor action therefore never rewrites the engine's own generated files, even when the configured CLI is older or newer than the checkout.
