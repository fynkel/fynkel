# ADR-0028: Editor extensions — a package's editor part, in Dart

**Status:** Proposed — by the assistant 2026-09-25, for the user's review  
**Date:** 2026-09-25  
**Supersedes:** ADR-0018 §2 ("no custom Inspector widgets, no custom windows, no editor scripts in 1.0") and §3 (a plugin SDK after 1.0, the reserved `capabilities: ["editor"]` manifest). ADR-0018 §1 (declarative hints) and its amendments stand.  
**Related:** ADR-0016 (the authoring service; documents change only through it), ADR-0017 (automation: stable command ids), ADR-0019 (declarative gizmos), ADR-0021 (the game compiled into its project editor, hot reload), ADR-0026 (code-first building: `ObjectSpec`), ADR-0027 (data assets, records), `docs/roadmap.md` Phase 3 items 1 and 3, `docs/ideas.md` §23.6

## Context

Unity lets a project ship editor code: a custom Inspector for a script,
a `PropertyDrawer` for a serialized class, an `EditorWindow`, `[MenuItem]`
commands and context-menu entries. The code lives in `Editor` folders (or
an editor-only assembly in a package) and never goes into a player build.
Every Unity package can carry such a part, which is how tools reach
projects: install a package, and its windows and inspectors appear.

Fynkel ruled this out for 1.0 (ADR-0018 §2) for two reasons:
- Flutter AOT could not load a project's Dart.
- The editor must never run game code.

ADR-0021 removed the first reason. A project now opens in its own editor
build with the game package compiled in, in JIT, with hot reload. It left
the scope question to "its own ADR", which is this one.

The user's steer (2026-09-25):
- Do extensions before the package architecture (roadmap Phase 3 item 3),
  but shape them as packages from the start.
- Unity's model is the reference: everything is a package, and an editor
  part is one thing a package can carry.

What exists already (mapped 2026-09-25):

- **The host.** `fynkel editor <dir>` generates
  `.fynkel/editor_host/lib/main.dart`, which imports
  `package:<game>/fynkel_game.dart` and calls
  `runFynkelEditor(args, game: EditorGame(…))`. Hot reload on save already
  covers the game's `lib/`.
- **Registries the editor uses itself.**
  - `CommandRegistry` / `EditorCommand` hold stable ids, menus, shortcuts
    and the palette.
  - The docking workspace's panels are identified by id, with a persisted
    layout.
  - `AssetDocumentSections.register(kind, builder)` adds pages to the
    asset Inspector.
- **Field editors that need no entity.** `FieldEditor` works on a
  `FieldTarget`: a component slot on the selection, or a data asset's
  `data`. Each change is prechecked and is one undo step. Prefab
  overrides, multi-selection and live Play edits come with it (ADR-0027
  step 6).
- **One way to change documents.** The authoring session's operations
  (`setField`, `addEntity`, …) are behind undo, prefab overrides and "Used
  by". `package:fynkel/authoring.dart` already turns an `ObjectSpec` into
  those documents' JSON.

## Decision

1. **Any package can have an editor part, the game included.**
   - **Where the code lives.** A Dart package's editor code is under
     `lib/editor/`, and its entry is `lib/editor/editor.dart`, which
     defines `void registerEditor(EditorExtensions editor)`.
   - **The game is simply the first package.** The same rule applies to
     any package the project depends on, which is what the package
     architecture will build on (ADR-0029, to come). A tool package
     installed with `pub` brings its windows and inspectors along, as a
     Unity package does.
   - **Discovery.** `fynkel editor` reads the project's resolved package
     graph (`.dart_tool/package_config.json`). For every package that has
     `lib/editor/editor.dart`, the generated host imports it and calls
     its `registerEditor`: dependencies first, then alphabetical, the game
     last. Adding or removing such a package regenerates the host and
     needs a hot restart.
   - **Seeing where things come from.** The editor lists each package
     that registered something, and what it registered, in
     Preferences › Extensions.

2. **Editor code never ships in a game.**
   - **One small API.** Extensions import one package,
     `package:fynkel_editor_api`. It is small, holds only Flutter widgets
     and interfaces, and has no platform plugins, so it adds nothing
     native to a game that depends on it. The editor (`fynkel_editor`)
     implements it.
   - **A validate rule.** No file outside `lib/editor/` may import
     `lib/editor/` or `fynkel_editor_api`. `fynkel validate` and
     `fynkel codegen --check` report a breach as an error with a new
     `FYN-` code. A game build's entry point therefore never reaches
     editor code, and tree shaking drops it.
   - **The build checks it too.** `fynkel build` fails if the compiled
     game still names the API.

