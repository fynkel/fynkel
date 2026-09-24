# M4 gate report and go/no-go

**Date:** 2026-09-24, verdict 2026-09-25 · **Milestone:** M4 (blueprint §16 "M4 — Fynkel Editor V0", backlog §19.2e item 10) · **Verdict: GO (2026-09-25).** Every row below passes; the first, the user's hands-on session, was run on 2026-09-25 (§2, §4).

M4 asked whether a developer can make a level in Fynkel with an editor
instead of code: open a project, build a scene visually, press Play and
get the real game, with every edit going through the same commands the
CLI uses and no file edited by hand. The backlog (§19.2e) landed items
1–9h between 2026-09-21 and 2026-09-24, plus ADR-0021 (Play inside the
editor, P1–P7), ADR-0020 (tags and layers) and roadmap Phases 1–2
(stable sub-asset ids, SetActive, scale, sidecars, scripts, the Profiler,
IDE navigation, accessibility and the words table). Each item was driven
by the user on a copy of the reference game before it was called done
(the M3-19a rule). This report is item 10.

## 1. Gate criteria

| Gate | Result | Evidence |
| --- | --- | --- |
| A developer can recreate the reference level visually (item 10: the user rebuilds the start area — floor, walls, the Player prefab, the camera rig, the sun, three coins — from an empty scene with the editor only, saves, and `fynkel validate` / `fynkel test --headless` / `fynkel run` accept it) | **Pass (2026-09-25).** The user's own session in the editor, on `examples/reference_game` itself rather than a fresh copy: "all working good". A first attempt on 2026-09-24 had found the prefab workflow unlike Unity's; that led to ADR-0022 and the Unity parity pass (`docs/unity-parity.md`, tiers 1–3), all done before this session (§4). | Items 3–7 done-notes in blueprint §19.2e; §2 below is the checklist. |
| All mutations go through shared authoring commands | **Pass.** The editor holds no project truth: every document change is one `fynkel_document` transaction through `AuthoringSession::apply` (the same commands `fynkel scene apply` reads), every file operation one session asset operation. Since 2026-09-24 the session also writes each of them to `.fynkel/authoring.log` — commands, undo, redo, save, asset operations — and replaying a log on another copy writes the same bytes. | `crates/fynkel_authoring/src/log.rs`; test `every_change_of_a_session_is_in_the_authoring_log_and_replays`; docs/documents.md § The authoring session and its ABI. |
| The CLI sees the same data immediately | **Pass.** Saves are atomic replacements of the source documents, no editor-only format in between; the session rescans what it wrote. The CLI reads the files, so `fynkel validate` / `scene inspect` right after ⌘S see the save (driven by the user in items 3, 4, 5). The other direction too: a file changed from a terminal re-validates through the watcher, a dirty tab gets the conflict dialog, a saved script reaches the Inspector in about a second (`fynkel codegen --watch`). | Tests `a_transaction_that_writes_resolves_paths_against_the_project_and_rescans`, `editing a scene on disk revalidates through the watcher`, `a conflict shows the badge; the dialog offers reload, save as, keep editing`, the codegen-watch tests in `project_scripts_test.dart`. |
| Play launches the actual project runtime | **Pass.** ADR-0021: `fynkel editor <project>` compiles the project's game into the editor, so Play runs the game's own `buildGame` in the Game view; *Build and Run* runs `fynkel run` as a separate process with the editor channel (Console and Profiler read it). Warm open measured today on a copy of the reference game: the built editor launches in 0.2 s and hot reload is on at 2.0–2.5 s (item 9's "under 3 s warm"); cold build 34.5 s. | docs/editor.md § The project editor, § The Game view, § Build and Run; the P1–P7 gates in `play-in-editor` notes. |
| Play crashes do not corrupt source documents | **Pass.** Play never writes a source document: what the editor holds unsaved is cooked in memory, recovery revisions go to `.fynkel/recovery/`. A `kill -9` of the run from outside shows the exit code, keeps the records and leaves every document byte-identical; Stop takes the whole process tree. | `play_process_test.dart`: `a \`kill -9\` from outside shows the exit code, keeps the records and changes no document`, `Stop takes the grandchild with it and leaves every document byte-identical`; `play_test.dart` (the stubborn tree). |
| Ordinary errors are diagnosable without editing raw UUIDs or cache files | **Pass.** Every problem is a diagnostic with a code and a suggestion in the Console, and a Console row opens the document and selects the entity; references are chosen from pickers by name; a missing or broken prefab says which; a script error shows in the Inspector and the toolbar and Play waits until it is fixed; the cache is rebuilt, never edited. | docs/editor.md § Missing and broken prefabs, § The Inspector, the Console sections; test `a Console row of a scene opens the tab and selects the entity`. |

Two gaps the gate itself exposed were closed while writing this report
(2026-09-24): the authoring log did not exist (`4209ba7`), and the startup
scene could only be changed by editing `fynkel.project.json` by hand
(`9e966a4`, `60ede40`).

Test totals at `60ede40`: the editor's `flutter test` 405 passing and
`flutter analyze --fatal-infos` clean; `fynkel_authoring` and
`fynkel_native` cargo tests green; the CLI's tests green.

## 2. The rebuild (for the user)

On a **copy** of the reference game inside `examples/` (relative path
dependencies) — `git archive HEAD examples/reference_game | tar -x -C /tmp
&& mv /tmp/examples/reference_game examples/rg_gate` — then
`fynkel editor examples/rg_gate` (the first open builds, about 35 s):

1. Project panel → **New ▾ → Scene**; double-click it (an empty scene).
2. Build the start area with the editor only: the floor, the walls, drop
   the **Player** prefab, the camera rig, the sun, three coins (prefab or
   copies). Position them with the gizmos and the Inspector.
3. ⌘S. Right-click the scene in the Project panel → **Set as Startup
   Scene** (new for this gate: `fynkel run` starts the startup scene, and
   nothing in the editor could set it before). Then in a terminal, with
   the editor still open: `fynkel validate`, `fynkel test --headless`,
   `fynkel run` on the copy.
4. Press Play in the editor: the player walks and collects the coins.
5. Read `<copy>/.fynkel/authoring.log`: every step above is a line.

Rules: no text editor on any scene, UUID, cache or generated file. Any
surprise goes into §4 before the verdict.

## 3. What M4 delivered

- **The session and its ABI** (`fynkel_authoring`, `fynkel_native`
  feature `authoring`, `package:fynkel_authoring`): one project, many
  documents, per-document undo, the edit-preview world, CPU picking,
  asset operations with their own undo, and now the authoring log.
- **The editor** (`apps/editor`): docking panels; document tabs; the
  Hierarchy; one Inspector (entities, assets, folders; prefab overrides;
  behaviours from committed metadata); the Project browser with Scripts
  and Create → Behaviour; the Scene view with move/rotate/scale/transform
  tools, component handles and `@Gizmo` handles; Prefab mode; Play inside
  the editor with live Hierarchy and Inspector on the running world and
  Apply to Scene; Build and Run with the game Console; the Profiler; IDE
  navigation; tags and layers; the command palette; Unity's keys.
- **Accessibility and the words** (item 9): ⌘1–⌘7 and ⇧⌘C reach every
  panel's keyboard focus, Tab moves within a panel, the Hierarchy rows
  and icon buttons carry semantics, and the command, menu and panel words
  live in one table (`lib/src/l10n/strings.dart`).

## 4. Surprises from the rebuild

- **The first attempt (2026-09-24)** found three things: no drag from the
  Hierarchy to the Project to make a prefab, a dropped prefab instance
  with no Transform, and an instance wrapper (`ball › Ball › Danger`)
  that is not Unity's. The first two were fixed the same day (`32f2cfa`,
  `82f5c34`), the third by ADR-0022 (one root; the instance is the root).
  The user then asked for the editor to behave as a Unity user expects,
  and the Unity parity pass followed: three tiers, each row with a test
  driving the whole shell (`apps/editor/test/parity/`).
