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
  with the shell (item 2).

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
changed paths and revalidates on every change. The lock check is the session's: a lock that
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
(`fynkel migrate <dir>`, with a Copy button). It comes from the session's
`migrations` query and **writes nothing** — the editor never migrates on
its own; `fynkel migrate` keeps `<file>.bak`.

**The main window** (`lib/src/shell/main_window.dart`): a toolbar (project
name, read-only chip, Close), the banners, the `Docking` workspace and a
status bar (error and warning counts, document/source/reference counts,
"validated HH:MM:SS"; a click shows the Console). The workspace
(`lib/src/shell/workspace.dart`) is the §22.2 default —
`Hierarchy | Scene over [Project, Console, Profiler, Animation, Asset] | Inspector`,
i.e. `R(Ihierarchy,C(Iscene,T(Iproject,Iconsole,Iprofiler,Ianimation,Iimport)),Iinspector)`
— saved per user (`layout.json` next to the preferences: the docking
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
with stable dotted ids; every command has a menu, a shortcut and a palette
entry, so each is reachable three ways (`test/commands_test.dart` proves the
same command runs from all three). The menu bar is native on macOS
(`PlatformMenuBar`) and in-window elsewhere and in tests; only the menus
that have commands exist (Project, Edit, Window, Help — no stub menus). The
palette (⇧⌘P, `lib/src/commands/palette.dart`) ranks by exact label, prefix,
word start, substring, id/menu, then subsequence, over the live label (so
"hide console" finds the toggle while the Console is visible); ↑↓ and Enter
run, Escape closes.

| Id | Menu | Shortcut | Does |
| --- | --- | --- | --- |
| `project.new` | Project | ⇧⌘N | the New Project dialog (`fynkel create`) |
| `project.open` | Project | ⌘O | pick a directory and open it |
| `project.recent.<i>` | Project | — | generated from Recent; no shortcut |
| `project.close` | Project | ⇧⌘W | close the project, back to the launcher |
| `project.revalidate` | Project | ⌥⌘V | validate again now |
| `project.migrations` | Project | ⌥⌘M | the migration preview |
| `project.reveal` | Project | ⇧⌘R | reveal the project in Finder |
| `app.quit` | Project | ⌘Q | quit |
| `document.open` | Project | ⇧⌘O | the Open Document dialog (scenes and prefabs of the project, searchable) |
| `document.save` | Project | ⌘S | save the focused tab through the session (refused while it has a disk conflict — the conflict dialog opens instead) |
| `document.saveAs` | Project | ⇧⌘S | save the focused tab under a new relative path (never over an existing file) |
| `document.saveAll` | Project | ⌥⌘S | save every dirty tab |
| `document.revert` | Project | ⌥⌘R | revert the focused tab to the saved file (asks; history dropped) |
| `document.close` | Project | ⌘W | close the focused tab (a dirty one asks Save / Don't Save / Cancel) |
| `document.next` / `document.previous` | Project | ⌥⌘→ / ⌥⌘← | cycle the tabs |
| `document.undo` | Edit | ⌘Z | undo the focused tab's last transaction — the label counts what is left, "Undo (3)"; inside a text field the field's own undo runs |
| `document.redo` | Edit | ⇧⌘Z | redo (same rule for text fields) |
| `hierarchy.rename` | Edit | ⌘R | rename the selected entity in place |
| `hierarchy.duplicate` | Edit | ⌘D | duplicate the topmost selected entities with their subtrees; the copies are selected |
| `hierarchy.delete` | Edit | ⌘⌫ | delete the topmost selected entities with their subtrees; the nearest row is selected (in a text field: delete to the line start) |
| `hierarchy.createEmpty` | Edit | ⌥⌘N | a new entity with a Transform, under the single selected entity or at the root |
| `hierarchy.createWith` | Edit | ⇧⌥⌘N | pick a native component type; the new entity has a Transform plus that component with its defaults, and is renamed at once |
| `hierarchy.keepWorldTransform` | Edit | ⌥⌘K | toggle the drop policy (✓ when on) |
| `hierarchy.search` | Edit | ⌥⌘H | focus the Hierarchy's search field |
| `hierarchy.expandAll` / `hierarchy.collapseAll` | Edit | ⌥⌘] / ⌥⌘[ | expand or collapse every node |
| `inspector.addComponent` | Edit | ⇧⌘A | the searchable Add Component popup; adds to every selected entity in one transaction |
| `inspector.openScript` | Edit | ⌥⌘↩ | the selected entity's first behaviour's source in the IDE |
| `prefab.applyOverrides` / `prefab.revertOverrides` / `prefab.unpack` / `prefab.open` / `prefab.selectAsset` | Edit | ⇧⌥⌘A / ⇧⌥⌘U / ⇧⌥⌘X / ⇧⌥⌘D / ⇧⌥⌘L | prefab mode on the selected instance (§ Prefab mode): apply the overrides to the prefab (two transactions, two documents), revert them all, unpack, open the prefab's tab, select its asset |
| `inspector.checkCodegen` | Project | ⇧⌘G | run `fynkel codegen --check --project <dir> --json` in the background; the Inspector banner shows the result |
| `scene.tool.select` / `scene.tool.move` / `scene.tool.rotate` / `scene.tool.scale` | Edit | ⌘1 / ⌘2 / ⌘3 / ⌘4 | the Scene view's tool (Q W E R in the viewport) |
| `scene.toggleSpace` | Edit | ⌘L | gizmo axes world ↔ local (X in the viewport; scale is always local) |
| `scene.toggleSnap` | Edit | ⌥⌘G | snapping on/off (the steps: long-press the snap button) |
| `scene.toggleHidden` | Edit | ⇧⌘H | hide / show the selection's subtrees in the Scene view (H) — editor-only, never in the document |
| `scene.showAll` | Edit | ⇧⌘U | show every hidden entity (⇧H) |
| `scene.toggleLocked` | Edit | ⌘K | lock / unlock the selection against picking and dragging (L) |
| `scene.alignCameraToView` | Edit | ⇧⌘L | write the editor camera into the selected `fynkel.Camera` entity: one transaction (Transform + lens fields) |
| `view.frameSelected` / `view.frameAll` | View | ⌘F / ⇧⌘F | frame the selection's bounds (F) / everything visible (⇧F) |
| `view.toggleProjection` | View | ⌘5 | perspective ↔ orthographic (5) |
| `view.alignToCamera` | View | ⌥⌘L | look through the selected scene camera (its exact matrix, lens and exposure) |
| `view.reset` | View | ⇧⌘0 | the initial editor camera |
| `view.toggleGrid` / `view.toggleOverlays` / `view.toggleStats` | View | ⌘G / ⌘I / ⌘T | grid and axes / light-camera-collider icons / the stats overlay |
| `view.reload` | View | ⌃⌘R | the preview world emptied and loaded fresh from the live document |
| `view.recook` | View | ⌃⇧⌘R | cook the project again and reload every preview over the new cache |
| `view.saveFrame` | View | ⌥⌘X | one frame of the viewport as a PNG at its physical size (the pixel gate's tool) |
| `assets.undo` / `assets.redo` | Edit | ⌥⌘Z / ⇧⌥⌘Z | undo / redo the last **asset operation** (its own history; the label names it: "Undo Rename coin.prefab.json → gold.prefab.json") |
| `assets.newScene` / `assets.newPrefab` / `assets.newMaterial` | Project | ⇧⌥⌘S / ⇧⌥⌘P / ⇧⌥⌘M | a new document in the Project browser's folder (`New Scene`, `New Scene 2`, …), selected and renaming at once |
| `assets.newFolder` | Project | ⇧⌥⌘F | a new folder there (`New Folder`), renaming at once |
| `assets.open` | Project | ⇧⌥⌘O | the selected scene or prefab into a tab; any other asset into the Asset panel |
| `assets.rename` | Project | ⇧⌥⌘R | rename the selected asset in place (the sidecar moves with it; the id survives) |
| `assets.move` | Project | ⇧⌥⌘V | move the selected assets to a folder picked from a list |
| `assets.delete` | Project | ⇧⌥⌘⌫ | delete the selected assets to `.fynkel/trash`; refused while referenced, with the dependents listed and a **Delete anyway** |
| `assets.reimport` | Project | ⇧⌥⌘I | run the selected source through its importer with the sidecar's settings |
| `assets.reveal` | Project | ⇧⌥⌘E | reveal the selected asset in Finder (`open -R`; `explorer /select,` and `xdg-open` elsewhere) |
| `assets.copyId` | Project | ⇧⌥⌘C | the selected asset's id on the clipboard |
| `assets.search` | Project | ⌥⌘P | focus the Project browser's search field |
| `assets.toggleView` | Project | ⇧⌥⌘T | the Project browser as folders or as one flat list (the label flips) |
| `app.preferences` | Edit | ⌘, | Preferences |
| `window.commandPalette` | Window | ⇧⌘P | the palette |
| `window.resetLayout` | Window | ⌥⌘0 | the §22.2 default layout |
| `window.panel.<id>` | Window | ⌥⌘1 … ⌥⌘8 | show/hide Hierarchy, Scene, Inspector, Project, Console, Profiler, Animation, Asset (the panel id stays `import`; the label flips between Show and Hide) |
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
visible line) on the clipboard.

**Preferences** (`lib/src/prefs/preferences.dart`;
`~/Library/Application Support/Fynkel Editor/preferences.json` on macOS,
`$XDG_CONFIG_HOME/fynkel-editor/` elsewhere): theme (system/light/dark),
the IDE command, the CLI, checkout and Flutter paths when detection is not
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
  by id or by `/entities/<id>` pointer).
- **Operations — one transaction each** (`HierarchyModel`): rename in
  place (double-click, ⌘R, Enter; Enter or focus loss commits, Esc
  cancels, an unchanged or empty name is not a transaction), Duplicate
  (⌘D; the topmost selected with their subtrees; a child's copy lands
  right after its original, a scene root's copy at the end — a scene's
  roots follow the entity key order), Delete (⌘⌫; the nearest remaining
  row is selected), Create empty (⌥⌘N) and Create with a native component
  (⇧⌥⌘N or the + menu; the Transform's and the component's **defaults
  come from the session** — the `defaults` query is the component's Rust
  `Default` through the document codec, so what the editor writes is what
  the runtime would assume). Drag-and-drop **reparents** with an explicit
  index: the row's middle drops *into* (appended), the top quarter *above*
  it, the bottom quarter *below* it (or as the first child when the row is
  expanded); a drop onto itself or a descendant is refused; several moved
  entities keep their visible order, and an index counts the slots they
  vacate. Between two **roots of a scene** there is no insertion line
  (roots follow entity key order; only a prefab's `roots` list is
  orderable) — the top and bottom quarters of a scene root mean *into*.
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
| `color` | a swatch (linear → sRGB for display) with the hex next to it; the picker is HSV + hex + alpha for four components; values are written linear |
| `assetRef` | the asset's name, `…` opens a **searchable picker** filtered by `asset(<kind>)` (a source offers its matching sub-assets; an audio source is offered whole as a `sound` — `AudioSource.clip` takes no sub), Clear when nullable, and a `DragTarget<AssetDragPayload>` for the Project browser's rows (item 5) |
| `entity` | a name field with a menu of the document's entity names |
| `layerMask` | a check-list of the project's `physics.layers` (Everything / Nothing), the number beside it |
| array, object | rows per item / per child, add and remove; `any` and keys the schema does not list fall back to JSON text |

Numbers round-trip **byte-exactly**: a field that held `1` stays an `int`,
one that held `1.0` stays a `double` (`jsonNumber`), so setting every
field of the fixture's scenes and the reference game's playground to its
own value and saving changes no bytes (`inspector_session_test.dart`).

### The panel (`inspector_panel.dart`)

Header with the name (editable) and id (copy), or "N entities — …" for a
multi-selection (above 200 entities a count only). One collapsible card per
slot with the type's icon (the `icon` hint or a per-type default), its
description as a tooltip, the field rows in groups, and a menu: **Reset to
Defaults**, **Open Script** (a behaviour: `package:<pkg>/<path>` from the
metadata resolved to `<project>/lib/<path>` when `<pkg>` is the project's
package, then the configured IDE command), **Copy Type Name**, **Remove
Component**. A prefab instance's anchor shows a **Prefab instance** section
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
exactly one sub-asset as that sub-asset; the Asset panel drags a source's
sub-assets one by one. The payload is the Inspector's `AssetDragPayload`
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

Toolbar: view toggle, search (⌥⌘P), the kind filter menu with counts and
a badge, sort, **New ▾** (Scene, Prefab, Material, Folder), Undo/Redo of
asset operations with the operation's label in the tooltip. Below, in the
folders view, the tree (`assets` root and every folder, the shown one
highlighted) beside the list; a folder opens on one click, a document
opens in a tab on double-click (or ↩), any other asset shows the Asset
panel. Rows show the file name (renaming changes the file, never a
document's internal `name`, which dims beside it when it differs), a
preview, the folder path in the list or while filtering, `N sub` with
the sub-asset ids as a tooltip, and the kind. **Inline rename** (F2, the
menu, or right after New): ↩ commits, ⎋ cancels, losing focus commits.
Keys: ↑↓ ⇧↑↓ ↩ F2 ⌫/⌦ ⌘A ⎋ ←(up a folder). Context menus: an asset's
Open / Show in Asset Panel / Reveal in Finder / Rename / Move to… / Delete /
Reimport / Copy id / Copy path / New … Here; a folder's Show / Reveal /
New … Here / Rename Folder / Delete Empty Folder; the empty area's New …
Here. **Drag and drop**: an asset row is a `Draggable<AssetDragPayload>`
(dragging a selected row drags the whole selection, the feedback says "3
assets"); every folder row in the tree and in the list is a drop target
that moves through the session (the target lights up only when the move
would change something). The **Delete refused** dialog groups the
dependents by document path with each pointer, and offers Cancel or
**Delete anyway**; the error bar shows any other refusal with its reasons
and a ×; the status line counts `shown of total assets in <where>` and the
selection, with an up arrow inside a folder.

A finding while testing: a `GestureDetector` that catches the empty-space
click (clear the selection, focus the list) must be `excludeFromSemantics`
— with its semantics node in the tree, rows leaving the `ListView` while
a menu route pops trip the framework's `'node.built'` assertion (the kind
filter did exactly that; the Hierarchy's identical detector never hit it
because its rows were never Text-only). It has no semantic meaning anyway.

### The Asset panel (`asset_panel.dart`)

The selected asset's inspector — the blueprint's "selected asset's
Inspector", as its own panel so the entity Inspector stays where it is:
header (name, kind, id with Copy), the actions Open (scenes and prefabs),
Reveal in Finder, Reimport (sources), Delete; **File** (path, size,
modified); for a source **Import**: importer and version, the sidecar
path, the importer's known settings as controls — `fynkel.image`
`colorSpace` (sRGB for colour, linear for data; the default writes no
key), `fynkel.audio` `stream`, `fynkel.gltf`/`fynkel.fbx`
`allowExternalFiles` — plus the raw `settings` JSON for everything else;
**Apply and Reimport** writes the settings through the session (one asset
history step, undoable) and reimports at once, the result box showing the
importer's summary (meshes, images, clips, …) or its refusal with the
diagnostics, **Revert** drops the draft; **Sub-assets**, each row
draggable onto an Inspector field or the Hierarchy; a material's
shader, alpha mode, double-sided and fields; a scene's or prefab's entity
count; **Referenced by (N)** grouped by document, each row opening that
document — a scene or prefab in a tab with the entity selected
(`openReference` in `app.dart`: the pointer's `/entities/<id>` is
revealed), any other document in the browser.

### Drop targets elsewhere

The Inspector's asset fields were `DragTarget<AssetDragPayload>` since
item 4 and accept a payload of their kind (a `texture` field takes an
image, a `mesh` field a `#mesh/0`). The **Hierarchy** now takes a
`prefab` — a prefab document or an imported model's `#prefab` — onto a
row (a child of it) or the empty area (a new root): one
`instantiatePrefab` transaction with the instance named after the file,
the parent expanded and the instance selected; refused into a prefab
instance's subtree, into a read-only tab, onto the prefab document itself,
and for every other kind (`HierarchyModel.canDropAsset`). The **Scene
view** (item 6) takes the same payload onto its picture — the instance
lands where the pointer's ray hits (§ The Scene view › Prefab drop).

What item 5 does **not** do: material preview spheres and mesh
thumbnails (not in M4), a material editor panel (the session edits
material fields since item 1 — `setDocumentField` on a material handle —
but no panel shows them yet; the Asset panel lists them read-only).

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
plane, one pixel one pixel), **⌥-right-drag or scroll dolly** (never
through the pivot; `minDistance` 5 cm), **right-drag look + WASDQE fly**
(a 16 ms timer while the button is down; ⇧ four times faster; the speed
scales with the distance). Keys: **F** frames the selection (its outline
bounds; hidden entities skipped; nothing selected → everything),
**⇧F** frames all, **5** toggles perspective/orthographic (the
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

Debug lines (`FynkelViewportController.debugLines`, ten floats a segment,
drawn by the renderer's depth-tested unlit debug pass, cleared by every
render): the **grid** on y = 0 at an adaptive power-of-ten step near a
twentieth of the camera distance, 40 × 40 cells around the pivot, every
tenth line brighter, the **world axes** through the origin (X red, Y
green, Z blue) — ⌘G; the **icons** of every light (its colour: point range
circles, spot cone, directional arrows), camera (frustum to 4 m) and
collider (its shape, green) — ⌘I; the **selection outline** — each
selected entity's mesh bounds as twelve orange edges, a cross for an
entity without a mesh, an anchor's whole instance. All three come from
the world (`preview.lines(outline:, iconsAspect:)`) so they are exact
about what is there. The **gizmo** and the **marquee** are 2-D, painted
by `_OverlayPainter` in logical pixels over the viewport from
`ProjectedGizmo` (the same projection the hit test uses). The **stats
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
at its origin, a hit inside a prefab instance answering as its anchor —
minus locked and hidden entities; the nearest is selected and revealed in
the Hierarchy. A repeated click **on the same pixel (± 3 px) cycles**
through the stacked hits. ⌘-click toggles, ⇧-click adds; a click on
nothing clears (with a modifier held it keeps the selection). A drag of
more than 4 px on empty space or with the select tool is a **marquee**
(`preview.pickRect`: the entities whose projected bounds overlap the
rectangle); the same modifiers. The Hierarchy and the Inspector follow
the tab's selection as always — the Scene view owns no selection state of
its own.

### The gizmo

Tools **Q** select, **W** move, **E** rotate, **R** scale (⌘1–⌘4 as
commands); **X** toggles world/local axes (scale is always local);
**⌥⌘G** toggles snapping, a long-press on the snap button edits the
steps (move 0.5 m, angle 15°, scale 0.1 by default), ⌘ held during a
drag inverts snapping for that drag. The gizmo sits at the **lead**
entity's world origin at a constant 90 px, axes as lines with tips, three
plane squares (move) and a centre square (move: screen-plane; scale:
uniform), rings for rotate (grabbed on the stroke; the ring facing away
flips its sign so a drag always turns the way the pointer goes). A drag
(`beginDrag` → `updateDrag` → `endDrag`) computes one delta — a move
along the axis line closest to the pointer ray, or on the plane the ray
hits; a rotation from the pointer's angle about the projected pivot; a
scale factor from the pointer's distance — applies it to **every
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
stable id); the ones that write do so in exactly one transaction. Scene
hotkeys without modifiers (Q W E R X F H L 5 Esc ⌫ WASDQE) belong to the
viewport's focus node and never fire in a text field.

### What item 6 does not do

Mesh thumbnails and material spheres (M5, the world now exists for
them); the Inspector's numeric drag previewing in the world (its drag
commits once already; the preview-then-commit rule is the gizmo's here);
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

### The instance in the Hierarchy

A prefab instance's anchor row keeps its badge (item 3); → on it now
**opens the whole instance**: the expansion's entities appear under the
anchor as **derived rows** — italic, in the hint colour while they are
purely inherited, **bold with a count badge** when a component of theirs
has overridden fields, a `+` badge when an override added them. The rows
come from the session's `instance` query (`HierarchyTree.expansionOf`,
one fetch per anchor, cached until a transaction touches the anchor or
the prefab document changes — an open prefab tab's unsaved edits show at
once in every scene that instances it, through `DocumentTabs`'
prefab-change broadcast). Derived ids are the runtime's derived ids
(`node/<i>` for an imported hierarchy), so a Console diagnostic about an
expanded entity selects its row — opening the instance on the way
(`HierarchyTree.locate`).

