# ADR-0019: Gizmos, handles and Scene tools — one overlay renderer, registered tools, declarative gizmos for game code

**Status:** Accepted (2026-09-23, the user's call: "let's do this with your recommendation and order")  
**Date:** 2026-09-23  
**Related:** ADR-0016 (the editor never executes game code), ADR-0018 (declarative Inspector hints), ADR-0012 (committed codegen metadata), `docs/blueprint.md` §19.2e items 9f–9h, `docs/renderer.md`, `docs/editor.md` § The Scene view

## Context

The Scene view draws three kinds of things over the picture:

- **gizmos** — annotations nobody drags: the ground grid, the selection outline, light ranges, camera frustums, colliders;
- **handles** — things the user drags: the move/rotate/scale gizmo today, a light's range or a collider's size tomorrow;
- the picture's own **overlays** — the marquee, the orientation gizmo.

At the end of item 9e they were built three ways, and the user judged the result "pixelated and not clean":

1. Gizmos go through the renderer's debug lines (`fynkel_render::debug`): a `LineList` pipeline, so every line is one hardware pixel wide with no anti-aliasing of its own. They are depth-tested against the scene, so a line behind anything simply breaks. Alpha is ignored. They are drawn into the HDR target before the post pass, which lets MSAA's partial coverage push a thin line under the "display-referred" alpha marker, and then the camera's exposure darkens it (the quirk recorded with M4-06). The ground grid sits exactly on `y = 0`, where floors are, and z-fights with them into dotted lines.
2. The transform gizmo is painted by Flutter (`CustomPaint`) from Dart-side projection. It is crisp, but it knows nothing about depth, and the three tools are `switch` statements across a 1 700-line view model. Adding a tool means editing all of them.
3. Game code has no way to draw anything. Unity's `Gizmos.DrawWireSphere` in `OnDrawGizmos` is the thing users reach for first. Unity runs that method *inside the editor*, which ADR-0016 rules out for Fynkel: Flutter AOT cannot load a project's Dart, and the editor runs no game code.

## Decision

1. **One overlay renderer (item 9f, `fynkel_render`).** Gizmos and handles draw in a dedicated **overlay pass after the post pass**, into the display-referred target. They are therefore never exposed, tone mapped, bloomed or resolved through MSAA coverage, and the M4-06 quirk is gone by construction. The primitives:
   - **lines as screen-space quads** — a width in logical pixels, an analytic anti-aliased edge (a one-pixel smoothstep), real alpha blending;
   - **depth-aware without breaking**: a line tests against the scene's depth, and the part that is behind geometry is drawn at a fraction of its alpha (default 0.25), not dropped. A primitive can opt out (`always on top`: handles);
   - **filled triangles** (flat, unlit, blended) for handle tips, plane squares and discs;
   - an **analytic ground grid**: a full-screen pass that intersects each pixel's view ray with the plane, draws minor/major lines by screen-space derivatives (no moiré, no z-fight: it reads the scene depth and never writes it), fades with distance and with grazing angle, and colours the X and Z axes.
   The old `DebugDraw` API keeps its signature and becomes a thin client of the overlay (width 1, occluded-faded), so every existing caller and test keeps working.
2. **Handles and tools are registered, not switched (item 9g, `apps/editor`).** A **handle** is a value: a shape (axis arrow, plane square, ring, dot, box face, sphere radius), its hit test in screen space, and a drag *constraint* that turns pointer motion into a value (along an axis, in a plane, around an axis, along a radius). A **tool** is a registered object with an id, icon, shortcut and `handlesFor(selection)`; move, rotate and scale are rebuilt as three registrations over the handle framework, and a combined transform tool is the proof that a fourth costs no surgery. **Component handles** — a light's range, a spot's angle, a box/sphere/capsule collider's size, a camera's field of view — are registered per component type, behind an "Edit" toggle in the Inspector like Unity's *Edit Collider*. Every drag stays one transaction (M4-06's rule). Handles render through the overlay pass (item 1); hit-testing stays in Dart.
3. **Game code draws gizmos two ways (item 9h, `package:fynkel` + generator).**
   - **At run time — `Gizmos`.** An immediate-mode API in `package:fynkel`: `line`, `ray`, `wireSphere`, `wireBox`, `wireCapsule`, `wireCircle`, `arrow`, `frustum`, with a `color` and a `matrix` scope, callable from any behaviour. It feeds the renderer's overlay in the **game's own window**, and is on in debug and profile builds only when the view asks for it (`FynkelGameView(gizmos: true)`, or `FYNKEL_GIZMOS=1` in the environment the editor's Play passes). In release builds every call is a no-op behind a `const bool` that the compiler strips.
   - **In the editor, while editing — declarative gizmos.** The editor never runs game code, so a behaviour *describes* its gizmos with an annotation on the fields they draw: `@Gizmo.radius()`, `@Gizmo.box()`, `@Gizmo.point()`, `@Gizmo.direction()`, `@Gizmo.cone(angle: 'field')`, `@Gizmo.lineTo()` (an entity field), `@Gizmo.path()` (a `List<Vec3>`). Each takes `color`, `when: selected | always` and `space: local | world`. `fynkel codegen` writes them into `behaviours.g.json` and the schema's `x-fynkel` (as ADR-0018's hints). The editor draws them through the overlay, and each is also a **handle**: dragging a `radius` gizmo edits the field, one transaction. Native components get the same through the editor's `native_hints.dart` table (ADR-0018's precedent).
4. **Colours and sizes are the editor's tokens.** X red, Y green, Z blue, the selection the accent, hover lighter, gizmo lines 1.5 px, handles 2.5 px. One place (`lib/src/theme/`) sets them.

## Alternatives considered

- **Keep `LineList` and turn on MSAA harder.** It stays one pixel wide, still breaks behind geometry, still z-fights, still has no alpha.
- **Draw everything in Flutter (`CustomPaint`).** It is crisp, but it has no scene depth. That is fine for a handle, which should sit on top, and wrong for a collider behind a wall. It also means projecting thousands of grid and outline segments in Dart every frame.
- **Run the project's `onDrawGizmos` in the editor (Unity's model).** Impossible under Flutter AOT without compiling project code into an editor build. It is exactly the plugin-SDK road ADR-0018 §3 leaves for after 1.0, and a `capabilities: ["editor"]` package can revisit it there. Declarative gizmos cover the common cases (a range, a box, a direction, a target, a path) and double as handles, which Unity's `OnDrawGizmos` does not.
- **Stream the running game's `Gizmos` into the editor's Scene view.** The Scene view shows the edit-preview world, not the running game's, so the gizmos would float over the wrong state. They belong in the game's own window.

## Consequences

- `fynkel_render` gains the overlay pass and pipelines. The Apple viewport ABI bumps for the new gizmo entry point. The wire fingerprint is untouched, because gizmos are never simulation state.
- The editor's Scene view model loses its tool `switch`es. `gizmo.dart` becomes the handle framework.
- `package:fynkel` gains `Gizmos` and `@Gizmo`. `package:fynkel_generator` gains the emission. Both are shared touches, each its own commit.
- The ground grid is a shader, not 1 681 line segments a frame.
