# ADR-0023: Built-in primitives and GameObject presets

**Status:** Accepted (2026-09-24, the user: "ok continue", in reply to the proposal in the Unity parity pass)  
**Date:** 2026-09-24  
**Related:** ADR-0006 (document format, asset references by id), ADR-0016 (the editor as a Flutter app over the authoring session), ADR-0022 (one-root prefabs), `docs/unity-parity.md` § Hierarchy, `docs/documents.md` § Session operations, `docs/editor.md` § Hierarchy

## Context

In Unity, a first-hour user opens GameObject → 3D Object → Cube and a
cube appears. It is selected and ready to rename, lit by the scene's
Directional Light and seen by its Camera. The same menu has Light →
Directional / Point / Spot and Camera.

Fynkel has nothing to show. The Hierarchy's Create menu lists
"Create with fynkel.X" by raw component type. A MeshRenderer needs a
`mesh` and a `material`, and a new project has no mesh except the
`fynkel create` template's `assets/models/cube.glb`. Nothing visible can be
made from the menu, and a light or a camera starts with defaults that are
not Unity's (a Directional Light pointing straight down, no rotation).

Unity's built-in meshes are not files in `Assets/`; the engine knows
them. Fynkel's rule is that everything a game uses is an asset in the
project: it is cooked, has an id, and resolves the same way in the editor,
the CLI and a shipped build. Teaching the importer, the cook and the
runtime about assets outside the project is a much larger change.

## Decision

1. **One primitives model, generated, with a fixed id.**
   - `crates/fynkel_authoring/assets/primitives.py` writes `Primitives.glb` beside itself. Like
     `examples/reference_game/tools/kit.py`, the repository carries no
     binary of unknown origin.
   - The meshes have Unity's sizes and are centred at the origin:

     | Mesh | Shape |
     |---|---|
     | `mesh/Cube` | 1 m |
     | `mesh/Sphere` | diameter 1 m |
     | `mesh/Capsule` | 2 m tall, radius 0.5 m, along Y |
     | `mesh/Cylinder` | 2 m tall, radius 0.5 m |
     | `mesh/Plane` | 10 × 10 m, facing +Y, 10 × 10 quads |
     | `mesh/Quad` | 1 × 1 m, facing +Z |

   - It also has one material, `material/Default`: a light grey
     metal-rough material with roughness 0.5.
   - Its sidecar has a **fixed, well-known asset id**,
     `f7a1e5d0-0000-4000-8000-000000000023`. Every sub-asset id is derived
     from the asset id (UUID v5, as for every model), so `mesh/Cube` has the
     same id in every project. The editor can build a reference without
     asking the project.
   - `fynkel_authoring` embeds the glb and its sidecar.

2. **Copied into the project on first use.**
   - A new session operation, `installPrimitives`:
     - If an asset with the well-known id exists, it returns that asset's
       path and changes nothing. A moved or renamed copy is found by id.
     - Otherwise it writes `assets/Fynkel/Primitives.glb` and its
       sidecar. The file is imported like any other model.
   - It is recorded in the asset history like `duplicateAsset`, so undo
     removes the files it wrote.
   - The files are then the project's own: you can move them, rename
     them, or remap their material (material remapping, 2026-09-24).
   - Deleting them is allowed, and so is editing the glb. If you do, the
     next 3D Object puts back the file, or finds the edited one by its id.
   - Nothing is added to a project until you use the menu, so existing
     projects and the reference game are unchanged.

3. **GameObject presets in the editor, named as in Unity.**

   | Menu item | Name | Components |
   |---|---|---|
   | 3D Object → Cube, Sphere, Capsule, Cylinder, Plane, Quad | `Cube`, `Sphere`, … | Transform; MeshRenderer on `mesh/<Shape>` + `material/Default` (casts and receives shadows); a Collider (below) |
   | Light → Directional Light | `Directional Light` | Transform tilted 50° down and turned 30°, as Unity's default sun; DirectionalLight with the template's values (intensity 10000 lux, casts shadows) |
   | Light → Point Light | `Point Light` | Transform; PointLight with its component defaults |
   | Light → Spot Light | `Spot Light` | Transform pointing down; SpotLight with its component defaults |
   | Camera | `Camera` | Transform; Camera with its component defaults |

   The collider for each 3D Object, as Unity (a Collider alone is static
   geometry):

   | Shape | Collider |
   |---|---|
   | Cube | box, half extents 0.5 |
   | Sphere | sphere, radius 0.5 |
   | Capsule | capsule, radius 0.5, half segment 0.5 |
   | Cylinder | capsule, radius 0.5, half segment 0.5 (Unity also gives a cylinder a capsule collider) |
   | Plane, Quad | mesh collider on the same mesh |

   - The presets are offered in three places:
     - a new **GameObject** menu in the menu bar;
     - the Hierarchy's **+** menu;
     - the Hierarchy's right-click menus (empty space and row).
   - A preset is created at the root, or, from a row, as that row's
     child. As Unity does, it is selected and renamed right away.
   - One undo removes it. When the menu also installed the primitives,
     that is a second step in the Project's history.
   - "Create with fynkel.X" stays, in a submenu, for everything else.

4. **The presets live in the editor, not in the formats.** A preset is
   a list of components with values, built from `componentDefaults` and
   changed where Unity differs. No format changes, the ABI stays 1, and
   the fingerprint is unchanged.

## Consequences

- A new user can build a lit scene from menus alone, which is the first
  row of the Unity parity checklist that was ❌.
- `assets/Fynkel/` is the first folder the editor writes into a project
  without being asked by name. Its name is fixed and documented, so a
  user knows what it is.
- The glb, 65 KB, is embedded once in the authoring library.
- Built-in *materials* for the other presets (for example Unity's
  Default-Line or Sprites-Default) are out of scope.
- The `fynkel create` template's `cube.glb` stays for now. A later change
  can switch the template to `Primitives.glb`.

## Alternatives

- **Engine-known built-in assets outside the project, as in Unity.** The
  importer, the cook, the runtime and the reference checker would all need
  a second kind of asset. This was rejected for 1.0. The fixed id keeps
  the door open: a later ADR can resolve that id without a file.
- **Procedural meshes as a component (a `Primitive` component).** This
  adds a new component, a cook path and a runtime mesh builder, and it
  cannot be remapped or edited like any other model. Rejected.
- **Add the glb to every new project from the template.** This does
  nothing for existing projects, and the editor still needs to find the
  model. The copy-on-first-use step covers both.