Everything you do to a derived row is an **override on the anchor** —
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
overrides on the anchor — `position` / `rotation` / `scale` as whole
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
edit on a derived component is a `setField` override on the anchor
(`field` is a whole top-level key of the component's `data` — the crate's
override model — so editing `position.x` overrides `position`);
`unsetField`, Reset to Defaults, Add Component (`addComponent` override)
and Remove Component (the revert of the override that added it, or a
`removeComponent` override) follow the same rule, and multi-object
editing folds them per key. The prefab's own entities cannot be renamed
here either (the name field is disabled).

The anchor's **Prefab instance** section (item 4) grew into the mode's
control panel: the prefab's name and kind ("imported hierarchy,
read-only"), a broken-link icon and the diagnostics when the instance
does not expand, the override list (each op described — "Light.intensity
= 5 on Mesh", "+ Component TriggerZone on Root", "+ Object Extra under
Root" — with **Revert** and **Apply** per row), and the buttons **Apply
to prefab**, **Revert all**, **Unpack**, **Open prefab** and **Select
prefab asset**; a disabled button carries its reason as the tooltip.

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
- **Unpack** (`prefab.unpack`, ⇧⌥⌘X) is one `unpackPrefab` transaction —
  the session's new command (a shared `fynkel_document` touch, M4-07):
  the anchor loses its `prefab` block, the expanded entities are inserted
  into the document right after it **under the derived ids** the runtime
  already used (so the edit-preview world does not even reload), the
  instance roots become the anchor's trailing children. One ⌘Z puts the
  instance back. Refused for an instance that does not expand.
- **Open prefab** (`prefab.open`, ⇧⌥⌘D) opens the prefab document — or the
  imported hierarchy, read-only — in its own tab and focuses it; the tab
  has its own Hierarchy, Inspector, edit-preview world and history like
  any scene tab (a prefab's roots are its `roots` list). **Select prefab
  asset** (`prefab.selectAsset`, ⇧⌥⌘L) shows the Project panel with the
  prefab document (or the model source / rig the hierarchy came from)
  selected.

### Imported hierarchies

An instance of `<model>#prefab` or `<rig>#prefab` expands like any other
(kind `source` or `rig` in the section header). **Open prefab** opens the
hierarchy the importer derives — `openDocument("<id>#prefab")`, a tab
labelled `hero.glb › prefab` / `column.rig.json › prefab` — as a
**read-only prefab document** (`state.sub == "prefab"`, the banner names
the source): its entities carry the derived `node/<i>` ids, so an
override written in the scene addresses exactly the rows this tab shows.
The Asset panel's sub-asset list has an open button on the `prefab`
sub-asset for the same tab. Overrides on such an instance work in full
(the Imported rig of the fixture's Rig scene is the test's case); Apply
is refused because the source cannot be edited; Unpack works and is the
way to "make it mine".

### Missing and broken prefabs

An instance whose prefab is gone (`FYN-D0401`), or whose override no
longer applies (`D0403` a component the prefab lost, `D0404`, `D0407`),
still opens: the anchor row keeps its badge and its data, the Inspector
section shows the broken icon and the diagnostics, the Console lists them,
Apply and Unpack are disabled with the reason, **Revert** (and Revert all)
and **Select prefab asset** stay available, and saving the document
without touching the instance writes the same bytes — nothing is
"repaired" behind the user's back. The `instance` query answers `expanded:
false` for such an anchor; the Hierarchy shows no derived rows under it.

### Commands

| Id | Menu | Shortcut | Does |
| --- | --- | --- | --- |
| `prefab.applyOverrides` | Edit | ⇧⌥⌘A | the selected instance's overrides into its prefab, then reverted here — two transactions in two documents |
| `prefab.revertOverrides` | Edit | ⇧⌥⌘U | every override of the selected instance dropped — one transaction |
| `prefab.unpack` | Edit | ⇧⌥⌘X | the instance replaced by its expansion as plain entities — one transaction |
| `prefab.open` | Edit | ⇧⌥⌘D | the instance's prefab (an imported hierarchy read-only) in its own tab |
| `prefab.selectAsset` | Edit | ⇧⌥⌘L | the prefab document or model source revealed in the Project panel |

All five are enabled when the selection lies in one instance (the anchor
or any of its derived rows), and appear in the Hierarchy's context menu.

### What item 7 does not do

Nested-instance apply across two levels of prefab (a nested instance's
overrides apply to the inner prefab, the outer document is untouched —
the crate's model); a visual diff of a prefab against an instance;
editing an imported hierarchy (import it with `fynkel import --prefab`
for an authored copy); picking a derived entity *in the picture* (a hit
inside an instance selects the anchor, item 6 — select the inner row in
the Hierarchy instead, then the gizmo is there).

## Play and the game Console (M4-08)

Explained simply: **Play** runs your game the way a player would get it —
a separate app in its own window, built by the same `fynkel run` you use
from a terminal. The editor does not run any of it: it starts the
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

### Play saves first

The game reads files from disk, so anything unsaved is not in the run.
With dirty tabs, Play asks: **Save and Play**, **Play Without Saving**,
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
| `play.start` | Play | ⌘P | saves what is dirty (after asking), then runs the project; the label reads *Playing…* while a run is up |
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
| 9 | — | Profiler, IDE navigation, `fynkel editor`, polish. |
| 10 | — | M4 gate and report. |

## Verification

```sh
cargo test -p fynkel_authoring                       # the session over a copy of the fixture project
cargo test -p fynkel_native --test authoring         # the ABI: statuses, results, poison, read-only
cd packages/fynkel_authoring && dart pub get && dart analyze --fatal-infos && dart test   # Flutter's dart
cargo build -p fynkel_native -p fynkel_platform_apple --no-default-features
nm -gU target/debug/libfynkel_platform_apple.dylib | grep -c fynkel_authoring_   # 0
cd apps/editor && flutter analyze --fatal-infos && flutter test   # 242 tests; documents_session_test.dart drives tabs + Hierarchy on the real session (a 10 000-entity scene, the conflict path through the watcher); inspector_session_test.dart sets every field of the fixture's scenes and the reference game's playground to itself and saves byte-identically; project_browser_session_test.dart renames, moves, deletes (refused, forced, undone), creates and re-imports on a copy of the fixture with `validateProject` clean after every step; scene_session_test.dart picks every entity of every fixture scene through the real edit-preview world, drags the gizmo (one undo step, the incremental world equal to a fresh load), hides without touching the document, drags the entity inside Crate A as one setPrefabOverride on the anchor (the prefab file's bytes unchanged, the sync incremental, one undo), looks through and writes back the scene camera, and renders through the real viewport (Metal, offscreen; the frame's PNG equals the first capture byte for byte); prefab_session_test.dart (M4-07) reverts and re-adds Crate A's override, instances the coin, applies an override to coin.prefab.json and undoes per document, unpacks and reloads, overrides and unpacks the Imported rig, opens a broken reference without rewriting the file, and opens rig.glb#prefab read-only — every override list checked against the raw document and `inspect`'s count; play_test.dart (M4-08) parses the channel's records behind a `flutter run` prefix, builds and signals a process tree from a canned `ps` table, drives a whole run over a fake process (the environment it was given, `starting → running → exited`, a crash's code, the dropped-row count, the CLI that is not there, a run that dies before the game starts — the device `flutter run` could not pick — with the Console showing the output that says why) and the shell (a project with errors refused onto the Project tab, the Game tab's rows, a diagnostic row that opens the scene with the entity selected, Stop, the "changes apply on the next Play" banner and its Play again — the old run stopped, the dirty document saved, a second process started and the banner gone —, the save prompt's three answers)
cd apps/editor && flutter test test/play_process_test.dart   # M4-08 against a *real* process: a shell script that announces itself on the channel and starts a grandchild — Stop takes the grandchild with it, a `kill -9` from outside ends in the exit code with the records it wrote still in the Console, and both leave every document in the project byte-identical
cd packages/fynkel && dart test test/editor_channel_test.dart  # the channel itself: `started`/`loaded` with a monotonic `seq`, a failed load as FYN- records then `loadFailed`, a missed deadline as `info`, `Diagnostics.log` as a log record with PrintDiagnostics quiet — and with the variable unset, not a byte written
FYNKEL_SCENE_FRAME_OUT=/tmp/main-320x200.png flutter test test/scene_session_test.dart --name viewport   # keeps the clean 320×200 frame of the fixture's Main through its camera …
cargo run -q -p fynkel_cli --bin fynkel -- render --project <fixture copy> --scene c0000000-0000-4000-8000-000000000030 --out /tmp/cli.png --size 320x200   # … to compare with this: 0 differing pixels (2026-09-22)
cargo test -p fynkel_authoring --test preview        # the edit-preview world: load, the incremental sync == a fresh load after every transaction kind, picking, outlines, hidden, the anchor map
cd packages/fynkel_flutter && flutter test          # viewport ABI v4: debug lines and read_frame on the real renderer
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
