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
(item 7). Materials open as tabs but have no Hierarchy (a material has no
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
prefab instance (item 7), the runtime view of a behaviour during Play
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

## What exists after each item

| Item | Done | What |
| --- | --- | --- |
| 1 | 2026-09-21 | `crates/fynkel_authoring`, the `authoring` feature and `fynkel_authoring_*` ABI in `fynkel_native`, `packages/fynkel_authoring`; D5 and D7 closed; the game runtime carries no editor code (CI check). |
| 2 | 2026-09-21 | `apps/editor`: launcher, docking workspace saved per user, native menu bar + command registry + palette, Console with open-in-IDE, read-only banner, migration preview, preferences (§ The shell). |
| 3 | 2026-09-21 | Document tabs with save / save-as / revert / close prompts, per-tab undo, the conflict dialog with a diff; the Hierarchy over the session (patched from `affectedIds`, one transaction per operation, keep-world-transform drops), Console → tab (§ Document tabs and the Hierarchy). |
| 4 | 2026-09-21 | The Inspector: one parser over schema + metadata + the editor's native-hint table (ADR-0018 accepted), one widget per field kind, multi-object editing with mixed values and one transaction per gesture, precheck + session validation with the message kept, Add / Remove Component, prefab override revert, Open script, the codegen banner over `codegen --check --json` (§ The Inspector). |
| 5 | 2026-09-21 | The Project browser: folders and a searchable flat list with kind filters and sort, texture previews and material swatches, the Asset panel (sidecar settings, Apply and Reimport, sub-assets, dependents), New / rename / move / delete-with-refusal / Reveal / Copy id as session operations with the asset history's own undo (⌥⌘Z), prefab drop onto the Hierarchy, incremental rescan (§ The Project browser). |
| 6 | 2026-09-22 | The Scene view: `FynkelViewport` per scene tab over the session's edit-preview world (incremental sync, fallback reload, Reload and Rebuild cache), the editor camera saved per scene in `.fynkel/editor.json`, grid / axes / light-camera-collider icons / selection outline as renderer debug lines, click (stacked-hit cycling) and marquee picking through the world, move / rotate / scale gizmos in world or local axes with snapping and one transaction per drag (preview in the world, commit on release, multi-selection about the lead), editor-only hidden and locked flags, prefab drop at the hit point, stats overlay, Save Frame as a PNG identical to `fynkel render` (§ The Scene view). |
| 7 | — | Prefab mode. |
| 8 | — | Play / Stop and the game Console over the stdout channel. |
| 9 | — | Profiler, IDE navigation, `fynkel editor`, polish. |
| 10 | — | M4 gate and report. |

## Verification

```sh
cargo test -p fynkel_authoring                       # the session over a copy of the fixture project
cargo test -p fynkel_native --test authoring         # the ABI: statuses, results, poison, read-only
cd packages/fynkel_authoring && dart pub get && dart analyze --fatal-infos && dart test   # Flutter's dart
cargo build -p fynkel_native -p fynkel_platform_apple --no-default-features
nm -gU target/debug/libfynkel_platform_apple.dylib | grep -c fynkel_authoring_   # 0
cd apps/editor && flutter analyze --fatal-infos && flutter test   # 194 tests; documents_session_test.dart drives tabs + Hierarchy on the real session (a 10 000-entity scene, the conflict path through the watcher); inspector_session_test.dart sets every field of the fixture's scenes and the reference game's playground to itself and saves byte-identically; project_browser_session_test.dart renames, moves, deletes (refused, forced, undone), creates and re-imports on a copy of the fixture with `validateProject` clean after every step; scene_session_test.dart picks every entity of every fixture scene through the real edit-preview world, drags the gizmo (one undo step, the incremental world equal to a fresh load), hides without touching the document, looks through and writes back the scene camera, and renders through the real viewport (Metal, offscreen; the frame's PNG equals the first capture byte for byte)
FYNKEL_SCENE_FRAME_OUT=/tmp/main-320x200.png flutter test test/scene_session_test.dart --name viewport   # keeps the clean 320×200 frame of the fixture's Main through its camera …
cargo run -q -p fynkel_cli --bin fynkel -- render --project <fixture copy> --scene c0000000-0000-4000-8000-000000000030 --out /tmp/cli.png --size 320x200   # … to compare with this: 0 differing pixels (2026-09-22)
cargo test -p fynkel_authoring --test preview        # the edit-preview world: load, the incremental sync == a fresh load after every transaction kind, picking, outlines, hidden, the anchor map
cd packages/fynkel_flutter && flutter test          # viewport ABI v4: debug lines and read_frame on the real renderer
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