- **The session of 2026-09-25** turned up one bug: Finder's `.DS_Store`
  in `assets/` got an automatic sidecar, because a rescan of named paths
  did not skip hidden files as the full scan does. Fixed in `a69d448`.

## 5. Carry-overs to M5

- **F1 — Autosave.** The editor has recovery revisions for Play only; an
  editor crash loses unsaved edits. Autosave with a recovery prompt on
  the next open.
- **F2 — Editor extensions** (ADR-0018, reopened by ADR-0021): custom
  Inspectors, drawers and windows in Dart — roadmap Phase 3 item 1.
- **F3 — Accessibility gaps.** Inspector text fields are not linked to
  their row labels for a screen reader; the docking splitters have no
  label; panel-internal strings (dialogs, empty states, tooltips) are not
  in the words table yet.
- **F4 — Runtime inspection over the channel for *Build and Run*.** Live
  inspection works on Play inside the editor (ADR-0021 P7); a game run as
  a separate process only streams records one way.
- **F5 — The log's size.** `.fynkel/authoring.log` grows without bound;
  rotate it once a real project shows how fast.
- **F6 — Unity differences left from the parity pass:** a selection
  spans one scene at a time; the model import settings have no Rig or
  Animation tabs (clip splitting, loop settings); one friction value in a
  Physics Material (Rapier has no static friction, ADR-0025).
- **F7 — CI.** GitHub has not started a job since 2026-09-23 (the
  account's billing); the Phase 1 CI steps have never run there.
- From M3: E2 (published artifacts), E3 (device and second machine), E4
  (threads), E5–E8 (physics, audio, animation, input V1 limits), E9
  (native components' Inspector hints still a table).

## 6. Decision

**GO (2026-09-25).** Every criterion passes, the user's own session
included. M4 is closed; the carry-overs are §5, and the order from here
is `docs/roadmap.md` Phase 3 (extensibility and the data model).
