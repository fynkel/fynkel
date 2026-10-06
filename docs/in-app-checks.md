# In-app checks still owed (2026-10-06)

Everything below passes its automated tests but has **not been confirmed
by a person in the real editor or on a device**. Collected from the
progress notes and ADR histories. Work through it in order; each row ends
as ✅ (works), 🐞 (bug — write what you saw) or ➖ (skipped, say why).

How to run: use a **copy** of an example, never the original, and check
`ps` first so you do not touch a copy that is open. The copy must live
**inside the repo** under `examples/` (the example's `pubspec.yaml` uses
`../../packages/...` paths, which break from `/tmp`). Name it `chk_<name>`:
those folders are git-excluded (`.git/info/exclude`).

Run the lines one at a time and do not paste `#` comments into zsh (it
passes them as arguments). Swap `reference_game` for `showcase`,
`shader_try` or `code_only` as each section says.

```sh
cd /Users/oumari/Desktop/Code/fynkelengine
cp -R examples/reference_game examples/chk_reference_game
FYNKEL_CLI=$PWD/target/debug/fynkel target/debug/fynkel editor examples/chk_reference_game
```

`~/.cargo/bin/fynkel` is stale; always use `target/debug/fynkel`.
Steps below name menus and keys as the notes recorded them; if one has
moved, note the new name rather than skipping the row.

## A. Animation (highest risk: biggest surface, never seen by a person)

Use `examples/reference_game` (Player has a controller) and `examples/showcase` (Runner).

