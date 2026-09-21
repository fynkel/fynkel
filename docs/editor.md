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
`Hierarchy | Scene over [Project, Console, Profiler, Animation, Import] | Inspector`,
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
| `app.preferences` | Edit | ⌘, | Preferences |
| `window.commandPalette` | Window | ⇧⌘P | the palette |
| `window.resetLayout` | Window | ⌥⌘0 | the §22.2 default layout |
| `window.panel.<id>` | Window | ⌥⌘1 … ⌥⌘8 | show/hide Hierarchy, Scene, Inspector, Project, Console, Profiler, Animation, Import (the label flips between Show and Hide) |
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
prefab instance (item 7), drag-in from the Project browser (the target
exists; item 5 supplies the drag), the runtime view of a behaviour during
Play (after V0).

## What exists after each item

| Item | Done | What |
| --- | --- | --- |
| 1 | 2026-09-21 | `crates/fynkel_authoring`, the `authoring` feature and `fynkel_authoring_*` ABI in `fynkel_native`, `packages/fynkel_authoring`; D5 and D7 closed; the game runtime carries no editor code (CI check). |
| 2 | 2026-09-21 | `apps/editor`: launcher, docking workspace saved per user, native menu bar + command registry + palette, Console with open-in-IDE, read-only banner, migration preview, preferences (§ The shell). |
| 3 | 2026-09-21 | Document tabs with save / save-as / revert / close prompts, per-tab undo, the conflict dialog with a diff; the Hierarchy over the session (patched from `affectedIds`, one transaction per operation, keep-world-transform drops), Console → tab (§ Document tabs and the Hierarchy). |
| 4 | 2026-09-21 | The Inspector: one parser over schema + metadata + the editor's native-hint table (ADR-0018 accepted), one widget per field kind, multi-object editing with mixed values and one transaction per gesture, precheck + session validation with the message kept, Add / Remove Component, prefab override revert, Open script, the codegen banner over `codegen --check --json` (§ The Inspector). |
| 5 | — | The Project browser. |
| 6 | — | The Scene view (edit-preview world, camera, picking, gizmos). |
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
cd apps/editor && flutter analyze --fatal-infos && flutter test   # 116 tests; documents_session_test.dart drives tabs + Hierarchy on the real session (a 10 000-entity scene, the conflict path through the watcher); inspector_session_test.dart sets every field of the fixture's scenes and the reference game's playground to itself and saves byte-identically
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
