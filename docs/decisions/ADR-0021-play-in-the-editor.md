# ADR-0021: Play in the editor — the game compiled into a project editor, a Game view, hot reload

**Status:** Accepted (2026-09-23, the user's call: "sounds good, let's go", after driving the spike on the reference game)  
**Date:** 2026-09-23  
**Supersedes:** ADR-0016's Play decision ("Play launches a separate game process/window. The editor never hosts game Behaviours") and the "In-process Play" alternative it rejected. The rest of ADR-0016 stands.  
**Related:** ADR-0007 (hosts own the loop), ADR-0008 (the main isolate steps the world), ADR-0014 (errors across the ABI), ADR-0016 (editor architecture), ADR-0018 (extension model), spike branch `spike/in-editor-play` (2166fc9)

## Context

Play today is `fynkel run`: validate, cook, `flutter run -d macos`, a second app in its own window, its output relayed over the stdout channel. It works and it is safe, but it is a **build**, not a play: tens of seconds before the first frame, a window that is not the editor, and an Inspector that cannot see the running game. The user's goal is Unity's loop — open or create a project, press Play, Pause, Stop, without thinking about processes — and "if Play does not feel like that, the engine falls apart".

ADR-0016 rejected in-process Play because it "needs crash isolation and in-editor hot reload of game code", and ADR-0018 because "Flutter AOT cannot load a project's Dart at run time". Both are true of a **prebuilt, AOT** editor. They are not true of an editor that is **built per project, in JIT**, which is what Unity does in spirit (it compiles the project's scripts and loads them into the editor) and what Unreal does literally (the editor loads the game module).

The spike (branch `spike/in-editor-play`, 2026-09-23, driven by the user on the reference game) proved it:

| | Measured |
|---|---|
| First open of a project (build the editor with the game in it) | 56 s once; later opens reuse the build cache |
| Play → first game frame, playing the open scene | **19 ms** |
| A saved change to a behaviour reaching the running game | **~350 ms** (hot reload; compile 7–14 ms) |
| The game's HUD and menus inside the editor | the game's own widgets, unchanged |

## Decision

1. **The editor is a library; a project opens in its own editor build.**
   - `apps/editor` becomes `package:fynkel_editor` with one entry point, `runFynkelEditor(args, game: …)`. Its own `main` (no game) stays: the launcher, and projects that are not Dart packages.
   - `fynkel editor [dir]` generates `<project>/.fynkel/editor_host/` — a Flutter package that depends on `fynkel_editor` and on the project's package, with the editor's macOS runner — and runs it **in debug (JIT)** under `flutter run --machine`. The host is generated, never edited, git-ignored with the rest of `.fynkel/`.
   - The launcher's Open and New start `fynkel editor <dir>` and hand over to it. The first open of a project builds (about a minute, like Unity's first import); later opens reuse Flutter's build cache.
   - A game already needs the Flutter SDK, so the project editor needs nothing a Fynkel user does not have.

2. **A game exposes one entry point, used by its build and by the editor.** `lib/fynkel_game.dart` exports `Widget buildGame(FynkelLaunch launch)`: the game's whole app (its own `MaterialApp`, theme and navigator), so the editor shows exactly what a build shows. `FynkelLaunch` (`package:fynkel_flutter`) carries the project directory, the cache, the scene to start in and whether this is the editor. The game's `main.dart` is `runApp(buildGame(FynkelLaunch.fromEnvironment()))`. `fynkel create` writes both files; the reference game is converted.

3. **Play runs the game inside the editor, in a Game view.**
   - **Scene** and **Game** are separate dockable panels, as in Unity. The Scene view is the editor camera, gizmos and handles, unchanged. The Game view shows the world through the scene's active camera, and between runs it is a live preview of what the camera sees.
   - **Play** cooks, mounts `buildGame` in the Game view, brings that tab to the front and gives it the keyboard. It plays **the open scene**: the game's normal start-up runs, then the open scene loads in place of the startup scene. A later Play option can choose "the startup scene"; the open scene stays the default.
   - **Pause** stops the game's loop **and its audio**. **Step** advances one frame. **Stop** unmounts the game, which disposes its world: nothing of the run survives.
   - The controls are the title bar's **Play / Pause / Step** group (Play turns into Stop while playing), with the run's state beside them, as in Unity's toolbar.

4. **Play never needs a save.** Play cooks the documents as they are in the editor — unsaved edits included — into a play cache, and leaves the files on disk alone. The save prompt before Play goes away. Before a run starts, dirty documents are also written to `.fynkel/recovery/`, so a hang (below) cannot lose work.

5. **Game code is hot reloaded into the running editor.**
   - `fynkel editor` watches the project's `lib/` and hot reloads on save; a playing game picks up changed methods without restarting.
   - When Dart refuses a reload (a changed static initialiser, some type changes), `fynkel editor` falls back to a **hot restart**. The editor restores its project, layout, tabs and selection from the state it already persists, and a running Play ends.
   - A change to a behaviour's fields still needs `fynkel codegen`, as today (the banner says so). Running it on save is a later improvement.

6. **`FynkelApplication` is what a game asks of the app it runs in** (Unity's `Application`):
   - `quit()` ends the process in a build and ends Play in the editor; games never call `exit()`.
   - `inEditor` says which of the two it is.
   - Pause, Step and audio suspension reach `FynkelGameView` through the same package, so every game gets them without code.

7. **Build and Run keeps the separate process.** Today's `fynkel run` path — its own window, the stdout channel and the Game Console — becomes **Play → Build and Run**: for a release-speed test, a real device, or a crash that must not take the editor down.

8. **Live Play mode is the next decision, not this one.** In Unity the Hierarchy, Inspector and Scene view show the *running* game during Play, and changes made there are thrown away on Stop. This ADR commits to that direction: nothing a run does ever writes a document, except an explicit "apply to source" command. It leaves the design to its own step (P7).

## What the editor gives up, and why that is accepted

- **One process.** An infinite loop in game code freezes the editor, as in Unity. Other failures are still contained:
  - an exception in a behaviour is reported by the world as it is today;
  - a widget error draws Flutter's error box inside the Game view;
  - a Rust panic stops at the ABI (ADR-0014).

  The recovery copy (4) covers the hang. Build and Run (7) remains for code that must be isolated.
- **Debug speed.** The project editor runs in JIT with debug checks, so frame times in the Game view are slower than a build's. Unity's editor has the same gap. Measurements use Build and Run or `fynkel run --profile`, as today.
- **Statics survive between runs.** Each Play gets a fresh `World`, but Dart globals and statics live as long as the isolate. This is Unity's "Enter Play Mode without domain reload" behaviour. It is documented; a "restart before Play" option can come later if real games need it.
- **A build per project.** The first open costs a build, and a broken `lib/` means the project editor does not compile. `fynkel editor` then says so and offers the plain editor (1), which opens the project with Play unavailable until the code compiles.

## Effect on other decisions

- **ADR-0016:** its Play bullet and the "In-process Play" alternative are superseded. The authoring service, the edit-preview world (still no behaviours ticking in the Scene view), metadata-driven Inspectors and the docking choice all stand. Its Play diagnostics channel stays with Build and Run.
- **ADR-0018:** its reason for "no project code in the editor" ("Flutter AOT cannot load a project's Dart") no longer holds, because the project's code is now compiled into its editor. Whether 1.0 opens custom inspectors and editor scripts is **not** decided here. It becomes a question of scope rather than possibility, reopened in its own ADR.
- **ADR-0007 / ADR-0008:** unchanged. The editor is one more host that owns the loop, and the game still steps its world on the main isolate.

## Implementation steps

One commit each (or a short series), each with its tests, on `main`:

- **P1** — this ADR accepted; ADR-0016's header points here.
- **P2** — `package:fynkel_flutter`:
  - `FynkelLaunch`;
  - `FynkelApplication` (`quit`, `inEditor`);
  - `FynkelGameView` honours the editor's Pause/Step and suspends audio while paused.

  The reference game exposes `lib/fynkel_game.dart`, and its Quit button uses `FynkelApplication.quit`. `fynkel create` writes the same files.
- **P3** — `package:fynkel_editor` (`runFynkelEditor`), and `fynkel editor [dir]`:
  - host generation;
  - `flutter run --machine` supervision;
  - hot reload on save;
  - the hot-restart fallback that restores the editor's state;
  - the plain-editor fallback when the project does not compile.

  The launcher hands over to it.
- **P4** — the Game view panel:
  - the camera preview between runs;
  - Play mounting the game there;
  - the title bar's Play / Pause / Step group;
  - the keyboard going to the Game view while playing.
- **P5** — Play from memory: the play cache cooked from the open documents, the recovery copy, and no save prompt.
- **P6** — Build and Run: today's process path under its new name. `docs/editor.md` and `docs/status.md` are brought up to date, and the spike branch is deleted.
- **P7** — Live Play mode (8): its own design note first — `docs/design/live-play-mode.md`, accepted 2026-09-23, steps L1–L5.