3. **What an extension can register (the first set, Unity's common
   cases):**
   - **Custom Inspectors.** `editor.inspector<Collectible>(builder)`
     replaces the card of a script or the page of a data type (Unity's
     `CustomEditor`).
     - **The builder's context.** It gets an `InspectorContext`:
       - `field('value')` draws that field's standard editor;
       - `defaultInspector()` draws every field, as today;
       - `value('value')` / `set('value', v)` read and write one field;
       - `targets` is the multi-selection count.
     - **Writes.** A write is prechecked, is one undo step, becomes a
       prefab override where it should, and reaches the running game
       during Play: the `FieldTarget` path, unchanged.
     - **Scope.** It works on the document's values, Unity's
       `SerializedObject`. No behaviour instance runs in edit mode, so a
       custom Inspector never calls a script's methods (ADR-0016's edit
       world stands).
   - **Property drawers.** A drawer draws one field, wherever it
       appears (Unity's `PropertyDrawer`). It is chosen one of two ways:
     - for a record class (`editor.drawer<Wave>(builder)`, ADR-0027
       records), or
     - by name (`@Expose(drawer: 'minMax')` with
       `editor.namedDrawer('minMax', builder)`). The name goes into
       `x-fynkel.drawer`.

     Its `FieldContext` has the value, `set`, the label and the standard
     editors for sub-fields.
   - **Windows.**
     `editor.window(id: 'game.levelTools', title: 'Level Tools', menu: 'Tools/Level Tools', build: …)`
     is a dockable panel with a persisted place in the layout (Unity's
     `EditorWindow`).
   - **Menu items and commands.**
     `editor.menuItem('Tools/Snap to Grid', id: 'game.snapToGrid', run: …, shortcut: …, enabled: …)`
     becomes an `EditorCommand`. It appears in the menu bar, the command
     palette and automation (ADR-0017), with the extension's stable id.
     Context entries reach:
     - Hierarchy objects (optionally only those with a given script);
     - Project assets of a kind or data type;
     - a script card's ⋮ menu.

     These are Unity's `[MenuItem]`, `CONTEXT/` items and
     `[ContextMenu]`.

4. **Extensions change documents only through the editor.** Every
   callback gets an `EditorContext`:
   - **Selection.** Read it and set it.
   - **Edits.** `edit('Snap to grid', (tx) { … })` is one undo step made
     of authoring operations:
     - `tx.setField` and `tx.remove`;
     - `tx.add(ObjectSpec(…))`, which builds a whole tree the way
       ADR-0026 writes one, so code-first building and editor tools are
       one vocabulary.
   - **Assets.**
     - list them, and find them by kind or data type;
     - create one, e.g. `createData('data/sword', Weapon()..damage = 3)`,
       ADR-0027's `writeData` through the editor;
     - ping and open them.
   - **Console.** Log to it, attributed to the package.
   - **Play.** `play.world` is the running `World` during Play. Changes
     made through it follow Live Play mode: never saved unless applied.

   **Files are the editor's.** Extensions do not write project files
   behind the editor (ADR-0016). Dart could, but the API gives the path
   that keeps undo, overrides and references right, and the docs state
   the rule.

5. **An extension changes how data is shown, never whether it can be
   edited.**
   - **Everything still works without extensions:**
     - the plain editor (no game compiled in, or code that does not
       compile);
     - the CLI;
     - agents (ADR-0017);
     - the default Inspector.
   - **Nothing changes in the files.** Extensions add views and tools.
     They never add a file format, a field or a document that only they
     can read.

6. **A failing extension fails alone.**
   - **Every call is wrapped.** Registration, builders, commands and
     drawers are all wrapped. An exception becomes a Console error that
     names the package and the extension, and the editor's default takes
     its place:
     - a custom Inspector falls back to the default one, with a line
       saying why;
     - a window shows the error in its own frame;
     - a failing `registerEditor` skips that package's extensions.
   - **Loops still freeze.** An infinite loop freezes the editor, as in
     Unity and as ADR-0021 accepted.

7. **Editor code reloads like game code.**
   - **Saving is enough.** A save in `lib/editor/` hot reloads. The
     registry is rebuilt after every reload, so a new Inspector, window
     or menu item appears without a restart. Open windows keep their
     place.
   - **A refused reload** falls back to ADR-0021's hot restart.

8. **Hints first, code when hints run out.** ADR-0018's declarative hints
   remain the first tool: they cost nothing and agents can read them.
   The docs say so, and each extension point names the hint it goes
   beyond.

9. **Later, not in this set:**
   - scene-view drawing from code (Unity's `OnDrawGizmos`, `Handles`;
     ADR-0019's declarative gizmos stay the way to draw);
   - asset import hooks (`AssetPostprocessor`);
   - custom asset previews;
   - project validation rules.

   Each can join the same `registerEditor` entry when a real case asks
   for it.

## Alternatives considered

- **Packages first.**
  - **The case for it.** The package is the box and extensions are one
    thing in it.
  - **Why not.** Designing the box before its most useful content would
    guess at that content. Extensions are useful to a game's own
    project straight away. The package work is also much larger: engine
    modules, versions, publishing.
  - **The compromise.** Rule 1 makes extensions package-shaped now, so
    the package ADR builds on them without changing them.
- **A separate editor package per project** (`<game>_editor`, like a
  Unity editor-only assembly).
  - **For it.** It isolates editor code more strongly, through the
    package system itself.
  - **Against it.** Every project carries a second pubspec and an import
    across packages.
  - **The choice.** A folder plus a validate rule is Unity's `Editor`
    folder, which is what users know. (A `pub` dev dependency cannot be
    used, because a package's dev dependencies are not visible to the
    packages that depend on it, such as the host.)
- **Annotations** (`@CustomInspector(Collectible)`, like Unity's
  attributes).
  - **Against them.** They need a generator and a discovery pass.
    Registration in one function is plain Dart, hot reloads, and is
    easy to read.
  - **Later.** Sugar over the same registry is possible.
- **Declarative hints only** (ADR-0018 as it stands).
  - **Against it.** Hints cannot express:
    - a preview;
    - a button that places objects;
    - a table of waves;
    - a tool window.

    This is the ceiling ADR-0018 set on purpose, and the roadmap now
    asks to lift it.
- **Plugins out of process** (a separate process or a web view per
  extension).
  - **For it.** Real isolation.
  - **Against it.** Slow, not Flutter widgets, and every value would
    cross a channel.
  - **The choice.** ADR-0021 already chose one process for Play for the
    same reasons.

## Consequences

- Games and tool packages get Unity's four everyday editor extension
  points. The shipped game stays free of editor code, and the build
  checks that.
- **The API is a contract.**
  - `fynkel_editor_api` is versioned like the SDK and grows by addition.
  - The editor's internal widgets stay private behind it:
    `FieldTarget` is not exported as such, only the contexts are.
- **The editor gains a failure surface.** A project's editor code can
  throw inside the editor process. Rule 6 contains exceptions, but not
  hangs.
- **ADR-0018's reserved manifest is gone.** `capabilities: ["editor"]`
  gives way to the `lib/editor/` convention. The package ADR decides
  what, if anything, a manifest adds.
- **Agents and the CLI see no change.** Documents and schemas keep their
  meaning. The only additions they meet are `x-fynkel.drawer` and new
  commands with ids.

## Order

1. **The loop.**
   - `package:fynkel_editor_api` with `EditorExtensions` and
     `menuItem`;
   - the host's discovery of `lib/editor/editor.dart` across the package
     graph;
   - registration rebuilt after hot reload;
   - the validate rule and its code;
   - Preferences › Extensions.

   A menu item in `examples/code_only` proves it end to end.
2. **`EditorContext`.** Selection, `edit` transactions (`setField`,
   `remove`, `add(ObjectSpec)`), assets (list, find, `createData`, ping,
   open), Console.
3. **Custom Inspectors** for scripts and data types
   (`InspectorContext`: `field`, `defaultInspector`, `value`, `set`,
   `targets`), with the fallback of rule 6.
4. **Property drawers.** By record class and by name
   (`@Expose(drawer:)` → `x-fynkel.drawer`, through the generator).
5. **Windows and context menus.** Dockable, with a persisted layout;
   Hierarchy, Project and script-card entries.
6. **Play access.** `play.world` under Live Play rules.
7. **Acceptance.**
   - **In `examples/code_only`:**
     - a `CoinKind` Inspector with a colour swatch and the coin drawn at
       its size;
     - a Level Tools window that places a ring of coins as one undo
       step, through `ObjectSpec`;
     - a Hierarchy entry "Make Big Coin".
   - **In `spinner_game`:** a drawer for the Spawner's `Wave`.
   - **The plain editor** still edits every field.
   - **A release build** contains no editor code.
   - **On a copy of the project, the user checks** that an edit to
     `lib/editor/` appears in the running editor on save.
