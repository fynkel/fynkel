# The Fynkel Editor (M4)

The editor is a Flutter desktop app (`apps/editor`, macOS; iOS never) over
an **authoring session** — a project, its documents and their histories —
that lives in Rust and is reached through the same C ABI as the runtime
(ADR-0016). The editor holds no project truth of its own: every panel is a
view over the session, every edit is one atomic transaction with byte-exact
undo, and the CLI sees each save at once. The editor never runs game code
(ADR-0018); Play is the project's real app in a separate process.

This document grows with the M4 items (blueprint §19.2e). It is the
reference for what exists; the UX spec is `design/editor-ux.md`.

## Layers (M4-01)

```
apps/editor (Flutter, macOS)                       ← item 2 onwards
   │  package:fynkel_authoring   AuthoringSession / DocumentHandle / Command
   │  (Dart, ffigen over include/fynkel_authoring.h; games never depend on it)
   ▼
libfynkel_platform_apple / libfynkel_native        cargo feature `authoring`
   fynkel_authoring_* entry points (fynkel_native/src/authoring.rs)
   ▼
crates/fynkel_authoring        AuthoringSession: project + lock + schemas +
                               asset database + behaviour metadata,
                               one Editor per open document, queries
   ▼
crates/fynkel_document         Editor (transactions, undo, validation),
                               Project, Lock, AssetDatabase, SchemaSet
```

- **One session, many documents.** `AuthoringSession::open_project(dir)`
  reads the project file, the lock (a lock written by a newer Fynkel opens
  the project read-only — every edit is `FYN-E0007` with the reason), the
  embedded and behaviour schemas, the asset database with every reference
  indexed, and `lib/generated/behaviours.g.json` when present.
  `open_document(path | id)` gives a handle over a
  `fynkel_document::authoring::Editor` for a scene, a prefab or a material;
  `apply` / `undo` / `redo` / `save` / `reload` / `close_document` act on it.
  `docs/documents.md` § The authoring session and its ABI has the full
  surface, the query kinds and the diagnostics (`FYN-T####`).
- **No editor code in a game.** The entry points are compiled only with the
  cargo feature `authoring`. `package:fynkel`'s build hook builds every
  runtime with `--no-default-features` and adds `--features authoring` only
  when the app's pubspec lists it under `hooks.user_defines.fynkel.features`
  — `package:fynkel_authoring` does, a game never does. CI builds
  `fynkel_native` and `fynkel_platform_apple` without the feature and fails
  if either exports a `fynkel_authoring_*` symbol.
- **The Dart surface** (`packages/fynkel_authoring`): `AuthoringSession`
  (`openProject`, `openDocument`, typed queries, `rescan`, a broadcast
  `Stream<AuthoringEvent>` — project opened/closed, document
  opened/changed/closed, assets changed), `DocumentHandle` (`state`,
  `apply(commands, baseRevision:)` → `Applied`, `undo`/`redo`, `save`,
  `reload`, `close`, `root()`, `inspect()`, `validate()`, `changes`),
  `Command` builders for the seventeen §11.1 commands, the ADR-0014
  `Diagnostic` and `AuthoringException`. Change notification is a plain
  `Stream` (the package is Flutter-independent like `package:fynkel`); the
  app wraps it for its widgets — a deviation from the blueprint's
  "`ChangeNotifier`s per panel concern", which belong to the app, not the
  package.
- **Transactions are the unit of everything.** A gesture (a rename, a
  drag, an Inspector edit) is one `apply` with the document's `revision` as
  `baseRevision`; a stale revision is refused (`FYN-E0001`) and nothing is
  touched — two writers never overwrite each other silently. A rejected
  transaction (an invalid command, a validation error) rolls the document
  back to the byte and says why per command. `history` in the document's
  state is what is left to undo; a saved file stays saved when its
  transaction is undone (`dirty` compares revisions).
- **Files the editor writes.** Documents only through the session (a
  `saveDocument` or `createMaterial` inside a transaction takes its relative
  path from the project directory and the asset database is rescanned for
  what was written). Its own preferences, project-local editor state
  (`.fynkel/editor.json`) and recovery revisions (`.fynkel/recovery/`) come
  with the shell (item 2). The session itself appends every change it
  makes to `.fynkel/authoring.log` (docs/documents.md § The authoring
  session and its ABI) — the record the M4 gate reads.

## The shell (M4-02)

`apps/editor` (`fynkel_editor`) is the Flutter macOS app. Item 2 started
with the docking spike the blueprint asked for: `package:docking` 1.16.2
against the seven written criteria, one widget test each in
`test/spike/docking_spike_test.dart`, all green — the report and the pick
are in ADR-0016 § Resolutions → Docking package. Three findings shape the
shell:

- every panel is a `keepAlive` item (its State — and the Scene view's live
  `Texture` — survives tab switches, re-docks, maximise/restore);
- the serialised layout string has no selected tabs, so the layout file
  stores the selected item ids beside it, with the package's format version
  and a "Reset layout" fallback for a string that fails to load;
- app-level shortcuts sit above `Docking` in a `CallbackShortcuts`; hidden
  tabs cannot hold focus, so nothing is typed into a panel you cannot see.

Run it with `flutter run -d macos` in `apps/editor`, or
`fynkel_editor <project dir>` to open a project straight away (the first
argument that is not a flag). Verify with
`flutter analyze --fatal-infos && flutter test` there (`test/project_test.dart`
drives the real session over the native runtime on the fixture project and
the reference game; the rest use the fakes in `test/support/fakes.dart`).

### What the shell does

**The launcher** (`lib/src/launcher/launcher.dart`) is the window without a
project: New (a dialog that derives the package name from the title —
`  2D Racer! ` → `game_2d_racer` — validates package and organisation
names, picks the parent directory, and runs
`fynkel create <dir> --name … --title … --org … --root <checkout> [--no-flutter] --json`,
showing the log and the CLI's `code: message` diagnostics when it fails;
the created directory is opened on `{ok:true}`), Open (a directory picker;
a directory without `fynkel.project.json` is refused with a message),
Recent (per user; a missing directory is greyed out as "missing" and can
be removed), Preferences, and the doctor checks over the detected
toolchain (`lib/src/env/tools.dart`): Fynkel CLI (PATH, `$FYNKEL_CLI`, or
a checkout's `target/`), Fynkel checkout (`$FYNKEL_ROOT`; New Project needs
it because `package:fynkel` is a path dependency), Flutter (a warning
only), Authoring ABI (the runtime's version against the editor's), IDE
command (a warning only). Each failing check names its fix.

**Opening a project** (`lib/src/project/editor_project.dart`,
`SessionProject`) is `AuthoringSession.openProject` followed by
`validateProject()` and `migrationPreview()`; a `watcher` on the project
directory (400 ms debounce; `.fynkel/`, `.dart_tool/`, `build/`,
`fynkel_cache/`, `.git/`, `ios/`, `macos/`, `.idea/` ignored) rescans the
changed paths and revalidates on every change. A source file with no
sidecar — dropped into `assets/` from Finder, or there when the project
opens — gets one on that scan, as Unity writes a `.meta`: a new id and the
importer from its extension (`OpenProject::rescan`, the report's
`sidecarsWritten`); it shows up in the Project browser like any asset.
Files dragged from Finder onto the Project panel are **imported into the
folder it shows** (`importFiles`, one ⌥⌘Z step): on macOS a
`FileDropView` over the Flutter view (registered for `.fileURL`, clicks
passed through) sends `over` / `exit` / `drop {paths, x, y}` on
`fynkel.editor/drop`; `FileDrops` hands a drop to the panel whose box
holds the point, which outlines itself while files hover.
Nothing is written in a read-only project (`FYN-A0001` stays). Move a
file with its sidecar, or in the Project browser: a file moved alone in
Finder is a new asset, and its old sidecar is reported orphaned
(`FYN-A0002`). The lock check is the session's: a lock that
names a newer runtime, ABI or tool version opens **read-only** — the
toolbar shows a chip and a banner with the exact reason (for example
`the lock names runtime 9.0.0; this build is 0.0.1`) and the Console lists
the matching `FYN-P0011`. Finding: the lock schema pins `formatVersion` to
`1` (a `const`), so a lock with `formatVersion: 2` is a schema *error*, not a
read-only project — "newer" means newer versions inside the lock, which is
what the read-only test drives. Documents behind their kind's
`formatVersion` (`FYN-D0016`) raise a second banner counting them, with
**Preview migration…** (`lib/src/project/migration_dialog.dart`, also
Project → Migration Preview… ⌥⌘M): per document the kind and
`vFrom → vTo`, the steps `fynkel migrate` would apply (id and description),
the unified diff, any diagnostics, and the command to run
(`fynkel migrate <dir>`, followed by `&& fynkel project lock --project <dir>`
when the lock names another build too; with a Copy button). It comes from the session's
`migrations` query and **writes nothing** — the editor never migrates on
its own; `fynkel migrate` keeps `<file>.bak`.

