# ADR-0029: Package architecture — everything a game uses is a package

**Status:** Accepted — proposed by the assistant 2026-09-25, confirmed by the user 2026-09-25 (modules optional whatever their size; assets as Unity has them, hosted read-only, path editable, Import Package Assets…; publishing stops at the local rehearsal: "Let's go with what you recommended")  
**Date:** 2026-09-25  
**Related:** ADR-0005 (license; nothing is published before it), ADR-0006 (project format, asset identity), ADR-0012 (codegen, the schema registry), ADR-0015 (native runtime distribution), ADR-0016 (the authoring service), ADR-0023 and ADR-0026 §4 (the built-in primitives), ADR-0027 (data assets, records), ADR-0028 (editor parts, found across the package graph), `docs/roadmap.md` Phase 3 item 3, `docs/m3-gate-report.md` E2

## Context

Unity's model is the reference (the user's steer, ADR-0028): everything is
a package.
- **A package carries everything a game can.** A UPM package has runtime
  scripts, editor scripts, assets (prefabs, materials, ScriptableObjects)
  and samples. Installing it makes all of them usable at once. Its assets
  show under **Packages/** in the Project window. A registry package is
  read-only there. A local or embedded package (one on disk, or inside
  the project's `Packages/` folder) is editable in place.
- **Unity also has asset packages, and they are a different thing.** An
  Asset Store `.unitypackage` is an archive of files. Importing it copies
  them into `Assets/`, and from then on they are the project's own,
  editable. It has no version link: an update is a re-import over the
  copies.
- **The engine's own features are packages.** Physics, Audio and
  Animation are *built-in packages* (modules), whatever their size. A
  project can switch one off in the Package Manager: its components
  leave Add Component, and a player build can leave its code out.
- **Packages come from a registry**, pinned by version, with no engine
  source needed.

What Fynkel has (mapped 2026-09-25):
- **One package matters, the game.**
  - The generator writes `registerBehaviours` for the package it runs in,
    and the game calls it by hand (`lib/game/game.dart`).
  - The project reads schemas from its own `schemas/`.
  - The project has one asset root, `assets/`. The only asset outside it
    is the built-in `Fynkel/Primitives.glb`, added as a special case
    (`fynkel_import::builtin`).
  - Only editor parts are found across dependencies (ADR-0028: the
    pubspec's `dependencies`, located through
    `.dart_tool/package_config.json`).
- **The engine is one block.**
  - `fynkel_core` depends on `fynkel_physics` (rapier) and `fynkel_audio`
    (cpal with the `device` feature) without a feature gate. Animation
    lives inside `fynkel_core`.
  - `World::step` runs every system. `ComponentValue` is a closed enum.
  - The Dart SDK exposes `world.audio`, `raycast`, `applyImpulse` and
    `animation` unconditionally.
- **Nothing is published.**
  - Every package is `publish_to: none`, and games depend on them by path.
  - `fynkel create` needs a checkout (`--root`).
  - The build hook can download a prebuilt runtime, SHA-256 checked,
    from a release store named in `packages/fynkel/fynkel.lock.json`.
    Every entry there is unpublished, and that lock has a different
    shape from a project's `fynkel.lock.json` (blueprint D1, C6; the M3
    gate's E2).
  - ADR-0005: nothing goes to pub.dev or crates.io before it is
    accepted.

## Decision

1. **A Fynkel package is a Dart package, found through the package graph.**
   - **The graph.** One walk, the one ADR-0028 uses: the game's pubspec
     `dependencies` (never `dev_dependencies`), transitively, located
     through `.dart_tool/package_config.json`. It moves into
     `fynkel_document`, so the CLI, the authoring session (editor) and
     the cook share it. Order: dependencies first, then alphabetically;
     the game last.
   - **What makes it a Fynkel package.** A `fynkel.package.json` at its
     root, beside `pubspec.yaml`, the way `fynkel.project.json` marks a
     project: `formatVersion`, `assets` (default `"assets"`), `namespace`
     (default: the package name). A package without it can still carry
     an editor part (ADR-0028). Nothing else about it is read. A
     `fynkel.package.json` that cannot be read is `FYN-P0019`, and the
     package is then not a Fynkel package.
   - **Versions are pub's.** A package states the engine versions it works
     with in its pubspec (`fynkel: ^1.2.0`), and `pub` resolves them.
     Fynkel adds no resolver of its own.

2. **A package carries scripts, data types, assets and an editor part.**
   - **Scripts and data types.**
     - Each package runs the generator for itself, as a game does. It
       commits `lib/generated/behaviours.g.dart` and `schemas/`.
     - The game's codegen also writes `lib/generated/packages.g.dart`:
       `registerPackages(registry)`, which calls every package's
       `registerBehaviours` in graph order. The game calls that one
       function; the template and the examples change to it.
     - The editor registers through the same function.
   - **Ids are namespaced.** A dependency's type ids start with its
     namespace (`pickups.Pickup`, `pickups.PickupKind`). The game keeps
     `game.`.
     - A clash between two packages, or a dependency's id outside its
       namespace, is a project error: `FYN-P0016` and `FYN-P0017`, from
       `fynkel validate` and codegen.
   - **Schemas** are read from every package's `schemas/`, then the
     game's, into one registry. `fynkel.project.json`'s `components`
     registry lists a package's types as it lists the game's.
   - **Assets.**
     - A package's asset folder is an asset root of its own, mounted at
       `Packages/<name>/`. This is Unity's Packages folder. Ids come from
       sidecars and documents as today, so a reference into a package is
       an ordinary asset reference.
     - **Editable or not follows where the package lives**, as in Unity:
       - A **hosted** package (pub.dev, git) lives in pub's shared cache,
         which pub may overwrite and other projects share. Its assets are
         **read-only**: the Inspector shows them and refuses an edit,
         with the reason.
       - A **path** package (`path:` in the pubspec: the user's own
         package, beside the game or inside it) is **editable in place**,
         like Unity's local and embedded packages. That is how a package
         is made.
     - **Copy into Project** copies an asset, or a whole folder, of a
       package into the project's `assets/`, with new ids. The copy is
       the project's own. It is the way to change a hosted package's
       asset.
     - **Import Package Assets…** is the `.unitypackage` counterpart: it
       copies a whole package's asset folder into the project's
       `assets/<name>/` in one step. The package stays a dependency for
       its code. This is for asset packs (models, materials, sounds): the
       game owns and edits them, and they have no update path.
     - The Project panel shows a **Packages** section under the project's
       assets.
     - The cook treats them as project assets. `package:fynkel`'s own
       `assets/` holds `Primitives.glb`, so the built-ins stop being a
       special case (ADR-0026 §4 keeps its ids and its behaviour).
   - **The editor part** is ADR-0028's, unchanged.

3. **Physics, audio and animation become optional modules**, as Unity's
   built-in packages are, whatever the size saved (the user, 2026-09-25).
   - **Measured, not gated.** The runtime library (`fynkel_platform_apple`,
     release, macOS arm64) is built with each module left out, and the
     sizes are recorded in this ADR. The numbers inform the docs; they
     decide nothing.
   - **Where the switch is.**
     - `fynkel.project.json` gets `modules: { physics, audio, animation }`,
       all `true` by default. Unity's built-in packages have the same
       list, and the editor shows it in Project Settings › Modules.
     - The build hook turns the list into cargo features:
       `physics`, `audio` and `animation` on `fynkel_core` and above,
       default on.
   - **The contract stays whole.**
     - Every component, command, event and record stays in the one
       registry. Their ids and the schema fingerprint are the same in
       every build, so documents, the wire and the ABI never depend on
       the module set.
     - What a module takes away is its systems and its dependencies
       (rapier, cpal, symphonia), not its types.
   - **When a module is off:**
     - A document that uses its components is an error in `fynkel
       validate` and the editor (`FYN-P0018`, naming the module and the
       setting).
     - Add Component does not offer them.
     - The Dart calls that need the module (`world.audio.play`,
       `raycast`, `applyImpulse`, `animation.play`) throw a `StateError`
       naming the module. `world.modules.has(Module.physics)` answers
       first.
     - Dart tree-shaking already drops what a game never calls.
   - **Animation moves into its own crate**, `fynkel_animation`, as
     physics and audio already are. It is a move, with no behaviour
     change.

4. **Published artifacts: prepared, rehearsed locally, published only
   after ADR-0005.**
   - **What gets published:**
     - the Dart packages `fynkel`, `fynkel_flutter`, `fynkel_generator`
       and `fynkel_editor_api` on pub.dev;
     - the runtime library for each target (the full module set) as
       release assets, SHA-256 in the hook lock;
     - the `fynkel` CLI.
   - **One lock shape.** The hook's artifact list joins the project lock
     format (blueprint D1). A project pins the runtime version, and
     through it the artifacts.
   - **One prebuilt, the full module set.** A module switched off is off
     in behaviour in every build: no systems, no components offered, the
     errors above. The prebuilt library still carries its code, as
     Unity's desktop players do. A project that wants the bytes gone too
     builds the runtime from source (cargo), with the setting
     `modules.strip: true`; the hook says so before building.
   - **`fynkel create` without a checkout** writes hosted dependencies
     (`fynkel: ^x.y.z`).
   - **Rehearsal before publishing.** A local release store (a directory,
     `file://`) receives the whole set, and a new project made outside
     the checkout builds and runs against it: no checkout, no cargo. The
     real publish waits for ADR-0005 and the user's word; the repository
     stays private until then.

5. **Later, not in this set:**
   - a package registry of Fynkel's own (pub.dev is the registry);
   - package samples imported on demand (Unity's "Import Sample");
   - native (Rust) code in a game package. A package's code is Dart. A
     native component still means a Fynkel release (ADR-0011).

## Alternatives considered

- **Assets copied into the project on install**, as `.unitypackage` files
  are. Rejected: updating the package would not update the copies, and
  ids would clash on a second install. Unity moved away from this with
  UPM.
- **Splitting the Dart SDK into `fynkel_physics`, `fynkel_audio` and
  `fynkel_animation` packages.** Rejected for now:
  - tree-shaking already drops unused Dart code;
  - the wire types are generated together;
  - the split would break every game's imports for no measurable gain.
  The native side is where the weight is.
- **A separate component registry and fingerprint per module.** Rejected:
  every module combination would become its own ABI, and documents would
  depend on build settings.
- **A Fynkel resolver or registry.** Rejected: pub already resolves
  versions and hosts packages, and Flutter games already use it.

## Consequences

- **What a game gets.** A game can depend on a package and get its
  scripts, data types, prefabs and editor tools, with no copying and no
  setup call per package.
- **The built-ins stop being a special case.** The primitives come from
  `package:fynkel`'s own assets.
- **Multi-root assets.** The asset database, the authoring session, the
  cook and the Project panel handle several roots. Hosted package roots
  are read-only; path package roots are editable.
- **The ABI stays whole.** Module switches never change the ABI or the
  documents.
- **Cost of a smaller build.** Turning a module off needs nothing. Removing
  its bytes from the build needs the Rust toolchain until per-module
  artifacts are worth publishing.
- **Publishing needs the user.** Nothing becomes public without ADR-0005
  and the user's go.

## Order

1. **The package graph in `fynkel_document`** (moved from the editor
   host), with `fynkel.package.json`.
2. **Scripts and data types across packages:**
   - `packages.g.dart` and `registerPackages`;
   - schemas from every package;
   - `FYN-P0016` and `FYN-P0017`;
   - the template, the examples and the editor host use it.
3. **Package assets:**
   - asset roots per package, mounted at `Packages/<name>/`;
   - hosted read-only, path editable; Copy into Project; Import Package
     Assets…;
   - the Project panel's Packages section;
   - the cook;
   - `Primitives.glb` into `package:fynkel`.
4. **An example package:** `examples/packages/pickups`, with a `Pickup`
   script, a `PickupKind` data type, a pickup prefab and material, and
   an editor Inspector. `examples/code_only` uses it for a new pickup,
   placed from the package's prefab.
5. **Modules:**
   - `fynkel_animation`, the cargo features, `modules` in the project,
     `FYN-P0018`, Add Component, the Dart errors, Project Settings ›
     Modules, `modules.strip`;
   - the sizes recorded;
   - `examples/code_only`, which uses no animation, plays with animation
     off.
6. **Artifacts:**
   - one lock shape;
   - `fynkel create` with hosted dependencies;
   - the local release-store rehearsal.
7. **Acceptance:**
   - code_only plays with the pickups package: its prefab placed in the
     editor, its Inspector shown; as a path package its assets edit in
     place, and as a git dependency (a local `file://` repository, so it
     lands in pub's cache) they are read-only and Copy into Project
     works;
   - the module measurement recorded;
   - a project outside the checkout builds against the local store;
   - the user's check in the editor.