| # | Check | Expect | Result |
|---|---|---|---|
| A1 | Open the **Animator** panel, select `player.controller.json` | State graph with zoom; states and transitions drawn | |
| A2 | Add a state, a transition, a parameter; undo/redo each | Edits apply and undo cleanly; file saves | |
| A3 | Select a model in Project, **Create Animator Controller** | A controller appears and opens | |
| A4 | Press Play; open the Animator panel | Progress bars move; live parameters change as you move the Player | |
| A5 | Blend tree: add a 2D tree to a state (Inspector shows a 2D map like Unity's) | New tree asks for a first clip; dragging points on the map edits thresholds | |
| A6 | Reference game: hold right mouse / LT (aim) | Strafe clips blend, Aim state is entered | |
| A7 | Press the wave key in the reference game | One-shot wave plays on the upper body only; legs keep walking | |
| A8 | Sub-state machine: enter one, use the path bar to go back | Path bar works; `interruptibleBy` respected | |
| A9 | **IK tab** and **Mask dialog** in the Animator | Feet plant on the Terrace steps; look-at goal turns toward the nearest pickup | |
| A10 | Reload after editing the controller during Play | Game keeps running; no crash | |

## B. Shaders and materials

Use a copy of `examples/shader_try` or `examples/showcase`.

| # | Check | Expect | Result |
|---|---|---|---|
| B1 | **Create ▸ New Lit Shader** and **New Unlit Shader** | A `.shader` file opens in its own page | |
| B2 | Assign the shader to a material (Shader dropdown on the material) | Material fields come from the shader's properties; same-named values carry over | |
| B3 | Introduce a WGSL error in the shader | Error shows at the right file line; object turns magenta; fix → recovers | |
| B4 | Showcase scenes `shader_dissolve`, `shader_water`, `shader_rim` (keys 1–4 in the app) | Each looks as the PNGs in `examples/showcase/renders/` | |
| B5 | Play, then edit a material value in the Inspector | The running game changes at once, including `dissolve_crate` | |
| B6 | Material Inspector: colour picker, sliders, drag a material onto an object (Hierarchy row and Scene view) | Live preview; OK = one undo, Cancel restores; drop assigns | |
| B7 | Volume Profile Inspector (bloom/AO/grading) | The picture changes visibly as you move sliders | |

## C. Inspector previews (ADR-0030)

| # | Check | Expect | Result |
|---|---|---|---|
| C1 | Select a material | Preview area with an orbitable sphere; the shape you pick **stays** when you select another material | |
| C2 | Select a model, a prefab, a mesh | 3D stage preview; drag to orbit | |
| C3 | Select a texture | Channels view + info | |
| C4 | Select a sound | Waveform, play, loop | |
| C5 | Open 3+ viewports (Scene, Game, preview) for a while | No stutter, no growing memory (known cost ≈ 45 MB per preview) | |

## D. Data assets and extensions (use `examples/code_only` copy)

| # | Check | Expect | Result |
|---|---|---|---|
| D1 | Create a data asset of a game type (e.g. the coin kind), edit its fields, assign it to a script field | Typed picker only offers the right type | |
| D2 | Press Play, then edit the asset in the Inspector | The running game follows the edit; stopping Play does **not** save game-side changes | |
| D3 | `@FynkelRecord` field (Sparks): foldout, add/remove, move up/down | List editor works; saved | |
| D4 | **Hot reload** (the one open ADR-0028 check): change a method body in `lib/`, save, with Play running | Behaviour changes without restarting Play | |
| D5 | Hot reload failure: put a syntax error in `lib/` | Console shows the compile error with file/line; Play refused; fix → works | |
| D6 | Custom Inspector, property drawer (jumpHeight), Tools menu, Level Tools window | Still work after the above | |

## E. Editor behaviour (Unity-parity pass)

| # | Check | Expect | Result |
|---|---|---|---|
| E1 | Drop a file from **Finder** into the Project panel | Imported; clicks, scroll and pinch still work afterwards (the drop overlay must not eat input) | |
| E2 | **GameObject** menu → 3D Object / Light / Camera (and the same in Hierarchy right-click) | Submenus cascade; the shape is visible; new row is renameable at once | |
| E3 | Colour picker on a component | Live preview in the Scene; OK = one undo | |
| E4 | Click `▸` on a model in Project | Parts appear as draggable rows | |
| E5 | Play gaps: while Playing, create and duplicate an object, **Apply to Scene** on a whole object | Appears in the running game; Apply writes the scene | |
| E6 | Prefabs: Overrides dropdown, blue bar, Apply/Revert, nested prefab, Variant (drag an instance to Project) | Matches Unity's behaviour | |
| E7 | Edit and save in the editor, quit, reopen | Everything is there; no `.DS_Store` sidecar noise | |
| E8 | **UI refresh**: look at the whole editor (title bar, Project header, status bar, Console picker) | Looks simple; note anything crowded | |

## F. Game and device

| # | Check | Expect | Result |
|---|---|---|---|
| F1 | `examples/ui_demo` with a **real game controller** | Menus navigate by stick/D-pad; A confirms | |
| F2 | `fynkel create` a fresh project; run it; use the main/pause/settings menus | Menus work as a real app (the template has only been tested by `flutter test`) | |
| F3 | Dense scenes (dense-2000 / dense-5000) at full screen on the Mac | Look for **judder** — the counters say 120 fps but pacing is bimodal | |
| F4 | A/E | **Every** Scene-view sync fell back to a full reload on `reference_game` (stats `syncs 97 fallback reloads 97`). **Cause:** a `Skin` component's `skeleton` is an object reference by authoring id in the document but a live handle in the world; the sync compared them raw (never equal), tried to write the id form, and the world refused it (`UnresolvedObjectReference`). Fixture tests had no object references. **Fixed:** `preview.rs` resolves references through the id map before comparing/writing; regression test `tests/preview_reference_game.rs` (fails without the fix). Needs the user to restart the editor and confirm. | L → fixed |
| F5 | A | Selecting a state in the Animator turned the Game view grey/white (materials and colours gone; Scene view still coloured). Suspected consequence of F4 (a full reload while the Game view draws the preview world). Not yet reproduced on purpose. | M |
| F6 | E | Pink (missing) materials appear in the Scene view now and then. Suspected: assets momentarily unresolved during an F4 reload. Not yet reproduced on purpose. | M |
| F7 | E | Dragging a panel splitter shows the old frame stretched until the new size renders. Unity re-renders while you drag. | M |
| F8 | E | Flutter's red "RIGHT OVERFLOWED BY 67 PIXELS" stripe on the Game view toolbar when the Game panel is narrow (debug build; the overflow is real and should be fixed or clipped). | S |