**The main window** (`lib/src/shell/main_window.dart`): the title bar
(the project menu, the document tabs, Play, the codegen state and the
command search — § The title bar, item 9b), the banners, the `Docking`
workspace and a status bar (error and warning counts,
document/source/reference counts, "validated HH:MM:SS"; a click shows the
Console; the current notice at its end). The workspace
(`lib/src/shell/workspace.dart`) is the §22.2 default —
`Hierarchy | Scene over [Project, Console, Profiler, Animation] | Inspector`
(item 9c took the Asset panel into the Inspector) — saved per user (`layout.json` next to the preferences: the docking
string, the selected tab ids and the package's format version) on every
change and restored on the next start; a string that fails to load falls
back to the default with a notice; **Reset layout** (⌥⌘0, also in
Preferences) restores it. Every panel is a `keepAlive` item; hiding a panel
removes its item, showing it again puts it back in its home tab group, or
in a fresh split at the panel's side when nothing of that group is
visible; a maximised area is restored before another panel is focused.
Panels that later items fill are placeholders that say which item brings
them ("Arrives with editor item N"), so an empty panel is never mistaken
for a broken one.

**Commands** (`lib/src/commands/commands.dart`): one `CommandRegistry`
with stable dotted ids; every command has a menu, a shortcut (or, like the
Scene tools, only Unity's bare key) and a palette entry, so each is
reachable three ways (`test/commands_test.dart` proves the
same command runs from all three). The menu bar is native on macOS
(`PlatformMenuBar`) and in-window elsewhere and in tests; only the menus
that have commands exist (Project, Edit, Window, Help — no stub menus). The
palette (⌘K, `lib/src/commands/palette.dart`) ranks by exact label, prefix,
word start, substring, id/menu, then subsequence, over the live label (so
"hide console" finds the toggle while the Console is visible); ↑↓ and Enter
run, Escape closes.

| Id | Menu | Shortcut | Does |
| --- | --- | --- | --- |
| `project.new` | Project | — | the New Project dialog (`fynkel create`); ⇧⌘N is Create Empty, as in Unity |
| `project.open` | Project | ⌘O | pick a directory and open it |
| `project.recent.<i>` | Project | — | generated from Recent; no shortcut |
| `project.close` | Project | ⇧⌘W | close the project, back to the launcher |
| `project.revalidate` | Project | ⌥⌘V | validate again now |
| `project.migrations` | Project | ⌥⌘M | the migration preview |
| `project.reveal` | Project | ⇧⌘R | reveal the project in Finder |
| `app.quit` | Project | ⌘Q | quit |
| `document.open` | Project | ⇧⌘O | the Open Document picker (scenes and prefabs of the project, searchable; § One picker) |
| `document.save` | Project | ⌘S | save the focused tab through the session (refused while it has a disk conflict — the conflict dialog opens instead) |
| `document.saveAs` | Project | ⇧⌘S | save the focused tab under a new relative path (never over an existing file) |
| `document.saveAll` | Project | ⌥⌘S | save every dirty tab |
| `document.revert` | Project | ⌥⌘R | revert the focused tab to the saved file (asks; history dropped) |
| `document.close` | Project | ⌘W | close the focused tab (a dirty one asks Save / Don't Save / Cancel) |
| `document.next` / `document.previous` | Project | ⌥⌘→ / ⌥⌘← | cycle the tabs |
| `document.undo` | Edit | ⌘Z | undo the focused tab's last transaction — the label counts what is left, "Undo (3)"; inside a text field the field's own undo runs |
| `document.redo` | Edit | ⇧⌘Z | redo (same rule for text fields) |
| `hierarchy.rename` | Edit | ⌘R | rename the selected entity in place |
| `hierarchy.duplicate` | Edit | ⌘D | duplicate the topmost selected entities with their subtrees, each right after its original and named as Unity names a copy (`Cube` → `Cube (1)`); the copies are selected |
| `hierarchy.copy` / `hierarchy.cut` | Edit | ⌘C / ⌘X | copy (cut: and delete) the topmost selected entities with everything under them to the editor's object clipboard (in a text field: the field's own copy / cut) |
| `hierarchy.paste` | Edit | ⌘V | paste right after the selected object (same parent), or at the end of the roots; into any open scene or prefab; the pasted objects are selected |
| `hierarchy.pasteAsChild` | Edit | ⇧⌘V | paste under the selected object |
| `hierarchy.delete` | Edit | ⌘⌫ | delete the topmost selected entities with their subtrees; nothing is selected afterwards, as in Unity (in a text field: delete to the line start) |
| `hierarchy.createEmpty` | GameObject | ⇧⌘N | a new object named `GameObject` with a Transform at the root, renamed at once (Unity's Create Empty) |
| `hierarchy.createEmptyChild` | GameObject | ⌥⇧N (bare) | the same under the selected object (Unity's Create Empty Child) |
| `hierarchy.createWith` | GameObject | ⇧⌥⌘N | pick a native component type; the new entity has a Transform plus that component with its defaults, and is renamed at once |
| `gameobject.cube` … `gameobject.quad` | GameObject ▸ 3D Object | — | Unity's Cube, Sphere, Capsule, Cylinder, Plane, Quad at the root (ADR-0023): a MeshRenderer on the built-in `mesh/<Shape>` and `material/Default` plus Unity's collider; the first one adds `Fynkel/Primitives.glb` to the project (`installPrimitives`) |
| `gameobject.directionalLight`, `.pointLight`, `.spotLight` | GameObject ▸ Light | — | Unity's lights: the Directional Light 3 m up, tilted 50° down and turned 30°, warm white, 10 000 lux with shadows; the Spot Light pointing down |
| `gameobject.camera` | GameObject | — | a Camera 1 m up and 10 m back, looking along −Z |
| `hierarchy.keepWorldTransform` | Edit | ⌥⌘K | toggle the drop policy (✓ when on) |
| `hierarchy.search` | Edit | ⌥⌘H | focus the Hierarchy's search field |
| `hierarchy.expandAll` / `hierarchy.collapseAll` | Edit | ⌥⌘] / ⌥⌘[ | expand or collapse every node |
| `inspector.addComponent` | Edit | ⇧⌘A | the searchable Add Component popup; adds to every selected entity in one transaction |
| `inspector.openScript` | Edit | ⌥⌘↩ | the selected entity's first behaviour's source in the IDE |
| `prefab.applyOverrides` / `prefab.revertOverrides` / `prefab.unpack` / `prefab.open` / `prefab.selectAsset` | Edit | ⇧⌥⌘A / ⇧⌥⌘U / ⇧⌥⌘X / ⇧⌥⌘D / ⇧⌥⌘L | prefab mode on the selected instance (§ Prefab mode): apply the overrides to the prefab (two transactions, two documents), revert them all, unpack, open the prefab's tab, select its asset |
| `inspector.checkCodegen` | Project | ⇧⌘G | run `fynkel codegen --check --project <dir> --json` in the background; the Inspector banner shows the result |
| `scene.tool.<id>` — `select` / `move` / `rotate` / `scale` / `transform` | Edit | Q / W / E / R / Y | the Scene view's tool, from any panel but a text field; one command per registered tool. No ⌘ digit: those are Go to *Panel*, as in Unity |
| `scene.toggleSpace` | Edit | ⌘L | gizmo axes world ↔ local (X in the viewport; scale is always local) |
| `scene.toggleSnap` | Edit | ⌥⌘G | snapping on/off (the steps: long-press the snap button) |
| `scene.toggleHidden` | Edit | ⇧⌘H | hide / show the selection's subtrees in the Scene view (H) — editor-only, never in the document |
| `scene.showAll` | Edit | ⇧⌘U | show every hidden entity (⇧H) |
| `scene.toggleLocked` | Edit | L | lock / unlock the selection against picking and dragging (L) |
| `scene.alignCameraToView` | Edit | ⇧⌘L | write the editor camera into the selected `fynkel.Camera` entity: one transaction (Transform + lens fields) |
| `view.frameSelected` / `view.frameAll` | View | ⌘F / ⇧⌘F | frame the selection's bounds (F) / everything visible (⇧F) |
| `view.toggleProjection` | View | 5 | perspective ↔ orthographic (bare 5, as in Unity; ⌘5 is Go to Project) |
| `view.alignToCamera` | View | ⌥⌘L | look through the selected scene camera (its exact matrix, lens and exposure) |
| `view.reset` | View | ⇧⌘0 | the initial editor camera |
| `view.toggleGrid` / `view.toggleOverlays` / `view.toggleStats` | View | ⌘G / ⌘I / ⌘T | grid and axes / light-camera-collider icons / the stats overlay |
| `view.reload` | View | ⌃⌘R | the preview world emptied and loaded fresh from the live document |
| `view.recook` | View | ⌃⇧⌘R | cook the project again and reload every preview over the new cache |
| `view.saveFrame` | View | ⌥⌘X | one frame of the viewport as a PNG at its physical size (the pixel gate's tool) |
| `assets.undo` / `assets.redo` | Edit | ⌥⌘Z / ⇧⌥⌘Z | undo / redo the last **asset operation** (its own history; the label names it: "Undo Rename coin.prefab.json → gold.prefab.json") |
| `assets.newScene` / `assets.newPrefab` / `assets.newMaterial` | Project | ⇧⌥⌘S / ⇧⌥⌘P / ⇧⌥⌘M | a new document in the Project browser's folder (`New Scene`, `New Scene 2`, …), selected and renaming at once |
| `assets.newVolume` | Project | — | New Volume Profile (Unity's Create → Volume Profile): the starting look of `Document::new` — ambient occlusion on, bloom at intensity 0 — renaming at once; also in the + menu and the folder and asset menus |
| `assets.newPhysicsMaterial` | Project | — | New Physics Material (Unity's Create → Physics Material, ADR-0025): friction 0.6, bounciness 0, both combines Average, renaming at once; also in the + menu and the folder menus |
| `assets.newFolder` | Project | ⇧⌥⌘F | a new folder there (`New Folder`), renaming at once |
| `assets.open` | Project | ⇧⌥⌘O | the selected scene or prefab into a tab; any other asset into the Inspector |
| `assets.rename` | Project | ⇧⌥⌘R | rename the selected asset in place (the sidecar moves with it; the id survives) |
| `assets.move` | Project | ⇧⌥⌘V | move the selected assets to a folder picked from a list |
| `assets.duplicate` | Project | ⌘D in the Project browser | a copy of each selected asset beside it (`coin 1`, …) under a fresh id, selected |
| `assets.delete` | Project | ⇧⌥⌘⌫ | asks first, then deletes the selected assets to `.fynkel/trash`; refused while referenced, with the dependents listed and a **Delete anyway** |
| `assets.reimport` | Project | ⇧⌥⌘I | run the selected source through its importer with the sidecar's settings |
| `assets.reveal` | Project | ⇧⌥⌘E | reveal the selected asset in Finder (`open -R`; `explorer /select,` and `xdg-open` elsewhere) |
| `assets.copyId` | Project | ⇧⌥⌘C | the selected asset's id on the clipboard |
| `assets.search` | Project | ⌥⌘B | focus the Project browser's search field |
| `assets.toggleView` | Project | ⇧⌥⌘T | the Project browser as folders or as one flat list (the label flips) |
| `app.preferences` | Edit | ⌘, | Preferences |
| `window.commandPalette` | Window | ⌘K | the palette (⇧⌘P is Pause, as in Unity) |
| `window.resetLayout` | Window | ⌥⌘0 | the §22.2 default layout |
| `window.panel.<id>` | Window | ⌥⌘1 … ⌥⌘7 | show/hide Hierarchy, Scene, Inspector, Project, Console, Profiler, Animation (the label flips between Show and Hide; the Asset panel's `import` is retired, item 9c) |
| `window.newCopy.<id>` | Window | ⇧⌥⌘1 … ⇧⌥⌘4 | New Hierarchy / New Scene View / New Inspector / New Project Browser: another copy beside the one used last (item 9e) |
| `window.goto.<id>` | Window | ⌘1 Scene, ⌘2 Game, ⌘3 Inspector, ⌘4 Hierarchy, ⌘5 Project, ⌘7 Profiler, ⇧⌘C Console | Unity's keys for its windows: show the panel and put the keyboard on its list or view (item 9 accessibility) |
| `console.focusSearch` | Window | ⌥⌘F | focus the Console's filter |
| `console.copy` | Window | ⌥⌘C | copy the selected or all visible Console lines |
| `console.openInIde` | Window | ⌥⌘O | open the selected diagnostic in the IDE |
| `console.openInEditor` | Window | ⌥⌘E | open the selected diagnostic's scene or prefab in a tab and select its entity (double-click and Enter do the same for those documents) |
| `help.documentation` | Help | ⌥⌘D | reveal the checkout's `docs/editor.md` |
| `help.about` | Help | ⌥⌘A | editor version and authoring ABI version |

Project-scoped commands are disabled without a project; the registry
refuses to run a disabled command.

**The Console** (`lib/src/console/console_panel.dart`) lists the session's
diagnostics — the validation report, the lock check, the watcher's
re-validations — as `severity CODE: message (file:line:column) — suggestion`,
filtered by text and severity chips (with counts), optionally collapsing
identical diagnostics into one row with a `×N` count; a selected row shows
its detail (document, pointer, suggestion, related). Double-click, Enter or
**Open in IDE** runs the preferred IDE command with `{file}`, `{line}` and
`{column}` filled in (presets for VS Code, Cursor, Zed and
IntelliJ/Android Studio; any template works); a failing IDE launch is
shown in the panel, not swallowed. Copy puts the selected line (or every
visible line) on the clipboard. A project from an older build does not
bury the list: what follows from that — `FYN-D0016` documents behind their
format, whatever those documents report (they are checked against the
current schema), `FYN-P0011` lock mismatches — is folded under one strip
("This project is from an older build: N documents use an older format
and the lock was written by another build. M diagnostics follow from
that.") with **Preview migration…** and **Show them**; every other
diagnostic lists as usual and the counts stay whole.

**Preferences** (`lib/src/prefs/preferences.dart`;
`~/Library/Application Support/Fynkel Editor/preferences.json` on macOS,
`$XDG_CONFIG_HOME/fynkel-editor/` elsewhere): theme (system/light/dark),
density, whether a Scene view scroll or pinch zooms toward the pointer
(on) or the view's centre, the IDE command, the CLI, checkout and Flutter paths when detection is not
enough, Recent (with Clear), Reset layout. Saved on change; the launcher's
checks re-run when a tool path changes.

What the shell does **not** do, by design: run game code (ADR-0016/0018 —
Play is a separate process, item 8) or migrate documents (the CLI's job;
the editor previews and shows the command). Editing arrives with item 3
below.

## Document tabs and the Hierarchy (M4-03)

Explained simply: a **tab** is one open scene or prefab. The **Hierarchy**
is the list of everything inside the focused tab, drawn as a tree. Every
change you make is one **transaction** the authoring session applies,
records for undo, and can save — the editor never edits JSON by hand.

### Tabs (`lib/src/documents/`)

- **Opening.** ⇧⌘O lists the project's scenes and prefabs (search, ↑↓,
  Enter); the Console opens a diagnostic's document into a tab and selects
  the entity it points at (⌥⌘E, double-click, Enter). Opening the same
  document twice focuses the existing tab. A tab strip sits above the
  docking area (`DocumentTabBar`): the kind's icon, the title, a **dirty
  dot** that turns into × on hover, an error count badge from the
  document's diagnostics, and a warning badge while the tab has a disk
  conflict. Middle-click closes; right-click: Close, Close Others, Save,
  Save As…, Revert, Copy Path.
- **`OpenDocument`** is one tab: the `EditorDocument` (the session's
  `DocumentHandle` behind an abstract class so widget tests run on an
  in-memory fake), the tree, the selection (a `LinkedHashSet` with an
  anchor and a lead for ⇧-ranges), the **baseline** — the file's bytes as
  read at open, save or reload — and the conflict, if any. `transaction`
  names the current revision as the base of every `apply`, so a change the
  editor did not see is refused by the `Editor` (`FYN-E0001`) and the tree
  is rebuilt from the truth instead of guessed.
- **Save** (⌘S) writes through the session and re-reads the baseline;
  **Save As** (⇧⌘S) takes a path relative to the **project directory**
  (prefilled with the document's own, e.g. `assets/scenes/main copy.scene.json`;
  a new sub-folder is created), requires the kind's extension and refuses
  to overwrite — the gate caught the first version resolving an
  `assets/`-relative path against the project root, so
  `test/documents_test.dart` now saves into a new folder; **Save All** (⌥⌘S);
  **Revert** (⌥⌘R) asks, then reloads (history gone); **Close** (⌘W) on a
  dirty tab asks Save / Don't Save / Cancel; closing the project asks for
  each dirty tab and Cancel aborts the close. Undo/redo (⌘Z / ⇧⌘Z) are
  **per tab** and the Edit menu's label counts the transactions left
  ("Undo (3)"); inside a text field the same keys run the field's own
  undo, because the command forwards a text `Intent` when the focus is in
  an `EditableText`.
- **The file watcher** (`DocumentTabs` listens to the project's
  `fileChanges`). An external write to an open document is compared with
  the baseline bytes: identical → nothing (the editor's own save);
  different and the tab has **no local edits** → the tab **reloads
  silently** and the tree is rebuilt; different and the tab **is dirty** →
  a **conflict**: the tab shows the badge, ⌘S refuses with "changed on
  disk" and opens the conflict dialog instead, which names the three
  revisions (the one the tab was opened from, the editor's, the disk's —
  the `revision` query), counts the changed lines and shows a coloured
  unified diff of baseline → disk (`lib/src/documents/line_diff.dart`,
  an LCS over the differing middle, a whole-file replacement past 4 000
  lines). Choices: **Reload from disk** (your edits go), **Save As…** (your
  version beside theirs; the original is untouched), **Keep editing**
  (the badge stays; nothing is written). No path overwrites the other
  writer's file without one of those choices — the `FYN-E0001` rule made
  visible.

### The Hierarchy (`lib/src/hierarchy/`)

- **The tree** (`HierarchyTree`) is built **once** per open from the
  document (`id`, `name`, `parent`, `children`, the Transform, the
  component types, the prefab block) and then **patched** from
  `Applied.affectedIds` / `Replayed.affectedIds` through the session's
  `document` query in its `ids` form (the named entities and the ordered
  roots, not the whole document); a parent the patch did not name but
  whose children changed is re-read in a second pass. `builds` and
  `patches` are counted so the tests prove a rename of one entity in a
  10 000-entity scene never rebuilds (`test/hierarchy_test.dart`,
  `test/documents_session_test.dart`). The visible rows are a flat list
  the `ListView.builder` draws at a fixed 24 px, so a 10 000-row scene
  builds only what is on screen. World matrices are composed from the
  Transform chain for the drop policy.
- **Selection**: click, ⌘-click toggles, ⇧-click ranges over the visible
  rows, ⌘A, ↑↓ step (⇧ extends), → expands / ← collapses (or goes to the
  parent), Esc clears (then clears the filter). Rows show a **prefab
  badge** for instances (with the override count) and an **error badge**
  with the entity's diagnostics (from `validate` plus the project report,
  by id or by `/entities/<id>` pointer). An object that is **not active
  in the hierarchy** — its own `active` off, or an ancestor's — is
  greyed like Unity's (`HierarchyTree.activeInHierarchy`; a derived
  entity reads its flag from the expansion).
- **Operations — one transaction each** (`HierarchyModel`): rename in
  place (F2, ⌘R, Enter; Enter or focus loss commits, Esc
  cancels and the next rename starts from the name again, an unchanged or
  empty name is not a transaction; a double-click frames the object in
  the Scene view, as Unity's), Duplicate (⌘D; the topmost selected with
  their subtrees, each copy right after its original and named `Cube
  (1)`), Copy / Cut / Paste / Paste as Child (⌘C ⌘X ⌘V ⇧⌘V, the
  `pasteObjects` command, across documents), Delete (⌘⌫; nothing stays
  selected), Create Empty (⇧⌘N at the root, ⌥⇧N as a child), Unity's
  **3D Object ▸ / Light ▸ / Camera** presets (ADR-0023: the GameObject
  menu at the root; the + menu and a row's menu under the selected
  object, the empty-space menu at the root — submenus that open on hover,
  as Unity's) and Create
  with a native component
  (⇧⌥⌘N or the + menu; the Transform's and the component's **defaults
  come from the session** — the `defaults` query is the component's Rust
  `Default` through the document codec, so what the editor writes is what
  the runtime would assume). Drag-and-drop **reparents** with an explicit
  index: the row's middle drops *into* (appended), the top quarter *above*
  it, the bottom quarter *below* it (or as the first child when the row is
  expanded); a drop onto itself or a descendant is refused; several moved
  entities keep their visible order, and an index counts the slots they
  vacate. A **scene's roots** reorder the same way (since 2026-09-24:
  `reparentObject` with an index moves the entity's member in
  `entities`, whose key order is the roots' order). ⌘Z brings back the
  selection a step was made with and ⇧⌘Z the one it was left with
  (`OpenDocument` keeps one entry per transaction beside the document's
  history). ⌥-click on an arrow opens or closes everything below it; a
  right-click on empty space offers Paste, Create Empty at the root and
  Expand / Collapse All; a row's menu has Cut / Copy / Paste / Paste As
  Child (2026-09-24).
  **Keep world transform** (on by default, ⌥⌘K) rewrites the moved
  entity's Transform in the **same transaction** — `newLocal =
  inverse(world(newParent)) × world(entity)`, only the fields that
  changed — so nothing moves on screen; off, the local values are kept.
- **Search and filter**: the search field (⌥⌘H) matches every word
  against the name, the id and the component types; the filter menu adds
  component types (of the project's authored, allowed set), prefab state
  (instances only / no instances) and **has error**. A match is shown
  with its ancestors dimmed and its path open whether or not it is
  expanded; the status line reads "N entities · M match · K selected".
- The editor never runs game code here either: the tree reads documents,
  and every write is a command the session validates.

- **Several scenes** (2026-09-25, Unity's multi-scene editing): every
  open scene tab is a section under its **header** (the scene icon, its
  name, `*` when unsaved, a fold); the focused one is the **active
  scene**, in bold — where new objects go, what ⌘S saves. A header's
  menu (right-click or its ⋮): Set Active Scene, Save Scene, Unload /
  Load Scene (`DocumentTabs.unload`: the header alone stays, nothing of
  it is listed, drawn or edited; not with unsaved changes, not the last
  loaded scene), Remove Scene (closes the tab, asking), Select Scene
  Asset. A click on another scene's row (or its object in the Scene
  view — the nearest hit of all the loaded scenes) makes that scene
  active and selects there; the old scene's selection goes (one scene's
  selection at a time). Rows dragged onto another scene's header, or
  onto one of its rows, **move** there (`HierarchyModel.moveToScene`: a
  `pasteObjects` there and a delete here, one step in each scene's
  history) and are selected there. The Project's double-click (and Open)
  opens a scene **alone** — the other scenes close, each with unsaved
  changes asking first, Cancel opening nothing — and its menu's **Open
  Scene Additive**, or a scene dropped on the Hierarchy, adds one with
  the active scene kept. Search and filters apply to every section; the
  Scene view keeps one camera between the loaded scenes. Row keys stay
  `row.<id>` (`row.<scene id>.<id>` when two scenes share an id).

What item 3 does **not** do: show a component's fields (item 4, the
Inspector), draw the scene (item 6), or open prefab mode from a badge
(item 7 — it did). Materials open as tabs but have no Hierarchy (a material has no
entities) — the tab shows the document's diagnostics and saves.

## The Inspector (M4-04)

`lib/src/inspector/`. The Inspector shows the focused tab's selection as
one card per component and edits every field through the session; it
never runs game code (ADR-0016/0018) — a behaviour's fields come from the
committed metadata and its schema, a native component's from the
generated schema.

### Where a field's look comes from (`field_spec.dart`, `native_hints.dart`)

One parser, `parseComponentSpec`, turns a component's JSON Schema into a
`ComponentSpec` of `FieldSpec`s. The widget kind is inferred from the
structure (a three-number array is a `vec3`, a `$ref` to `assetRef` an
asset, `enum` a menu, a description that says "quaternion" a rotation) and
then overridden by the ADR-0018 display hints, read in this order — the
last one wins:

1. **The editor's native table** (`native_hints.dart`): `type → field →
   hints` for the 23 built-in components (the audio pair included), in the same `x-fynkel`
   vocabulary the generator writes (Camera `fov_y` is an `angle` in the
   `Lens` group, lights' `color` is a `color`, `MeshRenderer.mesh` an
   `asset(mesh)`, `Collider.layer` a `layerMask`, `Skin.skeleton` an
   `entity`, units `m` / `lux` / `cd` / `kg` / `s`…). It stands in until
   the Rust `#[fynkel(...)]` field attributes exist (ADR-0018 names them;
   `fynkel_schema_derive` and the component structs are the M3 stream's
   territory, not on the editor's closed shared-touch list), and it only
   ever shrinks: `inspector_test.dart` fails when an entry names a type or
   a field no schema has.
2. **The behaviour metadata** (`lib/generated/behaviours.g.json`,
   `behaviours[].fields[]` and `icon` / `category` / `library`).
3. **The schema's own `x-fynkel`** on each property and on the root.

The hint set is ADR-0018's: `label`, `tooltip`, `min`, `max`, `step`,
`group`, `order`, `header`, `unit`, `kind` (`color`, `asset(<kind>)`,
`entity`, `multiline`, `angle`, `slider`, `layerMask`), `readOnly`,
`hidden`, `showIf: "<bool field>"` / `"!<bool field>"`; a behaviour adds
`icon` and `category` (`@FynkelBehaviour`). Fields are ordered by `order`,
then declaration; `group` starts a captioned run, `header` a caption above
one field; `showIf` hides a row while its sibling boolean disagrees.

### The model (`inspector_model.dart`)

`InspectorModel` listens to `DocumentTabs` and the focused `OpenDocument`,
reads the selected entities with the `document` query's `ids` form and
pairs their components **by type and position**: a slot is shown when every
selected entity has that component (a second `Spinner` is slot
`game.Spinner#1`). A field whose values differ across the slot is
**mixed** and drawn as `—`; setting it writes one `setField` per instance
inside a single transaction, so one ⌘Z undoes it everywhere (§22.6).
A typed rotation keeps its angles: the editor remembers the last few
Euler triples typed per rotation (`EulerHints`, Unity's Euler hint — the
document stores only the quaternion) and shows one while the quaternion
still matches, so Y 120 reads 120 after a reselect or an undo rather than
X 180, Y 60, Z 180. An asset field's name **pings** the asset in the
Project browser (⊙, or an empty field, opens the picker).
With several objects, a Rotation axis edits that axis on each object's
own Euler angles (`setFieldEach`: one quaternion per object, one
transaction); an axis that differs shows `—`. Scrubbing a Transform
value's label moves the objects in the Scene view while the pointer
moves (`previewEach` → `SceneViewModel.previewLocals`, the gizmo's
`setTransforms`) and commits once on release (2026-09-24).
Every edit is one transaction: `setField` (a name, or a JSON pointer such
as `/position/1` for one vector component), `setFields` (a whole vector,
Euler angles → one quaternion), `unsetField`, `addComponent` (a native
type takes its Rust `Default` from the `defaults` query, a behaviour its
schema defaults), `removeComponent`, `resetToDefaults`, `renameObject`,
`revertPrefabOverride` (one override by index, or all).

**Validation** happens twice, and the document is never touched by a bad
value. Before a command is sent the model **prechecks** against the spec:
a `min`/`max` violation, a non-integer for an integer, an enum value not
in the list, a wrong vector length, and — because `apply` does not resolve
assets — an **unknown asset** (`FYN-A0004`, checked against the asset
database, sub-asset included). What passes goes to the session, whose
validation may still reject the transaction (the schema's `minimum`, a
stale revision); the model then shows `code: message` from
`Applied.diagnostics`. Either way the field shows the last committed value
again and keeps the message under it until its next accepted edit.

### The widgets (`field_widgets.dart`)

One widget per kind; each shows the committed value it is given and
reports an edit **once per gesture**:

| Kind | Widget |
| --- | --- |
| `number`, `integer` | text (plain numbers or `1 + 2 * 3`), a horizontal **drag on the label** (`step` per pixel, ⇧ ×10, ⌥ ×0.1, clamped to `min`/`max`) that previews locally and commits **once on release** — one transaction, one undo step; `unit` as a suffix; `angle` shown in degrees (rounded to three decimals — a float32 radian would otherwise read `45.000002`; the stored value is untouched until an edit) and stored in radians; `slider` adds a slider |
| `boolean` | a checkbox; tristate when mixed |
| `string`, `multiline` | a text field committing on ↩ / focus loss, ⎋ restores |
| enum | a menu of the `enum` values |
| `vec2` / `vec3` / `vec4` | X Y Z W numbers, each a pointer write on its own; the per-axis fields carry no unit suffix (too narrow for `20 m`) — the row's label shows it once |
| quaternion | **Euler degrees** (X, Y, Z, `q = qz·qy·qx`) over the stored `[x, y, z, w]`; each axis commits the whole quaternion; the shown triple is kept across a commit so dragging Y never snaps X |
| `color` | a swatch (linear → sRGB for display) with the hex next to it; the picker is HSV + hex + alpha for four components; values are written linear. As Unity's, the Scene view follows every change while picking (`preview.setComponents`, no transaction); OK writes one transaction, Cancel or a click outside puts the world back (`restoreComponents`) |
| `assetRef` | the asset's name, `…` opens a **searchable picker** filtered by `asset(<kind>)` (a source offers its matching sub-assets; an audio source is offered whole as a `sound` — `AudioSource.clip` takes no sub), Clear when nullable, and a `DragTarget<AssetDragPayload>` for the Project browser's rows (item 5) |
| `entity` | a name field with a menu of the document's entity names |
| `layerMask` | a check-list of the project's `layers` (Everything / Nothing), the number beside it — a collider's `mask` ("Collides with"), a camera's or light's `culling_mask` |
| array, object | rows per item / per child; a list's `+` copies the last item (Unity's rule; into an empty list the item's default, and an asset item — which cannot be None — is picked first), each item moves up or down and is removed; `any` and keys the schema does not list fall back to JSON text |

Numbers round-trip **byte-exactly**: a field that held `1` stays an `int`,
one that held `1.0` stays a `double` (`jsonNumber`), so setting every
field of the fixture's scenes and the reference game's playground to its
own value and saving changes no bytes (`inspector_session_test.dart`).

### The panel (`inspector_panel.dart`)

Header with the **active checkbox** (`inspector.active`, Unity's; a
dash when the selection disagrees), the name (editable) and id (copy), or
"N entities — …" for a multi-selection (above 200 entities a count
only). The checkbox is one transaction for the whole selection:
`setObjectActive` on a document entity (a prefab instance included: its
own flag wins, ADR-0022), a `setActive` override on the instance for a
derived one (docs/design/set-active.md). One collapsible card per
slot with the type's icon (the `icon` hint or a per-type default), its
description as a tooltip, the field rows in groups, a component's boolean
`enabled` as a checkbox in the header (Unity's; the Emitter and the
Volume have one), and a menu: **Copy Component** / **Paste Component
Values** (any component of the same type, in any document, one
transaction; a key the copy lacks goes back to its default), **Reset to
Defaults**, **Open Script** (a behaviour: `package:<pkg>/<path>` from the
metadata resolved to `<project>/lib/<path>` when `<pkg>` is the project's
package, then the configured IDE command at the line of its `class`
declaration), **Copy Type Name**, **Move Up** / **Move Down** (one
object's own components, `moveComponent`; the Transform stays first, and
not during Play or on a prefab instance, 2026-09-24; since 2026-09-25
also Unity's drag: a header dropped on another card's takes its place,
a line showing where — after it moving down, before it moving up),
**Remove Component**. A prefab instance shows a **Prefab instance** section
listing the override ops in bold with a Revert arrow each and **Revert
all** (per-field bold inside the expanded instance is item 7's). **Add
Component** (⇧⌘A) is a searchable popup over the project's authored,
allowed native components and every behaviour, grouped by category with
descriptions; a type an entity already holds is greyed out with "already on
Robot" (the document holds **one component per type per entity**,
`FYN-D0204`; the model refuses before asking the crate). A refusal that is
not one field's shows under the cards **with the reasons, not the summary**:
the crate answers `FYN-E0005` "it would leave the document with N
error(s)" and carries the real errors as `related` diagnostics —
`describeRejection` in `open_document.dart` prints those (code, message,
suggestion) and skips the summary, for the tabs' `lastError` and the
Inspector alike. That message goes away with the selection, with the next
accepted transaction from anywhere (a Hierarchy rename counts), or with its
×. The gate found all three: a second Transform was offered, refused with
the bare summary, and the summary never cleared.

**Layer and tags in the header** (ADR-0020, L4). Under the name (or the
multi-selection line) the header has a **Layer** row, a dropdown of the
project's `layers`, and a **Tags** row, the object's tags as chips with a
× each and a `+` menu of the project's `tags` (check = every selected
entity has it, dash = some do). `object_header.dart` draws them. They are
`fynkel.Layer` and `fynkel.Tags`, which never get a card and are not in
Add Component (`headerTypes`). `InspectorModel.setLayer`/`setTag` write
them in one transaction for the whole selection: a `setField` where the
component exists, an `addComponent` (or its override on a derived entity)
where it does not. A layer set to `default` on an object without the
component writes nothing. Several entities with different layers show `—`,
and a tag only some carry shows as a dim chip. An index the project does
not declare is refused before any command with `FYN-H0012`, because the
crate does not check the project's lists at apply (`fynkel validate`
does).

**Tags and Layers** (`project/tags_and_layers_dialog.dart`). The layer
menu's **Edit Layers…** and the tag menu's **Edit Tags…** open one dialog
with both lists side by side. Each row is a name field with move up, move
down and remove, and **Add layer** / **Add tag** sits under each list (32
at most). `default` is layer 0 and is fixed. `ListDraft` keeps track of
the old index each entry comes from. Apply sends `setLayersAndTags` with
`layersFrom`/`tagsFrom` (`EditorProject.setLayersAndTags`). Apply is
disabled while nothing changed, and while a name is empty or appears
twice. A rename or an addition only writes `fynkel.project.json`. A move
or a removal shows a warning first, because it rewrites every scene and
prefab that uses the entries: objects on a removed layer go to `default`,
a removed tag is taken off, masks follow. It is refused (the reason shown
in the dialog) while one of those documents has unsaved edits. The open
documents it rewrote are re-read, and ⌥⌘Z (the asset undo) puts
everything back.

**The codegen banner.** Two signals, cheapest first: *committed drift* —
the metadata's inline schemas compared with the `schemas/behaviours/`
files the session loaded (instant, no process; `committedDrift`) — and the
CLI check, `fynkel codegen --check --project <dir> --root <repo> --json`
(one of the editor's three commands; the CLI runs `build_runner` in the
game package and restores the committed files, so a check never writes),
run **once in the background when a project with behaviour metadata
opens** and on **Check Generated Behaviour Files** (never on a timer).
Stale or failed, the banner names the reason, shows `fynkel codegen
--project <dir>` with a Copy button and **Check again**; the Inspector
keeps showing the last committed fields underneath. A project without
`behaviours.g.json` (the fixture) has nothing to check and no banner.

What item 4 does **not** do: per-field override marks inside an expanded
prefab instance (item 7 — landed there), the runtime view of a behaviour during Play
(after V0). Drag-in from the Project browser, left open here, landed with
item 5 (a row dropped onto an asset field of the matching kind).

## The Project browser (M4-05)

Blueprint §19.2e item 5. `lib/src/project_browser/`: `project_browser_model.dart`
(the model over `EditorProject`), `project_panel.dart` (the **Project**
panel), `asset_panel.dart` (the **Asset** panel — item 2's Import
placeholder, filled), `asset_preview.dart`. Everything that changes a file
goes through the session's `operate` (§ documents.md "The authoring
session"): the editor never renames, deletes or writes an asset itself,
so every operation leaves `fynkel validate` clean or is refused before it
touches anything. Folders are the one exception — a folder is not an
asset, has no id and no sidecar, so **New Folder** and an empty folder's
rename/delete are plain directory work (the precedent is Save As creating
directories); renaming a folder that holds assets moves each asset through
the session, one undo step each, and removes the empty tree after.

### The model (`project_browser_model.dart`)

`ProjectBrowserModel` listens to the project and re-reads its entries,
folders (walked from the asset root, dot-directories skipped), details and
history whenever `EditorProject.assetsRevision` moves — the
`SessionProject` bumps it on every `AssetsChanged` event, which the
watcher's incremental `rescan(paths)` and every operation raise (the
package's event stream is synchronous, so the browser is current the
moment `operate` returns). Two **views**: *folders* (a tree beside the
shown folder's sub-folders and assets) and *list* (every asset, one flat
list with its folder path). The **search** matches path, name, id and
sub-asset ids, case-insensitively; from a folder it looks under that
folder, from the root or the list everywhere. **Kind filters** are a set
over `scene`, `prefab`, `material`, `rig`, `blend`, `texture` (image
sources), `model` (glTF/FBX), `sound` (audio), `other`, with live counts;
**sort** by name, kind, path or modified time, the same choice again
flipping the direction. Selection is a set with a lead: click, ⌘-toggle,
⇧-range, ↑↓ (⇧ extends), ⌘A; a selection, lead, renaming row or shown
folder that a rescan removed is pruned. **Previews**: an image source
decodes its own file (`Image.file` at twice the row height, cached by
Flutter; a file that fails to decode falls back to the icon), a material
is a **swatch** of its `base_color` (white for a PBR material that leaves
it default; a custom shader gets the icon), everything else the kind's
icon from `assetKindIcon`. Material preview spheres and mesh thumbnails
are *not in M4* by the blueprint.

**What a row drags** (`dragPayload`): a document by its kind, an audio
source as a `sound` and an image as a `texture` — both are referenced
**whole** (a material's texture slot names the image's own id; the fix in
`AssetEntry.wholeKind` also lets the Inspector's asset picker offer
textures), an imported model as its `#prefab`, any other source with
exactly one sub-asset as that sub-asset; the Inspector's asset page drags a
source's parts one by one. The payload is the Inspector's `AssetDragPayload`
(`id`, `sub`, `kind`, `name`; `toRef()` is the document's `{ asset, sub? }`).

**Operations**, each a session call behind `_guard` (a refusal becomes
`lastError` with the session's reasons; success clears it and any pending
delete): `newDocument(kind)` finds a fresh `New Scene`, `New Scene 2`, …
in the shown folder and leaves the row selected and renaming;
`rename(id, typed)` re-appends the document suffix or the source's
extension the user left off (`level1` → `level1.scene.json`, `bot.png`
for a `.glb` → `bot.png.glb` — a typed extension never changes the real
one) and strips `/`; `move(ids, folder)` passes `folder/file` (or the bare
file name for the root, which the session refuses as a target) per asset
and stops at the first refusal; `renameFolder`, `newFolder`, `deleteFolder`
(refused while the folder holds assets — the message counts them);
`delete(ids, force)` first asks the project for every target's dependents
and, when any is referenced, **touches nothing** and parks a
`DeleteRefusal` (ids, the dependents as `FYN-A0004` diagnostics with
document, pointer and id) for the panel's dialog — `force` moves them to
`.fynkel/trash` one by one; `setImportSettings`, `reimport` (the
importer's `ok: false` is a *result*, shown, not an error), `undo`/`redo`
over the **asset history** (`AssetHistory`: labels and both stacks). A
read-only project refuses everything with the lock's reason.

### The Project panel (`project_panel.dart`)

Toolbar: view toggle, search (⌥⌘B), the kind filter menu with counts and
a badge, sort, **New ▾** (Scene, Prefab, Material, Folder), Undo/Redo of
asset operations with the operation's label in the tooltip. Below, in the
folders view, the tree (`assets` root and every folder, the shown one
highlighted) beside the list; a folder opens on one click, a document
opens in a tab on double-click (or ⌘↓, Unity's), any other asset shows in the
Inspector. Rows show the file name (renaming changes the file, never a
document's internal `name`, which dims beside it when it differs), a
preview, the folder path in the list or while filtering, `N sub` with
the sub-asset ids as a tooltip, and the kind. **Inline rename** (↩ or F2
as in Unity, the menu, or right after New): ↩ commits, ⎋ cancels, losing
focus commits. Keys (2026-09-24, Unity's): ↑↓ ⇧↑↓ ↩/F2 rename, ⌘↓ open,
⌘D duplicate (`duplicateAsset`: `coin 1` beside it under a fresh id, the
copies selected), ⌘⌫ (and ⌫/⌦) delete after **Delete selected asset?** —
a referenced asset goes straight to the "still referenced" dialog below —
⌘A ⎋ ←(up a folder). An asset field clicked in the Inspector **pings**
its asset here: shown in its folder, scrolled to and outlined, nothing
selected. Context menus: an asset's
Open / Show in Inspector / Reveal in Finder / **Find References In
Scene** (not a scene: the focused document's Hierarchy comes forward
searching `ref:<asset id>:<name>`, as Unity's — the objects whose
components reference the asset or whose prefab it is, prefab instances'
contents included; × clears it) / Rename / Move to… / Delete /
Reimport / **Set as Startup Scene** (a scene: the one `fynkel run`
starts with, one asset-history step) / Copy id / Copy path / New … Here; a folder's Show / Reveal /
New … Here / Rename Folder / Delete Empty Folder; the empty area's New …
Here. **A model's parts**: a source with more than one part (a glTF's
meshes, materials, images, clips) has Unity's ▸ before its icon (→ and ←
on the selected row); open, its parts are listed under it by key, each
one a `Draggable<AssetDragPayload>` of `{asset, sub}` onto an asset field,
and a click shows that part's page in the Inspector.
**The grid** (2026-09-25, Unity's): the **zoom slider** at the status
bar's right turns the list into tiles, bigger further right (48 to 128
points), the list again at its left end; `ProjectBrowserModel.zoom` is
remembered with the folder. A tile is the picture over the name and does
what the row does — click, ⇧/⌘-click, double-click, right-click, inline
rename under the picture, drag, a folder tile's drops; a model's ▸ sits
at its tile's right edge and its parts follow on a darker ground; ← → ↑ ↓
move across and down (a row is the grid's column count). Models,
prefabs, materials and a model's meshes and materials are drawn by
`fynkel thumbnail` (`project_browser/thumbnails.dart`, `ThumbnailCache`,
one per project shared by the browser copies): the tiles ask as they are
built, the asks wait 150 ms and go as one `fynkel thumbnail --json`
batch, the PNGs go to `.fynkel/thumbnails/<id>[__<sub>].png` with
`index.json` recording each one's source size and modification time; a
changed file is drawn again (the old picture stays until then), a refusal
— a CLI without the command, a failed cook — leaves the kind's icon and is
not asked again until the file changes. A texture is its own picture.
**Drag and drop**: an asset row is a `Draggable<AssetDragPayload>`
(dragging a selected row drags the whole selection, the feedback says "3
assets"); every folder row in the tree and in the list is a drop target
that moves through the session (the target lights up only when the move
would change something). The **Delete refused** dialog groups the
dependents by document path with each pointer, and offers Cancel or
**Delete anyway**; the error bar shows any other refusal with its reasons
and a ×; the status line counts `shown of total assets in <where>` and the
selection, with an up arrow inside a folder.

### Scripts and Create → Behaviour (roadmap Phase 2 item 1)

Below the asset folders the tree has a **Scripts** root: the game's
`lib/` and its folders (`scripts.dart`, `ProjectBrowserModel`'s Scripts
section). Scripts are not assets. The session never indexes them, so the
browser lists the directory itself and reads it again when the watcher
reports a change under `lib/`. The list shows the shown folder's
sub-folders and `.dart` files. Kind is `behaviour` for a file the
committed metadata names as a behaviour's `library` (its class names
under Contents), `generated` for `lib/generated/` and `*.g.dart`, else
`script`. A search covers the paths and behaviour ids under the shown
folder, and the list view shows every script. A double-click or ↩ opens
the file in the IDE (the Console's opener, `IdeLauncher.open`). The
context menu has Open in IDE, Reveal in Finder, Copy path and New
Behaviour Here…. Scripts only open: they do not move, rename or delete
here, which is the IDE's job. The folder shown in Scripts is remembered
with the rest of the browser.

**New ▾ → New Behaviour…**, for an editable Dart package with `lib/`:

1. A dialog asks for the class name (UpperCamelCase, checked as typed)
   and shows the file it makes: `snake_case.dart` in the shown scripts
   folder, else `lib/game/` when there is one, else `lib/`.
2. The file is written from the template: `@FynkelBehaviour(id:
   '<namespace>.<Name>')`, one `@Expose` field, and `start` and `update`.
   The namespace is the one every existing behaviour shares, else
   `game`. A file, or an id, that is already there is refused before
   anything is written.
3. The file opens in the IDE at once.
4. `fynkel codegen --project <game> --project-only --json` runs
   (`CodegenCheck.generate`; seconds, shown in the status line). This is
   one of the two times the editor runs a command that writes (ADR-0016
   amendments of 2026-09-24; the other is a saved script, below). `--project-only` never touches the engine's own
   generated files.
5. The session's `registerBehaviour` adds the id to the project's
   component registry (one asset-history step, so ⌥⌘Z takes it back)
   and re-reads the metadata and schemas. Add Component lists the type
   at once.

Each failure is shown in the error bar: codegen failing (the file stays
written), or codegen not listing the type (a compile error in the file).

**Drag a script onto an object** (Unity's script drop). A behaviour's
file in Scripts drags the behaviour (`scriptDragPayload`: of several in
one file, the one named after it, as Unity asks; a file without one does
not drag). Dropped on a Hierarchy row or on an object in the Scene view
(`HierarchyModel.addScript`), it is added with its schema defaults — an
`addComponent` override when a prefab puts the object there — in one undo
step, and the selection stays. Dropped on the Inspector it goes onto the
selection through Add Component. An object that already has the script
refuses the drop (FYN-D0204).

**A saved script updates the Inspector** (Unity's recompile; the second
ADR-0016 amendment of 2026-09-24). `CodegenCheck.recompileOnSave` listens
to the project's file changes. A `.dart` file under `lib/` that is saved,
added or deleted (not `lib/generated/`, not `*.g.dart`) runs the same
`--project-only` codegen 0.5 s later, after any command already running.
Then the session's `reloadBehaviours` re-reads the metadata and schemas,
so a new `@Expose` field is in the Inspector a few seconds after the save.
The toolbar reads *Updating scripts…* meanwhile. A run that fails (the
code does not build yet) shows in the toolbar and reloads nothing. The
game in the editor hot-reloads the regenerated file as well: the
generated types are getters, so a new behaviour or field reaches the
next Play.

**Kept warm** (the third ADR-0016 amendment). `CodegenCheck.watch` keeps
`fynkel codegen --watch` running from open, so a save reaches the
Inspector in about a second instead of 3 s and more. Its first build is
the check at open. Every build that wrote reloads the metadata. Create →
Behaviour waits for the build its new file starts. A script that does not
compile reads *Script error* in the toolbar and fills the Inspector
banner. Play refuses until the script is fixed, as in Unity, because
build_runner removes the generated files while a build fails. Closing the
editor ends the watch, and the CLI puts the last good files back. A CLI
without `--watch` falls back to `recompileOnSave`.

**A hot reload that fails** (Unity's compiler errors). When `fynkel
editor` reports `reloadFailed`, `CodeReloadStatus.errors` holds the
compiler's `file:line:column: Error: …` lines (`CompileError.parse`; a
message without one is one error). They are error rows in the Console
with the code `compile`, and ↩ on a row opens the file at that line in
the IDE. The notice links to the Console. Play and Build And Run are
refused while any are left (*All compiler errors have to be fixed before
you can enter Play mode*), since the game in the editor still runs the
old code. The next reload that goes through clears them.

A finding while testing: a `GestureDetector` that catches the empty-space
click (clear the selection, focus the list) must be `excludeFromSemantics`
— with its semantics node in the tree, rows leaving the `ListView` while
a menu route pops trip the framework's `'node.built'` assertion (the kind
filter did exactly that; the Hierarchy's identical detector never hit it
because its rows were never Text-only). It has no semantic meaning anyway.

### The Asset panel (item 5; since item 9c the Inspector's asset page)

What follows is what item 5 built as its own panel. Item 9c moved all of
it into the Inspector (`lib/src/inspector/asset_pages.dart`, § One
Inspector) — the same content, with the import settings' Revert and
Apply on a bar pinned to the page's bottom; the panel and its file are
gone. Item 5's panel showed: header (name, kind, id with Copy), the actions Open (scenes and prefabs),
Reveal in Finder, Reimport (sources), Delete; **File** (path, size,
modified); for a source **Import**: importer and version, the sidecar
path, the importer's known settings as controls — `fynkel.image`
`colorSpace` (sRGB for colour, linear for data; the default writes no
key), `fynkel.audio` `stream`, `fynkel.gltf`/`fynkel.fbx`
`allowExternalFiles` and (Unity parity, 2026-09-24) Scale Factor, Normals
(Import / Calculate, with the Smoothing Angle), Import Animation — plus
the raw `settings` JSON for everything else;
**Apply and Reimport** writes the settings through the session (one asset
history step, undoable) and reimports at once, the result box showing the
importer's summary (meshes, images, clips, …) or its refusal with the
diagnostics, **Revert** drops the draft. As in Unity, leaving an asset
whose settings changed without Apply asks **Unapplied import settings for
'assets/…'**, with Revert and Apply; the change is never dropped silently.
In a narrow Inspector the bar's buttons go under its message; **Sub-assets**, each row
draggable onto an Inspector field or the Hierarchy; a material's
fields, edited in place (§ The material Inspector below); a scene's or prefab's entity
count; **Referenced by (N)** grouped by document, each row opening that
document — a scene or prefab in a tab with the entity selected
(`openReference` in `app.dart`: the pointer's `/entities/<id>` is
revealed), any other document in the browser.

### Drop targets elsewhere

The Inspector's asset fields were `DragTarget<AssetDragPayload>` since
item 4 and accept a payload of their kind (a `texture` field takes an
image, a `mesh` field a `#mesh/Cube`). The **Hierarchy** now takes a
`prefab` — a prefab document or an imported model's `#prefab` — onto a
row (a child of it) or the empty area (a new root): one
`instantiatePrefab` transaction with the instance named after the file,
the parent expanded and the instance selected; refused into a prefab
instance's subtree, into a read-only tab, onto the prefab document itself,
and for every other kind (`HierarchyModel.canDropAsset`). The **Scene
view** (item 6) takes the same payload onto its picture — the instance
lands where the pointer's ray hits (§ The Scene view › Prefab drop).

**A material dropped on an object** (Unity parity, Tier 2, 2026-09-24)
becomes its MeshRenderer's material: onto a Hierarchy row (the whole row
lights up; refused on an object without a MeshRenderer), or onto the
Scene view's picture, where it goes onto the nearest mesh under the
pointer — inside a prefab instance the child the ray met (the pick's
`object`). While it hovers over the picture that object draws with it
(`hoverMaterial` → `setComponents`, nothing written; leaving puts it
back). The drop is one transaction — `setField`, or a `setField`
override on the instance for an object a prefab puts there
(`HierarchyModel.assignMaterial`) — and the selection stays.

What item 5 does **not** do: material preview spheres and mesh
thumbnails (not in M4). The material editor came with the Unity parity
pass (§ The material Inspector).

## The Scene view (M4-06)

Blueprint §19.2e item 6. `lib/src/scene_view/`: `scene_view_model.dart`
(the model — one `SceneTab` per open scene or prefab tab, the camera, the
tools, picking, the gizmo drag, the pointer and key handling),
`scene_panel.dart` (the **Scene** panel: toolbar, the `FynkelViewport`, the
2-D overlay, the stats box, the status line), `scene_math.dart` (vectors,
quaternions, the projection matrices — the runtime's own formulas —, the
`EditorCamera` rig, `ViewProjection`, `Ray`), `gizmo.dart` (the gizmo's
geometry and its projection to pixels), `editor_state.dart` (the editor
state file), `png.dart` (a PNG writer and reader for Save Frame and the
tests). The picture is the session's **edit-preview world**
(§ documents.md "The edit-preview world"): a real `fynkel_core::World` in
edit mode that holds the *live* document — dirty edits included, prefab
instances expanded, imported hierarchies from the importer — and never
runs a system. The editor never steps it, never submits a command to it
and never reads a cooked cache itself; every change reaches it through
the session, which brings every open preview up to date before `apply`,
`undo`, `redo`, `reload`, `save`, `rescan` or `operate` returns.

In five-year-old terms: the Scene view is a window onto a frozen copy of
the scene. The copy is redrawn from the document after every change; the
camera you fly around with is the editor's own and is never written into
the scene unless you ask (⇧⌘L); dragging a thing shows it moving in the
window and writes the document **once**, when you let go.

### The world behind the picture

`EditorDocument.openPreview()` → `PreviewBackend` (`SessionPreview` over
`ScenePreview` in production, `FakePreviewBackend` in the widget tests).
The model opens a tab's preview the first time the tab is focused and
closes it with the tab (`SceneTab.dispose`). A material tab has no world
(`FYN-T0003`: "A material has no world"); a preview that failed to open
shows the diagnostic and a **Try again**. The first preview of a session
cooks the project in process (`fynkel cook`'s cooker; the report is on
the preview's `cook`) — a failed cook is announced in the notice bar with
the failed assets counted, the preview draws the rest. The viewport
renders the world by its native address (`FynkelViewportController.
renderHandle(preview.worldAddress)`, viewport ABI v4) and adopts the
world's own asset store, so nothing is configured on the viewport.

**Sync.** The session diffs the world against the description the
document has *now* and applies only the differences through the world's
own command path (`PreviewSyncReport`: `spawned`, `destroyed`, `updated`,
`reordered`, `steps`). When a round makes no progress it falls back to a
fresh load and says `reloaded: true`; the model counts those
(`SceneTab.fallbackReloads`) and shows "Preview reloaded: the incremental
sync could not apply a change (n×)". After every sync or reload the model
re-applies the hidden set (the world's filter is applied at set time
only) and its camera. **Reload** (⌃⌘R) is the same fresh load on demand;
**Rebuild cache** (⌃⇧⌘R) cooks the project again and reloads every
preview over the new cache — the model also runs it on its own when the
project's `assetsRevision` moves (an import, a reimport, a file the
watcher saw). `nothing ticks`: the world's tick counter moves only with
the sync's own steps, never with time — a rigid body stays where the
document put it for as long as the tab is open.

### Resizing the panel (why it used to blink)

The panel draws **on demand** — a camera move, a selection, a sync, never
a clock — so an idle Scene view costs nothing. A drag of the window or of
a splitter, though, hands the viewport a new size on every frame, and
each size reallocates the ring's surfaces and rebinds the Flutter texture;
nothing can render while that is in flight (`renderHandle` refuses a frame
mid-resize), so the picture blinked on and off for the length of the drag.

Two things fix it, and both keep a *game*'s behaviour exactly as it was:

- `FynkelViewport.resizeDebounce` (new; `Duration.zero` by default, which
  is what a game wants — its viewport is the window and it draws every
  frame anyway). The Scene view passes **120 ms**: the last good frame
  stays on screen, scaled by Flutter, until the drag settles, and the ring
  is then resized **once**. The first size is never held — there is
  nothing on screen to scale yet.
- A frame the ring refused because a resize was in flight is **asked for
  again** instead of being dropped (`SceneViewModel.renderFrame`). Drawing
  on demand means a lost frame is lost until the next thing happens; now
  the panel catches up by itself.

### The camera (`EditorCamera`)

An orbit rig — `pivot`, `distance`, `yaw`, `pitch` (clamped short of the
poles), perspective or orthographic, `fovY`, `near`, `far`, an optional
manual exposure — from which `world`, `view` and `projection(aspect)`
follow; the matrices are the runtime's own (`perspectiveMatrix` and
`orthographicMatrix` reproduce `fynkel_core::projection_matrix`, Metal's
0..1 depth), so the picture through the editor camera and the picture
through the scene camera differ only by the camera. Mouse: **⌥-drag
orbit**, **⌥⇧-drag or middle-drag pan** (the pivot slides in the view
plane, one pixel one pixel), **⌥-right-drag or scroll dolly** (⇧-scroll
pans; inside `throughDistance` 0.5 m a perspective dolly keeps its pace
and pushes the pivot ahead — Unity's zoom-through — instead of stalling;
a scroll or a pinch scales about the point under the pointer on the
pivot's plane, so it stays under the pointer, and the zoom-through
follows the pointer's ray — the preference turns that off for the
centre; ⌥-right-drag stays on the centre), and on a
trackpad **two-finger scroll or pinch dolly** — macOS sends both as one
pan-zoom gesture, not as wheel events, so the panel listens for it
(`panZoomStart/Update/End`: the scroll part dollies exactly as the wheel
does, a pinch by its own ratio, spread fingers bringing the view closer;
Flutter drops the system's momentum events, so the scroll coasts on the
fingers' lift-off velocity, decaying at 2/s, until a touch, a wheel, a
click or a frame stops it),
**right-drag look + WASDQE fly**
(a 16 ms timer while the button is down; ⇧ four times faster; the speed
scales with the distance). Keys: **F** frames the selection (its outline
bounds; hidden entities skipped; nothing selected → everything),
**⇧F** (or **F** twice) **locks the view to the selection** (2026-09-25,
Unity's Lock View to Selected: framed, then followed as it moves — dragged,
or in Play; orbit and zoom keep the lock, panning, flying, framing or
selecting something else let it go; Frame All is the View menu's, with no
key), **5** toggles perspective/orthographic (the
orthographic height keeps what the perspective view showed at the pivot),
**⇧⌘0** resets the view. **Look through the selected camera** (⌥⌘L,
`alignViewToSelectedCamera`) takes the lead entity's `fynkel.Camera`
exactly — its world matrix (`EditorCamera.alignTo`: the rig approximates
the matrix and the *exact* matrix is used until the next movement releases
it), `projection`, `fov_y`, `ortho_height`, `near`, `far`,
`exposure_mode`/`exposure` — which is how the pixel gate is driven: the
Scene view through the scene's camera is `fynkel render`'s picture.
**Align selected camera to view** (⇧⌘L, `alignSelectedCameraToView`) is
the opposite, one transaction of `setField`s on the camera entity's
Transform (local = inverse(parent world) × view) and its lens fields;
exposure is left as the scene camera's own. The camera is saved **per
scene** in the editor state file (below), coalesced 400 ms after the last
movement, flushed on close.

### What is drawn over the world

The renderer's overlay (item 9f, ADR-0019; `SceneViewModel.overlay` builds
one `ViewportOverlay` a frame for `FynkelViewportController.overlay`,
drawn after post at the picture's own resolution — never exposed — and
cleared by every render): the **grid**, the renderer's analytic one on
y = 0 (power-of-ten cells from the camera's height and each pixel's
footprint, three levels cross-fading, faded with distance and at grazing
angles, hidden under a floor instead of fighting it), its X and Z axes
red and blue (no Y axis: a line up from the origin only cut through the
scene) — ⌘G; the **icons** of every light (its colour: point range
circles, spot cone, directional arrows) and camera (frustum to 4 m, at a
third of its alpha unless selected), and the collider of each *selected*
entity (its shape, green — as Unity does; every wall of a level collides)
— ⌘I; the **selection outline** (2026-09-25) — the renderer's ring
around what the selected entities draw, Unity's orange
(`GizmoTokens.outlineSelected`), and around what is under them —
children, a selected instance's parts — its grey-blue
(`outlineChildren`), `outlineWidth` (1.5) points wide
(`ViewportOverlay.outline` with the handles of `preview.entities(ids)`,
read once per document revision, selection and visibility, every frame
in Play); an entity without a mesh keeps a cross
(`lines(outlineBoxes: false)`). All of it comes from the world so it is
exact about what is there. Every line is `GizmoTokens.lineWidth` (1) logical
pixels times the panel's pixel ratio, anti-aliased, and the part behind
geometry is drawn at a quarter of its alpha instead of breaking off. The
**gizmo's handles** (item 9g) are drawn into the same overlay, on top
(`GizmoTokens.handleWidth`, filled cones, cubes and plane squares; the
hovered or dragged handle in `GizmoTokens.highlight`, the others dimmed
while a drag runs). Only the **marquee** and the focus ring are still
painted by `_OverlayPainter` in logical pixels. The **stats
overlay** (⌘T) shows the picture's size and the model's own frame time,
the world's counts (`entities` with `live`, `meshes`, `lights`, `hidden`;
`tick`, `loads`, `syncs`, the tab's fallback reloads — the tick moves
only with the syncs' own steps, never with time) and the viewport's last
frame (draws, CPU and GPU p50, frames rendered; placeholders, missing
assets and asset errors when there are any).

### Selection

A **click** asks the world (`preview.pick(camera, aspect, ndc)`): every
entity the ray meets, nearest first — a visible mesh through its
bounding sphere then its triangles, anything else through a 25 cm sphere
at its origin, a hit inside a prefab instance answering as the instance —
minus locked and hidden entities; the nearest is selected and revealed in
the Hierarchy. **Inside a prefab instance** (2026-09-25, Unity's): the
first click selects the instance's root, the next one on the same object
the child under the pointer, the next what is behind; with the instance
or something in it already selected, a click goes straight to the child. A repeated click **on the same pixel (± 3 px) cycles**
through the stacked hits. ⌘-click toggles, ⇧-click adds; a click on
nothing clears (with a modifier held it keeps the selection). A drag of
more than 4 px on empty space or with the select tool is a **marquee**
(`preview.pickRect`: the entities whose projected bounds overlap the
rectangle); the same modifiers. The Hierarchy and the Inspector follow
the tab's selection as always — the Scene view owns no selection state of
its own.

### The gizmo

Tools **Q** select, **W** move, **E** rotate, **R** scale, **Y**
transform (move, rotate and scale at once) (as commands too, with no ⌘ key);
**X** toggles world/local axes (scale is always local);
**⌥⌘G** toggles snapping, a long-press on the snap button edits the
steps (move 0.5 m, angle 15°, scale 0.1 by default), ⌘ held during a
drag inverts snapping for that drag. The gizmo is a constant 80 pt
(`GizmoFrame.screenSize` × the panel's pixel ratio). **Z** toggles
**Pivot / Center** (2026-09-25): Pivot puts it at the active object's
origin and each object turns and scales about its own; Center puts it at
the middle of the selection's bounds, children included, and the group
turns and scales about it. A scale multiplies each object's own scale
(never a shear); the uniform cube scales by the drag, 100 points doubling
it, wherever it is grabbed.

**T** is the **Rect tool** (2026-09-25): a rectangle around the
selection, children included (`GizmoFrame.box`), in the plane of its two
axes most across the view. A corner resizes about the opposite one (⌥
about the centre, ⇧ in proportion), an edge one way; the objects move with
the edge and their own scale is multiplied. A drag inside moves them in
the plane, a drag just outside a corner (`rect.rotate.*`, invisible zones)
turns them about the plane's normal, and a click inside that never drags
selects what is under it (the move handle is a *background* handle, hit
only when nothing else is). **E**'s rotate gizmo has the same kind of
background disc inside its rings (`rotate.free`, `TrackballConstraint`):
a drag there turns freely, one degree per point, about the axis across
the drag; the rings keep their priority. The Transform tool has no
trackball — its move handles are inside.

**Handles and tools (item 9g, ADR-0019 §2).** A tool is a registered
object (`SceneTool` in `scene_view/scene_tools.dart`: id, label, icon,
key, `alwaysLocal`, `hasGizmo`, `handles(ToolContext)` and
`edit(handle, value, context)`); `SceneTools` holds the built-ins —
`SelectTool`, `MoveTool`, `RotateTool`, `ScaleTool` and `TransformTool`,
which is the other three composed (its edits are theirs) — and
`register`/`unregister` more (a duplicate id throws; built-ins stay).
The toolbar, the commands and the keys come from the registry, and
`editor.json` stores the tool by id. A **handle** (`scene_view/gizmo.dart`)
is a value: an id (`move.x`, `move.xy`, `move.view`, `rotate.z`,
`rotate.view`, `scale.uniform`…), a world-space shape (lines, filled
triangles, a faint back that is not hit), a colour, a grab point and a
**drag constraint** that turns the pointer into a value —
`AxisConstraint` (the axis line closest to the pointer ray),
`PlaneConstraint` (a plane, or the view plane), `RingConstraint` (the
angle about the projected pivot; a ring seen from behind flips so a drag
turns the way the pointer goes), `ScreenScaleConstraint` (the pointer's
distance from the pivot) and `RadiusConstraint`. The hit test is in
screen space: the first handle whose triangles contain the point
(edge-on ones skipped), else the nearest line within 8 pt; a plane
square seen edge-on is not offered. The tool's `edit` returns a
`TransformEdit` — the delta, its pivot, the readout and the channel
(move keeps rotation and scale exactly, rotate keeps scale).

**Component handles** (`scene_view/component_handles.dart`) are the same
handles registered per component type (`ComponentHandles`, the
`ComponentHandlers` registry): a point light's range (a dot at each end
of its three circles), a spot light's range (at the cone's end) and outer
and inner angles (on the two circles there — narrowing the outer cone
past the inner one takes the inner along, the inner never passes the
outer), a box collider's six faces (the opposite face stays put, so the
offset moves by half; the entity's scale is honoured), a sphere
collider's radius and a capsule's radius and half height (about the
centre), a camera's field of view — or an orthographic camera's height —
from the top and bottom of its frustum where the world draws it. A mesh
collider has none. They show behind the component card's **Edit** toggle
in the Inspector (`InspectorModel.toggleEditInScene`; offered when one
entity is shown, it is the tab's lead and the document is writable),
**in place of** the tool's gizmo, on the lead entity; the tab remembers
the type (`OpenDocument.editingType`) and forgets it when the lead
changes, a tool key is pressed or **Esc** is pressed (the first Esc; the
next clears the selection). The dots are `GizmoTokens.dotSize` (8 pt)
squares facing the camera; while a drag runs, the dragged shape is drawn
thin in the handle's colour over the world's committed one. A drag
writes nothing until release, then **one transaction** of `setField`s for
the fields that changed (a derived component: `setField` overrides on
its instance); limits the runtime refuses are never produced (a range,
radius or extent of at least 1 mm, angles and a field of view inside
(0, π)). The status line says "editing Collider · Esc to stop".

**Declared gizmos** (item 9h, ADR-0019 §3, `scene_view/declared_gizmos.dart`)
are how game code gets the same without the editor running it: a
behaviour puts `@Gizmo.radius()`, `box()`, `point()`, `direction()`,
`cone(angle: 'field')`, `lineTo()` or `path()` on the `@Expose` field it
draws (with `color: 0xAARRGGBB`, `when: GizmoWhen.always` and
`space: GizmoSpace.world` as options), `fynkel codegen` writes a
`gizmo` hint into the field's `x-fynkel` (docs/codegen.md), and the
Inspector's spec parser reads it as `FieldSpec.gizmo`. Built-in
components get theirs from `native_hints.dart`: an Emitter's `velocity`
is a world-space direction, a Follow's or camera rig's `target` a line
to the entity. The Scene view draws them in the overlay — for the
selection, and for every shown carrier of a `when: always` field — in
the field's colour or `GizmoTokens.declaredGizmo`, 1 pt and faded
behind geometry like the rest: a radius as three circles, a box, a point
as a small cross with a line from the entity, a direction as an arrow, a
cone along −Z with its angle from a sibling field, a line to the entity
a `lineTo` names (by name, as `world.find` does), a path as a polyline
with a cross per point. A value is in the entity's space (moved, turned
and scaled with it) unless the gizmo says `world`; a field the data
leaves out draws at its schema default. When a type has no registered
handles, its declared gizmos are its handles: the card's **Edit** toggle
is offered when one of them can be dragged (a `lineTo` cannot), and a
drag sets the field, clamped to the field's `min`/`max` (whole for an
`int`), one transaction as above — a radius by its dots, a box's faces
symmetrically about its centre, a point, a direction's tip or a path's
points in the view's plane, a cone's length and its angle field. The
specs come from `ComponentSpecs`, the Inspector's cache shared with the
Scene view, cleared when the project rescans (a codegen run shows at
once).

A drag
(`beginDrag(handleId)` → `updateDrag` → `endDrag`) applies it to **every
draggable target about the lead's pivot** (`delta × startWorld`, then
back to the parent's space: `inverse(parentWorld) × newWorld`; move keeps
each target's rotation and scale, rotate keeps its scale) and previews
it in the world only (`preview.setTransforms` — the document stays
clean, `dirty` false, no transaction). Releasing restores the preview
(`restoreTransforms`) and commits **one transaction** of `setField`s for
the fields that changed (compared as the document would write them); a
drag that moved nothing writes nothing; **Esc** cancels and restores;
undo, redo and a reload from outside cancel a drag in flight. The
targets are the topmost selected entities that are unlocked and have a
`fynkel.Transform`, the lead first; children of a selected parent ride
along, so they are not moved twice. When a transform tool shows no gizmo
the status line says why (`noGizmoReason`: "locked (L to unlock)", "no
Transform component — add one in the Inspector to move it", "read-only:
no gizmo"). The readout chip shows the delta while dragging (`+1.25 m`,
`45°`, `×2.00`).

### Hidden and locked (editor-only)

**H** hides the selection's topmost entities and their subtrees (all
hidden → shows them), **⇧H** shows all, **L** locks/unlocks the
selection. Hidden entities leave the picture, the picks, the outline and
the bounds of F/⇧F; locked ones are never picked or dragged but the
Hierarchy still selects them. Both live in the **editor state file**,
`.fynkel/editor.json` (`EditorState`, `formatVersion` 1: the tool, the
axes space, the snap steps, the grid/icons/stats toggles, and per scene
id its camera, `hidden` and `locked`) — never in the document, which
stays clean; a file from a newer editor is read and never overwritten
(`readOnlyReason`), a garbage file starts fresh and is replaced on the
next save (`loadProblem` is mentioned in the layout notice), and
`EditorState.inMemory()` is what the tests use.

### Prefab drop

A prefab (a prefab document or an imported model's `#prefab`) dragged
from the Project browser onto the picture lands **where it hits**: the
nearest mesh under the pointer, else the ground plane y = 0, else 5 m
along the ray (snapped when snapping is on) — one `instantiatePrefab`
transaction through the Hierarchy model with the instance's Transform at
that point, named after the file, appended at the root and selected. The
drop's position comes through the picture's own render box (the toolbar
above it does not shift it) and the browsers anchor their drag feedback
at the pointer, so `DragTargetDetails.offset` *is* the pointer.

### Save Frame

**Save Frame…** (⌥⌘X) asks for a file, renders one frame and writes it
as a PNG at the viewport's physical size (`readFrame`, viewport ABI v4:
RGBA8, top row first) — the pixel gate's tool: through the scene camera
with the grid, icons and selection off it is `fynkel render --size WxH`
of the same scene to the byte (`test/scene_session_test.dart` proves it
for the fixture's Main at 320×200 against a saved render; the notice
says when the grid, icons or an outline are in the picture).

### Commands

Every command below is first-class (menu entry, palette entry, shortcut,
stable id); the ones that write do so in exactly one transaction. The
scene keys without modifiers (Q W E R, X, F / ⇧F, H / ⇧H, L, 5) are the
commands' bare keys (`EditorCommand.key`): they work from any panel — the
Hierarchy, the Inspector's empty space, the Project browser — after that
panel's own keys, never while a text field has focus, and are not native
menu key equivalents (the menu bar would take the letter from every
field). Esc, ⌫ and WASDQE stay the viewport's own.

### What item 6 does not do

Mesh thumbnails and material spheres (M5, the world now exists for
them); the Inspector's numeric drag previewing in the world (done
2026-09-24 for Transform values, see § The Inspector);
vertex/edge snapping and surface-snapped placement beyond the drop point;
multiple Scene views of one document; a game camera as the editor camera
(never — ADR-0016).

## Prefab mode (M4-07)

Explained simply: a **prefab** is a recipe — "a coin is a mesh, a
trigger and a spin". A scene puts **instances** of the recipe down; each
instance may change a few things ("this coin is worth 5") — those are
**overrides**. Prefab mode is how the editor shows the recipe, the copies
and the changes, and moves changes between them: open the recipe in its
own tab and edit it like a scene (every copy follows); in the scene, see
which fields a copy changed (**bold**), throw a change away (**Revert**),
push it into the recipe so every copy gets it (**Apply to prefab**), or
turn the copy into plain entities that forget the recipe (**Unpack**).
Nothing here runs game code; every write is a session transaction the
crate validates.

### Making a prefab (2026-09-24)

As in Unity: **drag an object from the Hierarchy onto the Project panel**
— onto the list (the shown folder) or onto a folder. The object and its
children become `<folder>/<name>.prefab.json` (numbered — `Lamp 1` — when
the name is taken), written with the prefab's own `createObject`
commands and saved, so the authoring log shows every step; in the scene
the object is replaced by an instance of it **with the same id**, one
transaction (⌘Z brings the object back; the prefab file stays, as in
Unity). The object becomes the prefab's root (ADR-0022) with its
Transform and components, so the instance needs no override and
nothing moves; references to the object by id or name still hold. The new prefab is selected
in the Project panel. Refused, with the drop not taken: a derived row,
several objects, a prefab tab (`HierarchyModel.cannotCreatePrefab`).

**Nested prefabs and variants (2026-09-25, ADR-0022 amendment).**
Prefab instances under the dragged object stay instances inside the new
prefab (`instantiatePrefab` in the prefab document), their overrides
kept. A prefab instance dragged itself asks Unity's question —
*Original Prefab*, *Prefab Variant* or *Cancel*
(`showCreatePrefabPrompt`): a variant is a prefab whose root is an
instance of the base (`newDocument` with `variantOf`), taking the
instance's overrides so the scene's instance has none; an original
prefab gets the base unpacked one level. The Project's context menu has
**Create ▸ Prefab Variant** on a prefab (`<name> Variant.prefab.json`
beside it). The Prefab bar reads *Prefab Variant* for an instance of one.

**Every object has a Transform**, as in Unity: Create makes one, a
prefab dropped on the Hierarchy or the Scene view gets one, and the
Inspector's *Remove Component* is disabled on it. A document written by
hand or by an older editor may still hold an object without one (the
playground's `Environment`); the engine accepts that.

### The instance in the Hierarchy

**The instance is the prefab's root** (ADR-0022, as in Unity): the row
with the prefab badge (item 3) is the root — its own name, and in the
Inspector the root's components, overridden fields bold; dragging it with
the gizmo, reparenting it with "keep world transform" or editing its
Transform writes `setField` overrides on the root's Transform. → on it
**opens the rest of the instance**: the root's descendants appear under
it as **derived rows** — italic, in the hint colour while they are
purely inherited, **bold with a count badge** when a component of theirs
has overridden fields, a `+` badge when an override added them. The rows
come from the session's `instance` query (`HierarchyTree.expansionOf`,
one fetch per instance, cached until a transaction touches the instance or
the prefab document changes — an open prefab tab's unsaved edits show at
once in every scene that instances it, through `DocumentTabs`'
prefab-change broadcast). Derived ids are the runtime's derived ids
(`node/<path>` for an imported hierarchy), so a Console diagnostic about an
expanded entity selects its row — opening the instance on the way
(`HierarchyTree.locate`).

Everything you do to a derived row is an **override on the instance** —
one `setPrefabOverride` transaction each, never a write to the prefab:
rename (only an *added* entity — a prefab's own entity is named by the
prefab: "rename it there (Open prefab)"), delete (a `removeObject`
override; on an added entity, the revert of the `addObject` that added
it, together with its added descendants, highest index first, one
transaction), Create Empty Child / Create with component (an `addObject`
override with a fresh id — the new row is selected and renaming). Refused
with a reason in the status line: duplicating a derived entity
("duplicate the instance"), dragging one or dropping onto one, dropping
an asset or another prefab into an instance. **The Scene view's gizmo
works on a derived row too** (gate finding, 2026-09-22): the drag
previews on the derived entity in the world and commits as `setField`
overrides on the instance — `position` / `rotation` / `scale` as whole
keys, the prefab-local ids from the expansion (`HierarchyNode.transformLocal`)
— one transaction, one ⌘Z, the prefab file untouched. The context menu says so in
its labels ("Delete ⌫ (as an override)", "Create Empty Child (as an
override)") and carries the five prefab commands below. A read-only tab
(an imported hierarchy, § below) shows a banner over the tree with the
reason.

### The instance in the Inspector

Selecting a derived entity shows a banner — "Part of the instance *Coin*
(prefab coin) — inherited; edits become overrides", or "Added to the
instance … by an override" — with **Select instance**. Its components are
the expansion's: every field editor works, and a value that differs from
the prefab is **bold** with two extra buttons on its row — **Revert**
(`revertPrefabOverride` of that field's override) and **Apply**
(`field.apply`: the two-step apply of that one override, below). A field
edit on a derived component (or on the instance's own, the root's) is a `setField` override on the instance
(`field` is a whole top-level key of the component's `data` — the crate's
override model — so editing `position.x` overrides `position`);
`unsetField`, Reset to Defaults, Add Component (`addComponent` override)
and Remove Component (the revert of the override that added it, or a
`removeComponent` override) follow the same rule, and multi-object
editing folds them per key. The prefab's own entities cannot be renamed
here either (the name field is disabled). The header's active checkbox
writes a `setActive` override; when one is set the checkbox carries the
accent bar and a **Revert** button (`inspector.active.revert`).

**The Prefab bar** (2026-09-25, `inspector/overrides_popup.dart`,
Unity's layout) opens the instance's Inspector: "Prefab  <name>", **Open**,
**Select** and **Overrides (n) ▾**, above the components (the title over
the buttons in a narrow panel); a broken instance shows its diagnostics
there. **Overrides ▾** is a popup: "Review, Revert or Apply Overrides on
'<instance>'", the overrides grouped by object, each described
("Light.intensity = 5", "+ Component TriggerZone", "+ Object Extra"); a
click opens a row to the prefab's value beside the instance's with
**Revert** and **Apply**, and **Revert All** / **Apply All** close it.
An overridden field is bold **with a blue bar in the margin**, and a
right-click on it offers **Apply to Prefab '<name>'** and **Revert**; a
component the instance added carries a **'+'** and the bar, with **Apply
Added Component to Prefab '<name>'** and **Revert Added Component** in
its card menu. Unpack is the Hierarchy's (its context menu), as in
Unity.

### Apply, Revert, Unpack — what each writes

- **Apply to prefab** (`prefab.applyOverrides`, ⇧⌥⌘A; per field or per
  override row) is the blueprint's two-step rule: the prefab document is
  opened in a tab (in the background, if it is not open) and gets **one
  transaction** with the overrides translated into its own commands
  (`setField`/`unsetField` on the prefab-local component, `addComponent`,
  `removeComponent`, `createObject` with the override's ids,
  `deleteObject`); then the scene gets **one** `revertPrefabOverride`
  transaction (all, or the applied indices highest first). **Two undo
  steps**, one per document, each in its own tab's history: ⌘Z in the
  scene brings the override back while the prefab keeps the value; ⌘Z in
  the prefab tab undoes the prefab's change. Both tabs are dirty until
  saved. Refused, with the reason on the button: a read-only scene, no
  overrides, a missing prefab, an imported hierarchy ("The prefab cannot
  be edited: …"), an instance that does not expand ("fix or revert the
  broken override first"). `lib/src/inspector/prefab_actions.dart` holds
  this once for the Inspector, the Hierarchy and the commands.
- **Revert** (`prefab.revertOverrides`, ⇧⌥⌘U; per field, per row, or all)
  is one `revertPrefabOverride` transaction.
- **Apply to a variant or its base.** An instance of a variant offers one
  Apply per prefab down the chain — *Apply to Prefab Variant 'Crate A'*,
  *Apply to Prefab 'Crate'* — in the field menu and as Apply ▾ in the
  Overrides dropdown (`applyTargets`). Each is `applyPrefabOverride` on
  the chosen prefab; for a base the override goes into the variant first
  and the variant's new root override is then applied on down, so the
  variant keeps none of it.
- **Unpack Completely** (`prefab.unpackCompletely`, Edit and the
  Hierarchy menu) unpacks nested instances too; plain Unpack is one level
  — nested instances stay instances under their derived ids.
- **Unpack** (`prefab.unpack`, ⇧⌥⌘X) is one `unpackPrefab` transaction —
  the session's new command (a shared `fynkel_document` touch, M4-07):
  the instance keeps its id and loses its `prefab` block, taking the
  root's components with the overrides applied; the expanded entities are
  inserted into the document right after it **under the derived ids** the
  runtime already used (so the edit-preview world does not even reload),
  the root's children first among its children. One ⌘Z puts the
  instance back. Refused for an instance that does not expand.
- **Open prefab** (`prefab.open`, ⇧⌥⌘D) opens the prefab document — or the
  imported hierarchy, read-only — in its own tab and focuses it; the tab
  has its own Hierarchy, Inspector, edit-preview world and history like
  any scene tab (a prefab has one `root`; its Hierarchy shows that row
and everything under it). **Select prefab
  asset** (`prefab.selectAsset`, ⇧⌥⌘L) shows the Project panel with the
  prefab document (or the model source / rig the hierarchy came from)
  selected.

### Imported hierarchies

An instance of `<model>#prefab` or `<rig>#prefab` expands like any other
(kind `source` or `rig` in the section header). **Open prefab** opens the
hierarchy the importer derives — `openDocument("<id>#prefab")`, a tab
labelled `hero.glb › prefab` / `column.rig.json › prefab` — as a
**read-only prefab document** (`state.sub == "prefab"`, the banner names
the source): its entities carry the derived `node/<path>` ids, so an
override written in the scene addresses exactly the rows this tab shows.
The Inspector's asset page lists the source's parts; the `prefab` part's
own page has an **Open** button for the same tab. Overrides on such an instance work in full
(the Imported rig of the fixture's Rig scene is the test's case); Apply
is refused because the source cannot be edited; Unpack works and is the
way to "make it mine".

### Missing and broken prefabs

An instance whose prefab is gone (`FYN-D0401`), or whose override no
longer applies (`D0403` a component the prefab lost, `D0404`, `D0407`),
still opens: the instance row keeps its badge and its data, the Inspector
section shows the broken icon and the diagnostics, the Console lists them,
Apply and Unpack are disabled with the reason, **Revert** (and Revert all)
and **Select prefab asset** stay available, and saving the document
without touching the instance writes the same bytes — nothing is
"repaired" behind the user's back. The `instance` query answers `expanded:
false` for such an instance; the Hierarchy shows no derived rows under it.

### Commands

| Id | Menu | Shortcut | Does |
| --- | --- | --- | --- |
| `prefab.applyOverrides` | Edit | ⇧⌥⌘A | the selected instance's overrides into its prefab, then reverted here — two transactions in two documents |
| `prefab.revertOverrides` | Edit | ⇧⌥⌘U | every override of the selected instance dropped — one transaction |
| `prefab.unpack` | Edit | ⇧⌥⌘X | the instance replaced by its expansion as plain entities — one transaction |
| `prefab.open` | Edit | ⇧⌥⌘D | the instance's prefab (an imported hierarchy read-only) in its own tab |
| `prefab.selectAsset` | Edit | ⇧⌥⌘L | the prefab document or model source revealed in the Project panel |

All five are enabled when the selection lies in one instance (the instance
itself or any of its derived rows), and appear in the Hierarchy's context menu.

### Prefab mode in context (2026-09-25)

An instance's **›** at the end of its Hierarchy row (or Open in its
Inspector's Prefab bar) opens its prefab **in context**
(`DocumentTabs.openDocument(context: PrefabContext(tab, instance))`,
remembered by the tab); from the Project a prefab opens **in
isolation**. While a prefab is shown the Scene view has a bar: ‹ back,
the **breadcrumbs** (`DocumentTabs.trail` — the scene, then each prefab
opened in context; an earlier crumb closes the prefabs after it, asking
about unsaved changes), the **Context** menu — Normal / Gray / Hidden
(`EditorState.prefabContext`, Gray by default, remembered) — and
**Auto Save** (`EditorState.prefabAutoSave`, on by default: each change,
undo or Apply is saved as it is made; off, a Save button and leaving
asks). The Hierarchy shows the prefab under a ‹ header. Closing a prefab
goes back to the tab it was opened from. In Normal and Gray the scene is
drawn **around** the prefab (`SceneViewModel._drawOtherWorlds`: the
scene's preview as the renderer's extra world, `ViewportOverlay.extraWorld`,
transformed by `rootWorld × inverse(instanceWorld)` so the prefab's root
sits where the instance was, the instance hidden in that world meanwhile;
Gray desaturates it and halves its brightness in the renderer). With
several scenes open the other loaded ones are drawn the same way, as
they are.

### What item 7 does not do

Nested-instance apply across two levels of prefab (a nested instance's
overrides apply to the inner prefab, the outer document is untouched —
the crate's model); a visual diff of a prefab against an instance;
editing an imported hierarchy (import it with `fynkel import --prefab`
for an authored copy); picking a derived entity *in the picture* (a hit
inside an instance selects the instance, item 6 — select the inner row in
the Hierarchy instead, then the gizmo is there).

## Build and Run and the game Console (M4-08, ADR-0021 P6)

Since ADR-0021, **Play** runs the game inside a project editor (§ The
Game view). This section is **Play → Build and Run** (⌘B, as in Unity):
the game the way a player would get it — a separate app in its own
window, built by the same `fynkel run` you use from a terminal. Use it
for release-like speed, or for a crash that must not take the editor
down. In the plain editor, which has no game compiled in, Play is Build
and Run. The editor does not run any of it: it starts the
program, listens to what the program says, and shows that in the
Console's new **Game** tab. **Stop** kills it. If the game crashes, or
somebody kills it from outside, the editor says so with the exit code and
not one byte of your project changes — the editor never writes a document
on the game's behalf.

The editor and the game stay two processes (ADR-0016, ADR-0018). Play is
the third and last CLI command the editor shells out to, after
`fynkel create` and `fynkel codegen --check --json`.

### The channel (the shared `package:fynkel` touch)

The game talks to the editor over **one pipe in one direction**: its own
stdout, as JSON lines, and only when it was started with
`FYNKEL_EDITOR_CHANNEL=stdio` in its environment. There is no socket, no
shared memory, and nothing ever travels the other way — Stop is a signal,
never a line on stdin. A game started by hand prints exactly what it
always printed; `EditorChannel.enabled` is false and the code is inert.
The records are described in [hosts.md](hosts.md) § The editor channel;
`lib/src/play/game_record.dart` is the editor's reader
(`GameRecord.parse` finds the record anywhere in the line, because
`flutter run` prefixes the app's output with `flutter: `; anything that
is not a record is an `output` row).

### The run (`lib/src/play/play_session.dart`)

`PlaySession` is one run, from ▶ to gone:

- `play()` starts `fynkel run --project <dir> -d macos` with the working
  directory at the project and `FYNKEL_EDITOR_CHANNEL=stdio` added to the
  environment the editor itself has. **The device is named on purpose**:
  without `-d`, `flutter run` chooses among everything connected — a
  phone, every open simulator, Chrome — and when more than one could
  serve it refuses to choose at all (*"More than one device connected"*,
  exit code 1) or picks a phone. Play means "run it here", so the editor
  names the desktop it is running on (`defaultPlayDevice()`); playing on
  a device stays `fynkel run -d <id>` from a terminal until the editor
  grows a picker. (`fynkel run` passes
  `--dart-define=FYNKEL_PROJECT=…` to Flutter, and Flutter passes its own
  environment down, so the variable reaches the game: no CLI change.)
- The state goes `idle → starting` (the CLI is validating, cooking,
  building) `→ running` (the game's own `started` record arrived, with
  its pid) `→ exited`, or `→ stopping` in between when Stop was pressed.
  The toolbar's label is that state in words, with the scene once the
  game says which one it loaded.
- Every line of stdout and stderr becomes a `GameRecord`. The tab keeps
  the last 5 000; older rows are dropped and **counted**, so nothing
  disappears silently.
- `stop()` signals the process **tree** (below) and returns when the
  process is reaped.
- The exit appends one row: `the game exited with code N — nothing in the
  project was changed` when it was not asked for, so a crash reads as a
  crash. `crashed` is true only for a non-zero exit nobody asked for.
- A run can also die **before the game ever starts** — validation, the
  cook, the build, a device `flutter run` could not pick. Nothing came
  over the channel then, so the reason is in the process's own lines:
  `failedToStart` is true, the toolbar says *Did not start (code N)*
  instead of *Exited with code N*, the last row says `the run failed
  before the game started … the lines above are what `fynkel run`
  printed`, and **the Console turns its Output switch on by itself** for
  that run so those lines are on screen. Switching it off again sticks;
  the next Play starts over.
- The stdout and stderr subscriptions are deliberately **not** cancelled
  at the exit: a process that printed and died can still have lines in
  the pipe, and `exitCode` may well complete first.

### Stop kills the tree (`process_tree.dart`)

`fynkel run` starts `flutter run`, which starts the game: killing only
the child the editor holds would leave the window on screen. macOS has no
`setsid` here, so `ProcessTree` reads the machine's table
(`ps -A -o pid=,ppid=`), walks it from the editor's child, and signals
**deepest first** — SIGTERM to that list (plus the game's own pid from
the `started` record), a grace of three seconds, then SIGKILL to whatever
of *that same list* is still alive. The list is taken **before** the
first signal on purpose: once the root dies its children are reparented
to launchd and no later walk would find them.

### Gizmos in Play (item 9h)

**Play ▸ Show Gizmos in Play** (⌥⇧G; `play.gizmos`, remembered in
`.fynkel/editor.json` as `playGizmos`) starts the game with
`FYNKEL_GIZMOS=1`, so a `FynkelGameView` with no `gizmos:` of its own
draws what the game's behaviours draw through `Gizmos` in the game's
window (docs/hosts.md § Flutter host). The Scene view never shows them:
it is the edit-preview world, not the running game's (ADR-0019).

### Build and Run saves first

A game run as its own app reads files from disk, so anything unsaved is
not in the run (Play in the Game view plays unsaved edits; see below).
With dirty tabs, Build and Run asks: **Save and Play**, **Play Without Saving**,
or Cancel (`play_prompt.dart`, `dialog.playSave`). A project that has
**errors** never plays — `fynkel run` would refuse it anyway: the Console
comes up on its **Project** tab with the count, and double-clicking a row
goes to the offending line, as always.

Editing during a run is allowed and marked: the moment a document's
history or the project's assets change, the toolbar and a banner say
*"The project changed since Play started — the game is running the older
files. Changes apply on the next Play."* with Stop and Play again. Merely
opening a tab is not an edit.

The Console's status line names the **game's** pid, not the CLI's. Three
processes are involved — `fynkel run`, the `flutter run` it starts, and
the game itself — and only the last one is worth knowing: it is what a
`kill -9` from a terminal needs, and what the `started` record carries.
Until the game announces itself the line says *cli pid N* instead, so the
two are never confused for one another.

The warning belongs to the run, not to the edit: the moment the game is
gone — Stop, a crash, an exit — the banner and the toolbar's *edited since
Play* suffix go with it (`runningOlderFiles = isActive && editedWhilePlaying`).
Nothing is on screen running the older files any more, so there is nothing
to warn about; the flag itself stays for the run's record. A run that Stop
ended reads **Stopped**, not *Exited with code -15* — that code is the
signal Stop sent (SIGTERM), and the Console's rows still carry it. Both
found by the user's gate.

**Play again** is not ▶. ▶ refuses while a game is up (it is ■ then), so
the banner's button has its own path: it stops the run that is up — the
same process-tree Stop — and only then starts a fresh one, with the save
prompt for whatever the edit left dirty. It is disabled while a Stop is
already in flight. Found by the user's gate, where the button did nothing
at all: it was wired to ▶, which returns immediately while a run is
active.

### The Game tab (`game_console.dart`, `game_console_panel.dart`)

The Console has two tabs now, **Project** (every diagnostic of the
validated project, item 2) and **Game** (this run). Both carry the error
count in the tab label. The Game tab's rows are tick, source, code,
message and the place — the document and pointer, or the asset id, or the
runtime entity handle when the game knew no authored one. Filters,
search, collapsing of repeats and ⌘C work exactly as the Project tab's,
so one Console behaves one way; two switches are its own: **Output**
(the CLI's and Flutter's own lines, off by default — a Play shows the
game, not the build log; a run that dies before the game starts turns
this on by itself, because its reason is in those lines) and **Stats** (the per-second windows, off; the
Profiler is where those belong, item 9, and the last window is in the
status line).

A row that names something opens it on ↩ or a double-click: the document
in a tab with the entity selected in the Hierarchy (through the same
`revealAndSelect` a Project row uses, so a diagnostic about an entity
inside a prefab instance opens the instance on the way), or the asset
revealed in the Project panel.

**An error goes to its line of code.** A behaviour that throws is caught
by Flutter, not by the engine. So while an editor listens,
`FynkelGameView` puts reporters in front of `FlutterError.onError` and
`PlatformDispatcher.onError`, and the handlers that were there still run.
Each error becomes an `exception` log record carrying the Dart stack
(`EditorChannel.error`). `dart_stack.dart` reads the stack back: the VM
form `#0  Player.update (package:game/player.dart:42:7)`, the terse form
of `package:stack_trace`, and `flutter run`'s `flutter: ` prefix. A frame
is the game's own when it is `package:<own>/…` or a `file:` inside the
project. `<own>` is the behaviour metadata's `package`, else the
pubspec's `name`. The row's location is the first such frame
(`lib/game/player.dart:42`). ↩ or a double-click opens the IDE there
(`console.game.open` too), and an error from the IDE command shows in the
status line. The details list the frames (up to 30,
`console.game.frame.<i>`); the game's own frames are links, and the
engine's and Flutter's are grey. Build and Run's printed frame lines
(`output` rows) open the same way.

A game names its documents the way the **cooked** project is laid out —
from the assets root, `scenes/menu.scene.json` — while the editor works in
project paths (`assets/scenes/menu.scene.json`). `_recordDocument` tries
the game's spelling first and the project path second, and takes an
absolute path (a game started from the source tree) as it is. Nothing
else would open: found by the user's gate, where a double-click on a
`loaded` row did nothing at all because the test behind it had used a
path no game ever sends.

### Commands

| Id | Menu | Shortcut | Does |
| --- | --- | --- | --- |
| `play.start` | Play | ⌘P | plays in the Game view in a project editor; in the plain editor, Build and Run; while a run is up it stops it (Unity's toggle) and reads *Stop Playing* |
| `play.buildAndRun` | Play | ⌘B | saves what is dirty (after asking), then runs the project as its own app; **Play again** in the edited-during-play banner repeats whichever ran |
| `play.stop` | Play | ⌘. | SIGTERM then SIGKILL to the game and everything it started |
| `play.showConsole` | Play | ⌥⌘J | the Console with the Game tab in front |
| `play.clearConsole` | Play | ⇧⌘K | empties the Game tab (the run, if any, keeps going) |

The toolbar carries the same ▶ / ■ and the state, which is a link to the
Game tab.

### What item 8 does not do

Choosing what to play on — Play runs the desktop app; a phone or a
simulator is `fynkel run -d <id>` from a terminal; attaching to a game
that is already running (the editor starts it, or it is not on the
channel); hot-reloading a running game with an edit (the
banner says the change applies on the next Play — the game reads files at
load); anything the game does not send (the channel is the SDK's own
emission points: `Diagnostics.log`, the engine's diagnostics, the load
report, `FynkelGameView`'s stats — a `print` in a behaviour is an
`output` row like any other); driving the game from the editor (no
stdin, no commands, ADR-0016); the per-frame graphs — item 9's Profiler
reads the same `stats` records.

## The project editor: `fynkel editor` (ADR-0021 P3a, P3c)

`fynkel editor [dir]` opens a project in **its project editor**: an
editor built with the project's game compiled in, so Play (P4) can run
the game inside the editor, and a save in `lib/` hot reloads it.

- The command generates `<project>/.fynkel/editor_host/` (a `.gitignore`
  of `*`): a Flutter app whose `main` is
  `runFynkelEditor(args, game: EditorGame(build: buildGame))` from
  `package:fynkel_editor`, depending on `apps/editor` and on the project
  by path, with `apps/editor/macos` copied in. It rewrites only files
  whose bytes changed, so the second start reuses the first build.
- A project opens this way when it has a `pubspec.yaml` and
  `lib/fynkel_game.dart` (`Widget buildGame(FynkelLaunch)`, which
  `fynkel create` writes). Without them, or with `--plain`, the command
  runs the plain editor (`apps/editor`) with the directory.
- It runs `flutter run -d macos --machine` (a debug JIT build: hot
  reload works, the frame rate is a debug build's) with `FYNKEL_ROOT`,
  `FYNKEL_CLI` and `FYNKEL_EDITOR_STATUS` in the environment. `--flutter
  <path>` or `FYNKEL_FLUTTER` pick the SDK; otherwise it comes from PATH.
- It watches `lib/**.dart` (a stat every 250 ms, the content read only
  for files whose time changed). When a change has settled for 150 ms, it
  asks for a hot reload. A file rewritten with the bytes it had is not a
  change: the editor's `fynkel codegen --check` at open rewrites the
  generated files and then puts them back.
- Each outcome goes to `status.json` next to the host (version 1: `seq`,
  `state` — `starting`, `running`, `reloading`, `reloaded`,
  `reloadFailed`, `reloadRejected`, `restarting`, `restarted`, `failed`,
  `exited` — `message`, `ms`, `files`). The
  editor follows it (`CodeReloadStatus`) and shows a notice: *Code
  reloaded in N ms* or *Code did not reload*, with the compiler's errors
  (paths relative to the project). The terminal gets the same lines.
- **Opening without a build.** After a build, the command records what
  the app was built from in `build/fynkel_launch.json` (next to the
  Flutter app: the project's `.fynkel/editor_host`, or `apps/editor` for
  the plain editor). It keeps two fingerprints, by file content: the
  **native** side (the Flutter engine, `pubspec.yaml`/`pubspec.lock`, the
  macOS runner, `crates/`, `schemas/`, the packages' build hooks) and the
  **Dart** code (`lib/` of the app, the editor, the packages and the
  project). The next open with the same native side launches the built
  app directly, with the engine switches `flutter run` uses. Its window
  is up in about 0.3 s instead of 11–20 s. `flutter attach` then connects
  in the background (hot reload is on about 2 s later). If the Dart code
  changed since the build, one hot restart (under a second) brings it
  up to date. Until then the window says *Loading code changes…* and
  takes no clicks or keys: the restart starts the editor over, so
  anything done in the old code would be lost (`FYNKEL_EDITOR_UPDATING`
  names the file that says so). Once that editor quits, the command
  rebuilds its app (`flutter build macos --debug`, 10–20 s, the window
  already gone), so the next open starts on the current code. A native
  change, or an app that does not start, means a full `flutter run`
  build, as before. The plain editor attaches only
  when its code needs that restart. `FYNKEL_LAUNCH_TRACE=<dir>` writes
  the files each fingerprint covered.
- A project whose code does not compile cannot build its project editor.
  The command prints the build's last lines and suggests `--plain`.
- `FYNKEL_EDITOR_TRACE=1` echoes every line `flutter run` prints.

### Opening a project hands it over (P3c)

Opening a game project from the launcher, **File → Open**, **Open
Recent** or **New Project** hands it over to its project editor, as Unity
Hub opens a project in its editor. You can start with `fynkel editor` (no
directory) and pick projects from there.

- The editor starts `fynkel editor <dir> --handover` detached, with its
  output in `.fynkel/editor_host/editor.log`. It shows *Opening <name>…*
  with Flutter's progress and quits once the project editor is up. The
  first build of a project's editor takes 15–30 s; later opens skip the
  build and take well under a second (see *Opening without a build*).
- The project editor opens **in the launcher's place**: the same position
  and size (`--window-frame x,y,w,h`, which `fynkel editor` hands the app
  as `FYNKEL_WINDOW_FRAME`; applied again once the window is on screen,
  since Stage Manager places new windows itself). From full screen it
  opens where it last did. As in Unity, the launcher (the Hub) and a
  project's editor are separate apps: the project editor has the game
  compiled in, and a running app cannot take in another project's code.
- Before it does, Play stops, and dirty documents get their save prompt.
  Cancel on the prompt, or on the *Opening* dialog, keeps you here (Cancel
  on the dialog also ends the build and everything it started).
- A build that fails shows why (the compiler's errors) and the log's
  path. **Open Without the Game** opens the project in this editor
  anyway; Play then runs it as its own app.
- `--handover` writes every step to `status.json` from the first moment:
  `building` (with Flutter's progress as `message`), `starting`,
  `running`, or `failed` with why. A project that is not a game fails
  there instead of opening the plain editor.
- A project that is not a game (no `pubspec.yaml` or no
  `lib/fynkel_game.dart`) opens in whichever editor you are in, as before,
  and so does the project editor's own project. The project that
  `fynkel editor <dir>` was started on is never handed over.
- Without the Fynkel CLI, the checkout or Flutter (the launcher's checks),
  a game project opens here, with a notice saying why.

`cargo install --path apps/cli` puts `fynkel` on `PATH`; the installed
copy finds the checkout it was built from, so `fynkel editor` works from
any directory. Install again after pulling CLI changes.

### Restart to Apply (P3b)

A hot reload keeps the editor's state (open documents, undo). Some
changes a hot reload cannot apply, such as a new field on a const class.
Flutter rejects those (`reloadRejected` in `status.json`), and the editor
restarts to apply them:

- With nothing unsaved and the game not running, it restarts at once, as
  Unity recompiles after a script change. The window stays; the editor
  starts over with the new code in under a second.
- Otherwise a notice says the change needs a restart, with **Save and
  Restart** (or **Restart**). Nothing restarts until you choose it.
- **Play → Restart to Apply Code Changes** (⌃⌥⌘R) does the same at any
  time.
- Restarting saves every document first (a save that fails stops it),
  stops Play, and writes `restart.request` next to `status.json`.
  `fynkel editor` takes the file and hot restarts the app (`restarting`,
  then `restarted`).

**The open tabs come back** on every open, not only after a restart, as
Unity reopens the scene you were in. The editor keeps the open tabs, the
one in front and each tab's selection in `.fynkel/editor.json` (the
`session` key). They are recorded as tabs change, and before a restart,
closing the project or quitting. A document that no longer opens is left
out.

## Editor extensions (ADR-0028)

Any package can add to the editor, the game first among them, as a Unity
package carries an `Editor` folder. Its **editor part** lives under
`lib/editor/`, and `lib/editor/editor.dart` defines

```dart
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

void registerEditor(EditorExtensions editor) {
  editor.menuItem(
    'Tools/Code Only/List Coin Kinds',
    id: 'codeOnly.listCoinKinds',
    run: (context) => context.log('…'),
  );
}
```

`examples/code_only/lib/editor/editor.dart` is the working example.

- **Finding the parts.** `fynkel editor` (`editor_parts` in
  `apps/cli/src/editor.rs`) walks the game's `dependencies` (not
  `dev_dependencies`: the host reaches only what the game depends on),
  finds each package through the project's
  `.dart_tool/package_config.json`, and follows each package's own
  `pubspec.yaml` dependencies. The host's `main.dart` imports every
  package with `lib/editor/editor.dart` and hands the list to
  `runFynkelEditor(extensions: [EditorPart('<package>', registerEditor), …])`.
  The order is dependencies first, then by name, and the game last.
  Without a resolved `package_config.json` only the game's own part
  counts.
- **Registering again on every hot reload.** The shell's `reassemble`
  calls every `registerEditor` again, from nothing
  (`EditorExtensionHost.rebuild`, `lib/src/extensions/`). A menu item
  added in a save appears, and one removed disappears, without a
  restart. Adding or removing a whole editor part changes the host's
  `main.dart`, which only a restart runs again. `fynkel editor` reports
  that save's reload as `reloadRejected`, so the editor restarts at once
  when nothing is unsaved and asks otherwise (*Restart to Apply*).
- **Menu items** (Unity's `[MenuItem]`) become ordinary commands, with
  the extension's stable id. They are in the menu bar, the command
  palette, shortcuts and automation.
  - The path is `Menu/Item` or `Menu/Submenu/Item`. The menu is one of
    the editor's; **Tools** (between Play and Window) is there for
    extensions and hidden while empty.
  - An extension's items come after the editor's own in the same menu,
    in registration order, with a submenu's items kept together.
  - `enabled` greys an item out (Unity's validate function);
    `description` is its palette line.
  - A bad path, an id the editor or another extension already has, or a
    taken shortcut leaves the item out, with a Console error saying why.
- **`EditorContext`** is what every callback gets. It speaks for its
  package: `log`, `logWarning` and `logError` put a row in the Console's
  Project tab (code `extension`, the message prefixed by the package).
  The rest goes through the editor's own paths
  (`lib/src/extensions/editor_access.dart`), so an extension's change is
  undone, overridden and shown exactly like the user's:
  - **`scene`** is the document the Hierarchy shows (the running game's
    during Play), read when asked: `objects` in the Hierarchy's order,
    `roots`, `byId`, and `find('Coin')` or `find('Level/Coins/Coin 1')`.
    An `EditorObject` has its id, name, own active flag, parent and
    children, its components' data as the document holds it (with a
    prefab's overrides applied), the prefab it is an instance of, and
    `fromPrefab` for the objects a prefab instance brings.
  - **`selection`** and **`select(objects)`** are the tab's (Unity's
    `Selection.objects`).
  - **`edit(label, (edit) { … })`** collects `setField`, `rename`,
    `setActive`, `addComponent`, `removeComponent`, `reparent`, `remove`
    and `add(ObjectSpec, parent:)`, then writes them as **one
    transaction, one undo step**. Values are the game's own (`Vec3`,
    `Quat`, `AssetRef`, a written `DataAsset`, an enum) or JSON. A field
    of an object from a prefab becomes an override on its instance, as
    in the Inspector. Structural changes to such an object (rename,
    move, delete, a component removed) are refused: open the prefab.
    `add` builds the spec as `world.build` would and a written scene
    would hold it (ADR-0026's `ProjectWriter.entitiesOf`), pasted under
    fresh ids; during Play each object is created in the running game.
    One refused change, or a throw inside the callback, writes nothing.
    `EditResult` has `ok`, the `error` (also in the Console) and the top
    object of each `add`.
  - **`assets`** is the project's asset list (Unity's `AssetDatabase`):
    `all`, `ofKind('prefab')`, `ofDataType('game.CoinKind')`,
    `find('data/coin')` (a path, with or without its extension, or a
    name), the Project panel's `selected`, `ping`, `open` (a scene or
    prefab in its tab, anything else in the Inspector) and
    `createData(path, value)`. `createData` writes a data asset through
    the Project panel's Create and binds the value to it, so a field set
    to it afterwards names the new asset.
  - `add` and `createData` need the game's behaviours and data types.
    The host passes the game's generated `registerBehaviours`
    (`EditorGame.registerBehaviours`) when `lib/generated/behaviours.g.dart`
    exists.
- **A failing extension fails alone.** Every call into a package is
  wrapped. A throwing `registerEditor` skips that package's extensions,
  and a throwing command or `enabled` check is a Console error (with the
  first frame of its stack) while the editor carries on.
- **Preferences › Extensions** lists each package with an editor part
  and what it registered, or why it registered nothing.
- **Editor code never ships in a game.** Only files under `lib/editor/`
  may import `package:fynkel_editor_api` or `lib/editor/`. `fynkel
  validate`, the editor's Console and `fynkel codegen --check` report
  any other file that does as `FYN-P0015`, with its line
  (`fynkel_document::editor_part`). The game's entry point therefore never
  reaches editor code, and tree shaking drops it. The API package holds
  only interfaces and has no plugins.
- The plain editor (no game compiled in) has no extensions. Everything
  an extension shows stays editable without it.

## The Game view: Play in the editor (ADR-0021 P4)

In a project editor (`fynkel editor <dir>`), **Play runs the game inside
the editor**, in the **Game** panel. The panel is a tab beside Scene in the
default layout (⌥⌘8 shows or hides it), as in Unity. The Scene view stays
the editor camera and its tools.

- **Play (⌘P):** brings `.fynkel/cache` up to date with the files. When a
  Scene view is open it cooks in process (⌃⇧⌘R's cook); otherwise it runs
  `fynkel cook --project <dir>`. A failed cook does not play and says why.
  Play then mounts the game's `buildGame(FynkelLaunch(projectDir, startScene,
  inEditor: true))` in the Game view. It starts in the scene open in the
  editor, brings the Game view to the front and gives it the keyboard.
- **Unsaved edits play (P5):** with a Scene view open, Play cooks the
  documents as the editor holds them (`recook` with `live: true`), so there
  is no save prompt and nothing is written to the documents' files; they
  stay dirty. The unsaved text of each dirty document is also written to
  `.fynkel/recovery/<path>` (replaced on every Play), so a crash during the
  run loses nothing. Without a Scene view open, Play cooks the files and
  asks to save first, as a process run does.
- **Pause (⇧⌘P):** stops the game's loop and its audio; like Unity's it
  can be on before Play, and the game then starts paused on its first
  frame (the toggle stays across Stop). **Step (⌥⌘P)**
  pauses and runs one fixed step. Both are in the title bar beside
  Play/Stop and in the Play menu. They reach every `FynkelGameView` of the
  game through `FynkelPlayScope`, so a game needs no code for them.
- **Stop (⌘P again, or ⌘.):** unmounts the game. It disposes its world, so nothing of
  the run survives. The game's own Quit (`FynkelApplication.quit`) does
  the same and never closes the editor.
- **Keyboard:** while the game has the keyboard, the editor's bare keys
  (Q W E R, F, H, …) do nothing (`KeyboardOwner`), as while typing.
  ⌘-shortcuts still work. Click the Scene view to take the keyboard back.
- **Console:** the game's editor-channel records (`Diagnostics.log`,
  diagnostics, stats) come in process to the Console's Game tab
  (`EditorChannel.debugUse`), the same rows as a process run.
- **Code changes:** a save in `lib/` hot reloads the running game (P3a).
  A document edited during the run shows the "changes apply on the next
  Play" banner, as before.
- **The plain editor** (no game compiled in) has the same panel, which says
  so. Play there is Build and Run: the game as its own app (`fynkel run`).
- **Build and Run** (⌘B) is always there too, in a project editor as well:
  the saved files as their own app, for release-like speed or a crash that
  must not take the editor down (§ Build and Run and the game Console).

- **Between runs** the Game view shows what the scene's active camera
  sees, live (P4b). The active camera is the runtime's rule: the enabled
  `fynkel.Camera` with the highest `priority`, the first in the document on
  a tie. The picture is the Scene view's preview world of the focused
  scene, drawn through that camera in its own viewport with no grid, gizmos
  or icons. It redraws when a view changes the world (an edit, a drag, a
  reload), when another document comes forward, and when the panel resizes.
  A hidden Game tab draws nothing and catches up when shown. A camera a
  script moves at start-up is shown where the scene puts it. A scene
  without a camera says so, and so does the plain editor, where Play opens
  its own window. The caption names the camera.

Not yet: the preview ignores a camera's
`culling_mask`, and the editor's hidden entities stay hidden in it.

## The look (item 9a)

The editor is meant to read like a tool — Unity's density, Rider's
structure, Framer's finish — not like a Material demo. All of it comes
from one place, `lib/src/theme/`:

- **`tokens.dart`** — `FynkelTokens`, a `ThemeExtension` reached as
  `context.tokens`: the surfaces (`window`, `panel`, `header`, `field` —
  recessed, darker than the panel it sits in — `popup`, `overlay` and
  `overlayEdge` for things drawn over the picture), one hairline (`line`,
  and `lineStrong` for the focused tab), three inks (`text`,
  `textSecondary`, `textDisabled`), **one accent** `#3E7BFA` used only for
  selection, focus and "on" (`accent`, `accentSoft` for a quiet tint), and
  the diagnostics' own hues (`error`, `warning`, `ok`, `info`) kept apart
  from it. Dark and light are two designed sets, not an inversion. The
  metrics: `row`, `toolbar`, `tab`, `pad` 8, `gap` 6, `icon` 16.
- **Density** (Preferences): Compact, Default or Comfortable — rows of 18,
  20 or 24 px; every row, field and toolbar derives its height from it.
- **`theme.dart`** — the `ThemeData`, every component theme explicit
  (buttons, fields, checkboxes, menus, tooltips, chips, dialogs), the
  platform UI face and the platform monospace for ids, paths, numbers and
  JSON, tabular figures for numbers.
- **`chrome.dart`** — the docking tabs (`tabbed_view`) and splitters
  (`multi_split_view`) from the same tokens instead of the packages'
  presets.
- **`icons.dart`** — `FIcons`, Lucide's 16 px stroke icons
  (`lucide_icons_flutter`, docs/toolchain.md) under the names the panels
  used before; `Icons.` appears nowhere in `lib/`. `@Expose(icon: 'star')`
  in a game names one of these by its short name, so a name is not free to
  change.
- **`focus_ring.dart`** — one accent outline around whatever the
  *keyboard* reached, drawn by a layer over the whole app: it shows after
  Tab, an arrow or a shortcut that moves focus, never after a click, and
  not for a key that moves no focus (W in the Scene view); text fields are
  skipped (their border already says it); it is clipped to the scroll
  views around the focused widget; it appears and moves without animation.
- **`panel_focus.dart`** — which panel holds focus (item 9b): a selected
  row is the accent only there and a quiet tint in every other panel, so
  there is never more than one blue selection on the screen.

**Nothing animates but hover.** Every `showDialog`, `showMenu` and
`PopupMenuButton` passes `AnimationStyle.noAnimation`; tabs do not slide,
tiles do not roll open, the theme switches at once. `test/look_test.dart`
reads the sources and fails on a popup without it.

**Notices** (`lib/src/shell/notices.dart`) replace snackbars: the one line
the editor says something in — *Reveal in Finder failed*, *Reimport
refused*, *Could not open …* — sits at the end of the status bar (the
launcher's own strip before a project is open), with an × and, for an
error, a red icon; it stays until dismissed or replaced by the next one,
because an error that slides away after four seconds is one nobody read.

## The title bar (item 9b)

One bar at the top holds what used to be a project line, a toolbar and a
tab row (`MainWindow._titleBar`, 38 px, key `window.titleBar`):

- **The project** (`window.project`): its name (`window.projectName`, the
  path in the tooltip, a spinner while it validates) opens a menu — the
  path, **Open Project…** (`window.openProject`), up to five **recent
  projects** (`window.recent.<i>`; the current one is left out and one whose
  folder is gone is shown disabled), **Validate Again**, **Reveal in
  Finder**, **Close Project** (`window.close`). A read-only project shows
  its chip (`window.readOnlyChip`) beside it.
- **The document tabs** (`lib/src/documents/document_tab_bar.dart`): a dot
  when unsaved, a close button on the active and the hovered tab only, the
  conflict and error badges, and **+** (`tabs.open`) opening the Open
  Document picker under itself.
- **Play** in the middle of the window: ▶ / ■ and the run's state, a click
  on the state showing the Game tab (§ Build and Run and the game Console).
- **The codegen state** (`window.codegen`): nothing until the project has
  been checked, an icon when the generated code is current, *Run codegen*
  when it is stale or the committed metadata has drifted, *Codegen check
  failed* when the check itself failed; a click checks again
  (`fynkel codegen --check --json`, as the Inspector's banner does).
- **The command search** (`window.commands`, ⌘K): opens the palette.

On macOS the bar *is* the window's title bar. `MainFlutterWindow.swift`
makes the native one transparent with its title hidden and the content
full-size, and adds an empty `NSToolbar` in the `.unifiedCompact` style —
that is what makes the native bar 38 pt tall with the traffic lights
centred in it. The window is **not movable by its background**
(`isMovable = false`): otherwise AppKit starts a window drag from a press
anywhere in the bar, a Flutter button included. The bar's empty space
(`window.drag`, a detector *under* the bar's contents, so only a press
that no control took reaches it) asks the native side over the
`fynkel.editor/window` channel: `startDrag` (`performDrag(with:)` with the
current event) and `titleBarDoubleClick` (the system's *double-click a
window's title bar to* setting: zoom, minimise or nothing). The bar leaves
room for the traffic lights, except in full screen (`fullScreen` from the
window's notifications). The Dart half is
`lib/src/shell/window_chrome.dart`; where the native half is missing (a
widget test, another platform) every call is a no-op and no room is left.

## The Scene view's overlay (item 9b)

The picture takes the Scene panel's whole height; everything else floats
on it in frosted strips (a 12 px blur behind the `overlay` token, an
`overlayEdge` border, radius 8):

- top left: the tools, their space (world / local) and snapping; top
  right: the camera and what the picture shows (projection, frame, align,
  grid, overlays, stats), then what is hidden or locked and Reload. The
  toolbar is a `Wrap`, not a scroll view: the space between the groups
  lets clicks, drags and scrolls through to the picture, and a narrow
  panel wraps the right side under the left instead of hiding any of it.
  A toggle that is on is the accent at 22 %, not a solid block.
- under the toolbar: the stats overlay (left) and the readout chip.
- bottom: the last notice (`scene.notice`), the camera-and-tool readout
  (`scene.status`, the navigation keys in its tooltip) and the hidden
  count (`scene.hiddenChip`).

The viewport is a `Texture`, so Flutter draws over it like over any
widget.

## Reading the Inspector (item 9b)

One label column; the value starts right after it, left-aligned (the
spreadsheet `alignRight` is gone), the unit beside the number
(`number.unit`: `1.8 m`, `45°`), the axis letter inside each part of a
vector; a value is never clipped. Groups are small uppercase captions. A
layer mask shows its names (`Everything`, `Nothing`, or the list), an
empty value `—`. The Project list has fixed columns — name, kind,
contents, size (`project.columns`, `project.column.name` / `.kind`,
`project.contents.<id>`, `project.size.<id>`) — and a selected row's
secondary text stays legible.

## One Inspector (item 9c)

The Inspector shows **whatever was selected last**, the way Unity's does.
`EditorSelection` (`lib/src/shell/editor_selection.dart`) holds one
target:

| Target | Written by | The page |
| --- | --- | --- |
| `InspectEntities` | the Hierarchy, the Scene view | the entity page (the components, as since item 4) |
| `InspectAsset(id)` | the Project panel, *Show in Inspector* | `AssetPage` |
| `InspectAsset(id, sub: 'mesh/Cube')` | a part row on an asset page | `SubAssetPage` |
| `InspectFolder(rel)` | a folder in the Project panel | `FolderPage` |

Selecting in one place moves the Inspector there, and so does coming back
to a panel: a click in the Hierarchy shows its selection again even when
it did not change. The Hierarchy keeps its own multi-selection for entity
work. **Pickers never write the selection** — choosing a texture for a
material's slot is not selecting the texture, and it would take the
Inspector away from the field being edited.

**The pages** (`lib/src/inspector/asset_pages.dart`) are built from one set
of pieces (`inspector_chrome.dart`), so an entity, an asset, a part and a
folder read alike: the header (picture, name, what it is, the id with
Copy, the padlock), the facts row (`facts.<label>`), uppercase section
captions, label/value rows on the fields' label column, and the **apply
bar** (`inspector.applyBar`) pinned to the bottom while something is not
applied yet.

- **An asset**: Open (a scene or a prefab), Reveal in Finder, Reimport (a
  source), Delete; **File**; a source's **Import settings** — the
  importer's known settings as controls plus the raw JSON; a change brings
  up the apply bar with **Revert** (`asset.revertSettings`) and **Apply and
  Reimport** (`asset.applySettings`: the sidecar through the session, one
  asset-history step, then the reimport, its summary or refusal shown);
  a model's **Materials** — one row per embedded material
  (`asset.materials.<key>`), a material field whose None is the embedded
  one and whose picker lists the project's material documents, editing
  the draft like any setting, and **Extract Materials**
  (`asset.extractMaterials`: the embedded materials not remapped yet
  written as documents into `Materials/` next to the model and remapped,
  one asset-history step, then the reimport; off while the settings have
  unapplied changes) — `documents.md` § Material remaps;
  **Parts** (click one to inspect it, drag one onto a field); the document
  kind's own sections; **Used by**, each row opening the referencing
  document at the field.
- **A part** (`asset.part.title`): what it is and which file it comes
  from, a link back (`asset.part.back`), its uuid, a drag handle
  (`asset.part.drag`), **Open** on an imported model's `prefab` part
  (`asset.part.open`, the read-only tab of § Prefab mode › Imported
  hierarchies) and the uses of that part.
- **A folder** (`folder.title`): what is inside it by kind
  (`folder.kind.<kind>`), its subfolders (`folder.sub.<name>`), Open and
  Reveal.

**A document kind's sections are registered**, not written into the page:
`AssetDocumentSections.register(kind, builder)` gives every document of
that kind its own sections on the asset page, in place of the generic
summary (a scene's or prefab's entity count, or the document's top-level
fields). A custom-shader material's summary is registered this way today; a later
data-asset kind (`@DataAsset`, the packages stream) registers one builder
— no panel, no change to the page.

**The material Inspector** (Unity parity, Tier 2, 2026-09-24): a
`fynkel.pbr@1` material's page edits it where it is shown, as Unity's
material Inspector does (`inspector/material_inspector.dart`, keys
`material.<field>`). **Surface Options**: Rendering Mode (Opaque, Cutout,
Transparent — `alpha_mode` opaque / mask / blend), Alpha Cutoff under
Cutout, Double-Sided. **Surface Inputs**: Base Map and Base Color (RGBA),
Metallic Map, Metallic, Roughness, Normal Map and its scale, Occlusion Map
and its strength. **Emission**: Emission Map, Emission Color and
Intensity (`emissive_strength` — the HDR colour as a colour and an
intensity). The maps take the Project's textures (the picker, a drop from
the Project browser, × for None). Each change is one transaction on the
material's own document, **saved at once** — there is no Save for a
material — through `AssetEdits` (`inspector/asset_edits.dart`,
one per project, which opens the document without a tab and shares it
with one when the material is also open in a tab). While a colour is
picked, a label dragged or a slider moved, the Scene view draws the
material as it would be and nothing is written (`previewMaterial` →
the preview's `setMaterial`); every change, undo and redo hands it the
document's fields, so the picture never waits for the recook the save
starts. **⌘Z / ⇧⌘Z undo and redo the material** while the Inspector or
the Project holds the keyboard and the Inspector shows it (the menu reads
"Undo Material Change"); anywhere else they undo the focused tab. Every
Inspector slider is live now, a component's included. Not yet: a preview
sphere at the bottom of the page, and a custom-shader material, whose page
stays read-only.

**The Volume Profile Inspector** (Unity parity, Tier 2, 2026-09-24): a
`.volume.json`'s page edits it as Unity's Volume Profile Inspector does
(`inspector/volume_inspector.dart`, keys `volume.<override>.<field>`).
One card per override the profile holds — **Ambient Occlusion**
(Intensity, Radius), **Bloom** (Intensity, Threshold, Scatter, Tint) —
with **All** / **None** and **Remove** in its menu; **Add Override**
lists the ones it lacks and adds an empty block. A **checkbox in front of
every field** says whether the profile sets it: ticked, the field is in
the file and editable; not ticked, it is absent — inherited from the
volumes below and the engine — and shows the engine default greyed.
Ticking writes the value shown. The rest is the material's: one
transaction per change on the profile's own document through
`AssetEdits`, saved at once, blended live in the Scene view while a label
is dragged or the tint picked (`previewVolume` → the preview's
`setVolume`), and ⌘Z / ⇧⌘Z in the Inspector read "Undo Volume Profile
Change".

**The Physics Material Inspector** (Unity parity, Tier 3, 2026-09-25,
ADR-0025): a `.physicsmaterial.json`'s page
(`inspector/physics_material_inspector.dart`, keys
`physicsMaterial.<field>`) edits **Friction**, **Bounciness**, **Friction
Combine** and **Bounce Combine** (Average, Minimum, Multiply, Maximum —
the higher rule of the two colliders wins), through `AssetEdits` like a
material: one saved step per change, ⌘Z reading "Undo Physics Material
Change". Nothing draws it in the Scene view; Play reads the cooked
values. New Physics Material is in the + menu, the folder menus and
Assets (`assets.newPhysicsMaterial`, no shortcut, as in Unity). A
Collider's **Material** field picks one (`asset(physics_material)`); while
it is set the collider's own Friction and Bounciness are hidden, since
the material's replace them (`showIf: '!material'` — `showIf` counts a
set reference as on).

**The Data page** (ADR-0027 §7, Unity's ScriptableObject Inspector,
2026-09-25, `7dec1d9`): a `.data.json`'s page is the registered
`AssetDocumentSections` builder for `data`
(`inspector/data_asset_inspector.dart`), which edits through the page's
`AssetEdits` (`AssetPageScope`). It draws the type's fields from its
committed schema (`schemas/data/<id>.schema.json`, with the metadata's
labels) with the same field editors a script's card uses: the field
editor (`inspector/field_editor.dart`) works on a `FieldTarget` — a
component slot on the selected objects, or a data asset's `data` — so
it needs no entity. Each change is prechecked like a script field's
(bounds, an existing asset), writes the whole top-level field at
`/data/<field>`, and is one saved undo step reading "Undo Data Asset
Change". A type with no schema shows its values read-only. Create ▸
<menu path> (`Game › Weapon`, from the type's `menu`; its `icon`) is in
the + menu and the folder and asset menus; it writes `New Weapon.data.json`
of that type with every field at its default and starts the rename. A
data-typed field (`x-fynkel { type: data, data: <type id> }`, alone or a
list's items) is an asset field of kind `data` whose ⊙ picker and drop
take only data assets of its type (the asset list's `dataType`). During
Play a saved edit reaches the running game's shared instance at once
(`LivePlay.dataEdited` → `World.applyDataEdit`), and a script's
data-typed field set in the Inspector takes the shared instance
(`World.resolveData`); runtime changes to an instance are never saved.
A record (`x-fynkel { type: record }`, an object schema with
`properties`), on a data asset or a script card, alone or as a list's
items, is a foldout of its fields — open until folded (`_Foldout`, per
field) — and + on a list of records adds one with the record's defaults.

**Open** is offered for scenes, prefabs and an imported model's `prefab`
part. Blend spaces have no editor in M4, so opening one
would give an empty tab; their pages show their content read-only until
an editor for them exists.

**The lock** (`inspector.lock`, the padlock in the header) keeps the
Inspector on its target while other things are selected; locking the
entity page pins those entities in the model as well. An explicit
request — *Show in Inspector* — goes to another Inspector instead, a new
one when every copy is locked (item 9e; until then it moved the locked
Inspector). Unlocking goes to what was selected meanwhile.

The Asset panel's panel id `import` is **retired**
(`PanelRegistry.retired`): a saved layout that still holds it loads with
it dropped and is saved again without it, rather than falling back to the
default. Its Window menu item went with it — 90 commands (94 since item
9e's four New <Panel>).

## One picker (item 9d)

Every place that asks "which one?" opens the same popover,
`showPicker<T>` (`lib/src/picker/picker.dart`): a search field, the
results in groups, **None** first where the field may be empty, a preview
pane on the right (a thumbnail and a few facts, the description when
there is one), and the same keys everywhere — ↑ ↓ move, ⏎ chooses, esc
closes. A row that cannot be chosen is shown disabled with its note (a
component the entity already has, a component without defaults). It sits
under the field that asked, its right edge on the field's (above it when
there is no room below), or at the top of the window for the palette and
Open Document, and opens without an animation.

| Who asks | Prefix | Groups |
| --- | --- | --- |
| an asset slot | `assetPicker` | by folder; a part by its source file |
| an entity slot | `entityPicker` | — |
| Open Document (⇧⌘O, the tab bar's **+**) | `openDocument` | Scenes, Prefabs |
| Add Component | `addComponent` | by category |
| Create Entity with Component (the Hierarchy's) | `createWith` | by category |
| the command palette (⌘K) | `palette` | by menu |

The keys are the prefix's: `<prefix>` (the popover), `.search`, `.title`,
`.<id>` for a row (`.row.<i>` for one without an id), `.note.<id>`,
`.none`, `.preview`, `.count`, `.empty`.

## Panel tabs, copies and the orientation gizmo (item 9e)

**The tabs** (`_PanelTab` in `lib/src/shell/workspace.dart`). A panel's
tab is its icon and its title — nothing else until the pointer is over
it, when a close × fades in (the `motion` token; hover is the one thing
9a lets move). A locked copy shows its padlock after the title. The title
is cut at 180 px, and in a group too narrow for it the row clips instead
of overflowing. The maximise button is gone: a **right click** on a tab
opens its menu — **Maximise** / **Restore**, **Close**, and for a panel
that can be copied **New <Panel>** — and a **double click** maximises or
restores. The menu opens without an animation, like every menu.

**Copies.** The Hierarchy, the Scene view, the Inspector and the Project
browser can be open more than once; the Console, the Profiler and the
Animation panel stay single. A copy's id is `<panel>#<n>` with the lowest
free number (`inspector#2`), its title `Inspector 2`. **New <Panel>** —
the tab menu's, or Window ▸ New Hierarchy / New Scene View / New
Inspector / New Project Browser (`window.newCopy.<id>`, ⇧⌥⌘1 … ⇧⌥⌘4:
the panel's own Show / Hide with ⇧) —
opens it to the right of the copy used last, as wide as that one (the
docking package would otherwise give the newcomer what is left). Each
copy has its own model:

| Panel | A copy's own | Its padlock |
| --- | --- | --- |
| Inspector | target (an `EditorSelection` each) | keeps the target while other things are selected |
| Project browser | folder, selection, search, list / grid | keeps the folder: a reveal goes elsewhere |
| Hierarchy | — | holds its document; unlocked it follows the focused tab |
| Scene view | camera, per document | — |

**Which copy answers.** Commands and the menus address the copy that had
the keyboard last (`Workspace.activeCopy`, most recent first; a copy that
is opened or focused by the editor itself counts). An **explicit
request** — Show in Inspector, Show in Project, a Console or Game row
that opens an entity — goes to the copy used last **that is not
locked**, and when every copy is locked a new one opens: a lock is never
broken from outside its panel. (This replaces 9c's rule that Show in
Inspector moved a locked Inspector.) A locked Hierarchy whose document
tab is focused brings its own tab forward.

**What comes back after a restart.** The layout (`layout.json`) keeps the
copies, where they are and which tab of a group was in front. What each
copy knew lives in the project's `.fynkel/editor.json` under `panels`,
by copy id: an Inspector locked on an asset, a part or a folder comes
back locked on it (when it still exists); a Project browser comes back in
its folder and view, with its lock. A lock on **entities** and a
Hierarchy's lock on a **document** do not come back — the editor opens a
project with no documents open, so there is nothing for them to hold.
Closing a copy forgets its entry.

**The Scene view's copies need no engine change.** They share the
document's one edit-preview world; each view sets its own camera on the
world right before it renders (`cameraApplied`), which is enough because
the extract is synchronous and each viewport has its own renderer. The
cameras of copies 2 and up are saved per document under `views` in the
same scene entry (a copy that has not looked at a document yet starts
from the first copy's camera); closing a copy forgets them.
`scene_session_test.dart` draws one world from two cameras through the
real Metal viewport.

**The orientation gizmo** (`lib/src/scene_view/orientation_gizmo.dart`,
key `scene.orient`). In the Scene view's top right, under the toolbar: an
84 px frosted circle with the world's three axes as the camera sees them
— the positive ends as filled circles labelled X Y Z in the axis colours
(red, green, blue, the move gizmo's too), the negative ends as rings
labelled only under the pointer, the far ends drawn first and dimmer.

- **Click an end**: the camera looks at the pivot from that side (Right
  from +X, Top from +Y, Front from +Z). Click the end it already looks
  from: the other side. Top and Bottom keep +Z at the bottom (looking
  down) or the top (looking up) of the picture.
- **Click the centre**: perspective / orthographic — unless an end covers
  it. Looking straight along an axis, that axis's end sits on the centre,
  and the click is the end's (the other side): what you see is what you
  click.
- **The caption** under the circle (`scene.orient.projection`) names the
  view while the camera looks within 0.75° of an axis — `Front · Persp`,
  `Top · Ortho` — and just the projection in between. A click on it
  always switches the projection (like **5**).
- **Drag** anywhere on the circle: orbit, exactly as ⌥-drag does.

The gizmo keeps its place while the caption changes under it.

**A known glitch, not ours.** When the docking package rebuilds a narrow
tab group whose tabs are hidden, `tabbed_view`'s tab-strip corner
overflows for one frame and a debug build reports it. It is inside the
package, cannot be seen in a release build and has no effect;
`panels_test.dart` lets exactly that report through
(`ignoreTabStripCornerOverflow`, matched on the package's paint frame)
around the one step that causes it.

## The Profiler (roadmap Phase 2)

`lib/src/profiler/`. The numbers are the ones the engine already
measures. `FynkelGameView` closes a stats window about once a second
(frame work, simulate, the Dart behaviours and the engine step, the
render call and its extract / prepare / encode, Flutter's build and
raster, GPU pass timings where the device has timestamps, draws,
instances, culling, memory). Under Play each window reaches the editor as
the channel's `stats` record, in the Game view and in Build and Run
alike, and `PlaySession.lastStats` holds it. The `ProfilerModel` keeps
every window in a ring (600, ten minutes), parsed once from the record
and its `summary` line (a timer is `p50/p95/p99` ms). Nothing is
sampled by the editor.

The panel follows Unity's:

- **Toolbar.** Record (off drops the windows that arrive), Clear, Clear
  on Play (on by default), and the chart: CPU, GPU, Rendering or Memory.
  The timing charts add the percentile their lines show (p95 by
  default). The status reads Playing / Stopped and how many seconds are
  kept.
- **Chart.** One line per series that any kept window has. The oldest
  window is at the left, and a short run grows from the left over at
  least 60 slots. The CPU and GPU charts carry dashed 60 and 120 fps
  lines. A click or drag selects a window; ← → step, and End or ⎋
  follows the newest again.
- **Legend.** Each series' colour and its value in the shown window.
- **Breakdown.** Every number of the shown window in sections (Frame,
  CPU, GPU, Rendering, Memory), timers as p50 / p95 / p99.

The Console's Game tab still hides the stats rows by default; its toggle
shows them as text. What the Profiler does not do yet: per-system times
(the window carries only the last tick's, and the channel record omits
them), a hierarchy of markers, or a capture saved to disk.

## The keyboard, a screen reader, and the words (item 9)

**The keyboard alone.** Window → Go to *Panel* uses Unity's keys: ⌘1
Scene, ⌘2 Game, ⌘3 Inspector, ⌘4 Hierarchy, ⌘5 Project, ⌘7 Profiler,
⇧⌘C Console. It shows the panel and puts the keyboard on the node the
panel names with `PanelFocus.primary`: the Hierarchy's, Project's and
Console's lists, the Scene and Game views, the Profiler's chart. A panel
with none, or whose list is empty, gets its first control. Every panel is
a `FocusTraversalGroup`, so Tab walks one panel's controls before the
next. Arrows, Enter and ⎋ work inside the lists. ⌘K reaches every other
command by name. The Scene tools gave their ⌘ digits to the panels and
keep Q W E R Y, as in Unity.

**A screen reader.** Most controls are named by their tooltip. The
Hierarchy's rows say whether they are selected and expanded, and the
expander is a named button. Each component's ⋮ menu is named after the
component. Clicks on empty space (the Hierarchy under its rows) and a
tab's right-click are left out of the semantics tree. The text fields are
named by the row label beside them, which is not yet linked to the field.
The splitters between panels belong to the docking package: a screen
reader can resize with them, but they have no name.

`test/accessibility_test.dart` holds both. It finds no nameless control
with a scene open and an entity selected. It walks every panel and the
palette with the keyboard alone, which is the item 9 gate.

**The words** are in one table, `lib/src/l10n/strings.dart`
(`EditorStrings`): every command's label and description by its id, the
templates the generated commands fill in (tools, Show/Hide, New, Go to,
Open Recent), the menus and the panel titles. `EditorCommand` takes its
words from there unless it passes its own, and `commands_test.dart`
checks that every command has an entry and that no entry has outlived its
command. A translation is another table with the same keys. The words
inside the panels (buttons, empty states, messages) are not there yet.
They move in as their panels are touched.

## What exists after each item

| Item | Done | What |
| --- | --- | --- |
| 1 | 2026-09-21 | `crates/fynkel_authoring`, the `authoring` feature and `fynkel_authoring_*` ABI in `fynkel_native`, `packages/fynkel_authoring`; D5 and D7 closed; the game runtime carries no editor code (CI check). |
| 2 | 2026-09-21 | `apps/editor`: launcher, docking workspace saved per user, native menu bar + command registry + palette, Console with open-in-IDE, read-only banner, migration preview, preferences (§ The shell). |
| 3 | 2026-09-21 | Document tabs with save / save-as / revert / close prompts, per-tab undo, the conflict dialog with a diff; the Hierarchy over the session (patched from `affectedIds`, one transaction per operation, keep-world-transform drops), Console → tab (§ Document tabs and the Hierarchy). |
| 4 | 2026-09-21 | The Inspector: one parser over schema + metadata + the editor's native-hint table (ADR-0018 accepted), one widget per field kind, multi-object editing with mixed values and one transaction per gesture, precheck + session validation with the message kept, Add / Remove Component, prefab override revert, Open script, the codegen banner over `codegen --check --json` (§ The Inspector). |
| 5 | 2026-09-21 | The Project browser: folders and a searchable flat list with kind filters and sort, texture previews and material swatches, the Asset panel (sidecar settings, Apply and Reimport, sub-assets, dependents), New / rename / move / delete-with-refusal / Reveal / Copy id as session operations with the asset history's own undo (⌥⌘Z), prefab drop onto the Hierarchy, incremental rescan (§ The Project browser). |
| 6 | 2026-09-22 | The Scene view: `FynkelViewport` per scene tab over the session's edit-preview world (incremental sync, fallback reload, Reload and Rebuild cache), the editor camera saved per scene in `.fynkel/editor.json`, grid / axes / light-camera-collider icons / selection outline as renderer debug lines, click (stacked-hit cycling) and marquee picking through the world, move / rotate / scale gizmos in world or local axes with snapping and one transaction per drag (preview in the world, commit on release, multi-selection about the lead), editor-only hidden and locked flags, prefab drop at the hit point, stats overlay, Save Frame as a PNG identical to `fynkel render` (§ The Scene view). |
| 7 | 2026-09-22 | Prefab mode: instances expand in the Hierarchy as derived rows (inherited / bold overridden / added), every edit on a derived entity an override on the anchor (the Inspector's fields and the Scene view's gizmo alike), per-field Revert and Apply in the Inspector, Apply to prefab (two transactions in two documents), Revert all, Unpack (`unpackPrefab`, one transaction), Open prefab in its own tab, imported hierarchies (`<glb>#prefab`, `<rig>#prefab`) opened read-only with the derived ids, missing / broken prefabs shown with their diagnostics and never rewritten, Select prefab asset (§ Prefab mode). |
| 8 | 2026-09-22 | Play / Stop over `fynkel run` with `FYNKEL_EDITOR_CHANNEL=stdio`: `package:fynkel`'s one-directional stdout channel (log / diagnostic / stats / state records), the Console's Game tab with the same filters, search and collapsing as the Project tab and a row that opens the document with its entity selected, the process-tree Stop (SIGTERM deepest-first, grace, SIGKILL), the save prompt, the refusal to play a project with errors, and the "changes apply on the next Play" banner whose **Play again** stops the run before it starts the next one; Play names the desktop (`-d macos`) so `flutter run` never picks a phone or refuses to choose, and a run that dies before the game starts shows the output that says why (§ Play and the game Console). |
| 9a | 2026-09-22 | The look: the design tokens and an explicit theme in `lib/src/theme/`, the Density preference, the docking chrome from the tokens, Lucide icons, the keyboard focus ring, notices in the status bar instead of snackbars, nothing animated but hover (§ The look). |
| 9b | done 2026-09-23 (gate driven by the user) | One title bar (the project menu with recent projects, the document tabs, Play centred, the codegen state, the command search) that is the macOS title bar; the Scene view's toolbar floating over the picture; the Inspector read left to right with units; the accent following focus; the Project list in columns (§ The title bar, § The Scene view's overlay, § Reading the Inspector). |
| 9c | done 2026-09-23 (gate driven by the user) | One Inspector for whatever was selected last — entities, an asset, a part, a folder — with the Asset panel's content as its pages, import settings applied or reverted from a pinned bar, the lock, and document kinds' sections registered by kind (§ One Inspector). |
| 9d | done 2026-09-23 (gate driven by the user) | One picker for asset and entity slots, Open Document, Add Component, Create Entity with Component and the palette (§ One picker). |
| 9e | done 2026-09-23 (gate driven by the user) | Panel tabs with the × on hover and a right-click menu, trackpad scroll and pinch in the Scene view, the orientation gizmo, and copies of the Hierarchy, Scene view, Inspector and Project browser with their own state and padlocks (§ Panel tabs, copies and the orientation gizmo). |
| 9f | done 2026-09-23 (gate driven by the user) | Gizmo rendering: the grid, outlines and icons drawn by the renderer's overlay — anti-aliased lines with a width, faded behind the scene, the analytic ground grid (§ What is drawn over the world; `docs/renderer.md` § The overlay). |
| 9g | done 2026-09-23 (gate driven by the user) | Handles and Scene tools: the handle framework, the tool registry with move, rotate, scale and the combined transform tool drawn through the overlay, and component handles — light range and angles, collider sizes, camera field of view — behind the Inspector's Edit toggle (§ The gizmo). |
| 9h | done 2026-09-23 (gate driven by the user) | Gizmos for game code: the runtime `Gizmos` API in the game's window (`FynkelGameView(gizmos:)`, Play's *Show Gizmos in Play*), and declarative `@Gizmo` fields drawn and dragged in the Scene view (§ The gizmo). |
| Scripts | 2026-09-24 | The Project browser's Scripts root over `lib/` (open in the IDE) and New ▾ → Behaviour: template, `fynkel codegen --project-only`, `registerBehaviour` (§ The Project browser › Scripts and Create → Behaviour; roadmap Phase 2 item 1). |
| Profiler | 2026-09-24 | The Profiler panel over Play's stats windows: CPU / GPU / Rendering / Memory charts, Record, Clear on Play, a selected window's breakdown (§ The Profiler; roadmap Phase 2). |
| 9 | — | IDE navigation, `fynkel editor`, polish. |
| 10 | — | M4 gate and report. |

## Verification

```sh
cargo test -p fynkel_authoring                       # the session over a copy of the fixture project
cargo test -p fynkel_native --test authoring         # the ABI: statuses, results, poison, read-only
cd packages/fynkel_authoring && dart pub get && dart analyze --fatal-infos && dart test   # Flutter's dart
cargo build -p fynkel_native -p fynkel_platform_apple --no-default-features
nm -gU target/debug/libfynkel_platform_apple.dylib | grep -c fynkel_authoring_   # 0
cd apps/editor && flutter analyze --fatal-infos && flutter test   # 293 tests (`-j 4` when another stream is compiling: the real-session files each cook a copy of the fixture and time out at 30 s when starved); look_test.dart checks the focus ring, the notices and that nothing opens animated; title_bar_test.dart the bar, its menu, the recent projects, the codegen state and the macOS channel (mocked); editor_selection_test.dart and project_browser_test.dart the one Inspector, its pages, the lock and the registered sections; picker_test.dart the picker and each user's own test its keys; panels_test.dart (9e) the panel copies, their tabs, locks and routing, and a restart that brings them back; documents_session_test.dart drives tabs + Hierarchy on the real session (a 10 000-entity scene, the conflict path through the watcher); inspector_session_test.dart sets every field of the fixture's scenes and the reference game's playground to itself and saves byte-identically; project_browser_session_test.dart renames, moves, deletes (refused, forced, undone), creates and re-imports on a copy of the fixture with `validateProject` clean after every step; scene_session_test.dart picks every entity of every fixture scene through the real edit-preview world, drags the gizmo (one undo step, the incremental world equal to a fresh load), hides without touching the document, drags the entity inside Crate A as one setPrefabOverride on the anchor (the prefab file's bytes unchanged, the sync incremental, one undo), looks through and writes back the scene camera, and renders through the real viewport (Metal, offscreen; the frame's PNG equals the first capture byte for byte); prefab_session_test.dart (M4-07) reverts and re-adds Crate A's override, instances the coin, applies an override to coin.prefab.json and undoes per document, unpacks and reloads, overrides and unpacks the Imported rig, opens a broken reference without rewriting the file, and opens rig.glb#prefab read-only — every override list checked against the raw document and `inspect`'s count; play_test.dart (M4-08) parses the channel's records behind a `flutter run` prefix, builds and signals a process tree from a canned `ps` table, drives a whole run over a fake process (the environment it was given, `starting → running → exited`, a crash's code, the dropped-row count, the CLI that is not there, a run that dies before the game starts — the device `flutter run` could not pick — with the Console showing the output that says why) and the shell (a project with errors refused onto the Project tab, the Game tab's rows, a diagnostic row that opens the scene with the entity selected, Stop, the "changes apply on the next Play" banner and its Play again — the old run stopped, the dirty document saved, a second process started and the banner gone —, the save prompt's three answers)
cd apps/editor && flutter test test/play_process_test.dart   # M4-08 against a *real* process: a shell script that announces itself on the channel and starts a grandchild — Stop takes the grandchild with it, a `kill -9` from outside ends in the exit code with the records it wrote still in the Console, and both leave every document in the project byte-identical
cd packages/fynkel && dart test test/editor_channel_test.dart  # the channel itself: `started`/`loaded` with a monotonic `seq`, a failed load as FYN- records then `loadFailed`, a missed deadline as `info`, `Diagnostics.log` as a log record with PrintDiagnostics quiet — and with the variable unset, not a byte written
FYNKEL_SCENE_FRAME_OUT=/tmp/main-320x200.png flutter test test/scene_session_test.dart --name viewport   # keeps the clean 320×200 frame of the fixture's Main through its camera …
cargo run -q -p fynkel_cli --bin fynkel -- render --project <fixture copy> --scene c0000000-0000-4000-8000-000000000030 --out /tmp/cli.png --size 320x200   # … to compare with this: 0 differing pixels (2026-09-22)
cargo test -p fynkel_authoring --test preview        # the edit-preview world: load, the incremental sync == a fresh load after every transaction kind, picking, outlines, hidden, the anchor map
cd packages/fynkel_flutter && flutter test          # viewport ABI v6: debug lines, the overlay (ViewportOverlay) and read_frame on the real renderer
cargo test -p fynkel_authoring --test prefab_mode    # M4-07: the instance query live against an open prefab, apply as two transactions, unpackPrefab, imported hierarchies read-only, the broken cases left alone
cargo test -p fynkel_document --test authoring        # unpackPrefab and apply_with_prefabs (the shared touch)
cargo test -p fynkel_authoring --test ops            # the asset operations, and the 2 000-asset tree: a full scan against one change's rescan (< 50 ms)
cargo run -q -p fynkel_cli --bin fynkel -- codegen --check --project packages/fynkel_generator/example/spinner_game --json   # what the Inspector banner runs
```

The item 3 gate, driven by the user: open the editor on a **copy** of the
fixture project (`crates/fynkel_document/fixtures/project` is a committed
fixture — the test suites expect its bytes) and on the reference game;
rename, create, duplicate, reparent and delete, ⌘S, then
`cargo run -q -p fynkel_cli --bin fynkel -- scene inspect <copy>/assets/scenes/main.scene.json --json`
shows every change; with a tab dirty, edit the same file from a terminal and see the
conflict dialog — never an overwrite; open a 10 000-entity scene and
scroll and select without a rebuild; close the Playground session that is
open on the reference game before its scene is edited.

The item 5 gate, driven by the user, again on **copies**: in the Project
panel rename a texture that a material uses (the material still validates,
`fynkel validate <copy>` clean, the sidecar moved with it), drag it into
another folder, delete it — refused with the material listed — then
**Delete anyway** and ⌥⌘Z to bring it back; New Material / New Scene /
New Folder and rename each; change a texture's colour space in the Asset
panel and **Apply and Reimport**; drag a prefab (and `robot.glb#prefab`)
onto the Hierarchy and a texture onto a material field in the Inspector;
Reveal in Finder; add a file to `assets/` from Finder and see it appear
(the watcher's incremental rescan); `fynkel validate <copy>` after every
step. The reference game's Playground session must be closed first.

The item 6 gate, driven by the user, on **copies** again: open a copy of
the reference game, open its Playground in a tab and watch the Scene
view — orbit (⌥-drag), pan (middle-drag), dolly (scroll), fly
(right-drag + WASD), F on a selection, ⇧F, 5, ⌘G / ⌘I / ⌘T; click
entities (a second click on the same spot cycles to what is behind),
⌘-click and ⇧-click, drag a marquee; W and drag a crate along an axis, on
a plane, with ⌥⌘G snapping on — **one** ⌘Z brings it back and the
Inspector shows the committed numbers; E and R the same; select several
and drag; H / ⇧H / L, and `git status` on the copy shows only
`.fynkel/editor.json` changed by any of that; drag a prefab from the
Project panel onto the floor; select the scene's camera, ⌥⌘L, ⌘G ⌘I Esc,
then **Save Frame…** (⌥⌘X) and compare with
`cargo run -q -p fynkel_cli --bin fynkel -- render --project <copy> --scene <playground id> --out /tmp/r.png --size <the saved size>`
(`cmp` says identical); leave the tab open for 60 s and see every rigid
body where the document put it (the stats overlay's tick does not move);
edit the playground's scene file from a terminal and see the picture
follow the reload. The Playground session must be closed first.

The item 7 gate, driven by the user, on a **copy** of the fixture project
(`crates/fynkel_document/fixtures/project`): open Main, press → on
**Crate A** and see the instance open with its `Crate` row bold (the
fixture's override: MeshRenderer `cast_shadows` = false); select the
row, see the bold field, **Revert**
it, ⌘Z, then **Apply to prefab** — the crate prefab's tab opens dirty, the
scene's override is gone, ⌘Z in the scene brings it back, ⌘Z in the
prefab tab undoes the prefab; ⌘S both and
`cargo run -q -p fynkel_cli --bin fynkel -- scene inspect <copy>/assets/scenes/main.scene.json --json`
shows the override count, `prefab inspect <copy>/assets/prefabs/crate.prefab.json --json`
the new value. Drag `coin.prefab.json` into the scene, open it, change
the trigger's `event`, Apply, Unpack (⇧⌥⌘X), ⌘S — `scene inspect` shows
one instance fewer and the coin's entities as plain ones; ⌘Z puts the
instance back. Open Rig, open **Imported**: its `node/…` rows, an
override on one, **Open prefab** (the tab `rig.glb › prefab`, read-only
banner, no gizmo), Apply disabled with its reason, Unpack. Edit a
scene's `prefab.asset` to a wrong id from a terminal, open it: the
badge, the broken icon, the diagnostic in the Console, Revert still
offered, ⌘S writes the same bytes (a diff of the copy shows nothing
for that entity). Never the committed fixture.

The item 8 gate, driven by the user, on a **copy** of the reference game:
press ▶ (⌘P) — the toolbar says *Starting…*, then *Playing · Playground*,
the game's window opens, and its diagnostics and `Diagnostics.log` lines
appear in the Console's Game tab within a frame of the game writing them
(the reference game logs on a pickup; the Console's tick column moves).
Double-click a diagnostic row that names an entity: its scene opens with
that entity selected. Press ■ (⌘.) and watch the window go — then
`pgrep -f <the copy's name>` finds nothing of the run left. Play again
and `kill -9` the game from a terminal: the toolbar says *Exited with
code -9*, the rows it had already written are still there, and
`diff -r --exclude=fynkel_cache --exclude=.fynkel <the game>/assets
<the copy>/assets` is silent — the editor wrote no document for the game,
neither on the way in nor on the way out. Edit a scene while the game
runs: the banner appears and the running game keeps the older files;
Play again and the change is in. Finally break the project (a misspelt
field from a terminal) and press ▶: it refuses, the Console comes up on
the Project tab with the count, and a double-click goes to the line. Then
break the *code* instead (a typo in a behaviour of the copy, which the
editor cannot see) and press ▶: the build fails, the game never starts,
the toolbar says *Did not start (code 1)* and the Console shows the
compiler's own lines without being asked. Never the committed reference
game.


The items 9a–9d gate, driven by the user, on a **copy** of the reference
game (`cp -R examples/reference_game /tmp/refgame-gate`, then point the
copy's three `path: ../../packages/…` dependencies in `pubspec.yaml` at
the repo's `packages/` with absolute paths and run `flutter pub get` in
it — outside the repo the relative paths resolve to nothing and the
codegen check fails — then open the copy, never the committed game):

- **9a** — Preferences (⌘,): Dark and Light, each switching at once;
  Compact, Default and Comfortable, every row, field and toolbar following.
  Click in the Hierarchy, then Tab and the arrows through every panel and
  a dialog: a thin blue ring always shows where focus is, a click hides
  it, W in the Scene view does not bring it back. File → Open Project… on
  a folder that is not a project: the message sits in the status bar with
  an × until dismissed. Menus, the palette, Preferences and About open at
  once.
- **9b** — the top bar is one bar with the traffic lights in it: drag the
  window by its empty space (not by a tab or a button), double-click that
  space (the window zooms, or does what System Settings says), enter and
  leave full screen (the room for the traffic lights goes and comes back).
  The project menu: the recent projects (switch to one and back), Reveal,
  Close. The **+** after the tabs opens Open Document under itself. Add an
  `@Expose` field to a behaviour in the copy's `lib/` and click the codegen
  icon: it says *Run codegen*; run `fynkel codegen` in the copy, click
  again: the quiet icon. Open the Playground: the Scene toolbar floats over the
  picture, clicks and scrolls between its groups reach the picture, and a
  narrow Scene panel wraps it; the Inspector's numbers sit next to their
  labels with units; only the focused panel's selection is blue; the
  Project list's columns.
- **9c** — select an entity, then a texture, a model, one of the model's
  parts (and back to the model with the link), a material, a folder: the
  Inspector follows each. Change a texture's colour space: the bar comes
  up; Revert; change it again and Apply and Reimport; ⌥⌘Z. Lock the
  Inspector on an entity and select other things: it stays; unlock. A
  saved layout that had the Asset panel opens without it.
- **9d** — keyboard only, each of the six: an asset slot (Tab to its
  button, ⏎), an entity slot, ⇧⌘O, Add Component, Create Entity with
  Component, ⇧⌘P — type to filter, ↑ ↓, ⏎ chooses, esc closes; **None**
  where a slot may be empty; the preview on the right.

Then item 9e, on the same copy:

- **Tabs** — no × and no maximise button on any panel tab; the × fades in
  under the pointer and closes. Right-click a tab: Maximise, then
  Restore; double-click does the same. Close from the menu, then bring
  the panel back from Window.
- **Zoom** — in the Scene view, two fingers up and down on the trackpad
  and a pinch both move the camera in and out; the mouse wheel too.
- **The gizmo** — click X, Y, Z and the three dots: the caption says
  Right, Top, Front, Left, Bottom, Back; click the same end twice: the
  other side. The centre (between views) and the caption switch Persp
  and Ortho. Drag the circle: the camera orbits.
- **Copies** — Window ▸ New Inspector, lock it on a texture, select
  entities: the first Inspector follows, the locked one does not. Show in
  Inspector from the Project browser with both locked: a third opens.
  New Project Browser: two folders side by side; lock one and use Show in
  Project on an asset elsewhere — the unlocked one moves. New Scene View:
  look from Top in one and from Front in the other; move an entity in
  one and watch it in both. New Hierarchy, lock it on a prefab tab, then
  switch to the scene tab: the locked one stays on the prefab. Quit and
  reopen the copy: the copies, the Inspector's lock on the texture, the
  browsers' folders and the two cameras come back.
