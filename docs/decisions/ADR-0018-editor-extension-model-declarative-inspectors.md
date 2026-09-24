# ADR-0018: Editor extension model for 1.0 — declarative Inspector hints, no game code in the editor

**Status:** Accepted (2026-09-21, with M4 item 4 — the Inspector; proposed the same day at the start of M4)  
**Date:** 2026-09-21  
**Related:** ADR-0012 (committed codegen metadata), ADR-0016 (the editor never executes game code), `docs/blueprint.md` §11.2, §19.2e; `docs/design/editor-ux.md` §22.6; `docs/ideas.md` §23.6

## Context

Unity lets a project ship C# editor code that draws a component's Inspector or a whole window. Fynkel's editor **never executes game code** (ADR-0016): the Inspector is generated from committed metadata, and Flutter AOT cannot load a project's Dart at run time anyway. Yet the editor-UX spec asks for "custom inspectors as an extension point" and the user wants the editor to be as capable as Unity's for the common cases: grouping, headers, ranges, units, colour fields, asset kinds, read-only and hidden fields, conditional visibility.

The question is what "custom inspector" means in 1.0 without breaking the no-game-code rule.

## Decision

1. **Inspector customisation in 1.0 is declarative.** A behaviour describes how its fields are shown through annotations that `fynkel codegen` (the behaviour generator, ADR-0012) turns into metadata in `lib/generated/behaviours.g.json` and `x-fynkel` hints in the behaviour's JSON Schema. The editor reads the metadata as data. The hint set for 1.0, on top of today's `@Expose(label, tooltip, min, max, step)`:
   - `group` (a collapsible section), `order`, `header` (a caption above the field);
   - `unit` (`m`, `deg`, `s`, `m/s`, free text) shown after the value;
   - `kind`: `color` (a `Vec3`/`Vec4` or `List<double>` as a colour picker), `asset(<kind>)` (`texture`, `material`, `mesh`, `prefab`, `scene`, `audio` — the picker filters by it), `entity` (a reference picker), `multiline`, `angle` (stored radians, edited in degrees), `slider` (when `min` and `max` exist), `layerMask`;
   - `readOnly`, `hidden`, `showIf: "<field>"` / `showIf: "!<field>"` (visibility bound to a sibling boolean — the one conditional 1.0 allows; expressions are not);
   - on the behaviour itself: `@FynkelBehaviour(icon, category, description)` for Add Component and the Hierarchy.
   The same hints apply to native components through the Rust schema attributes (`#[fynkel(...)]`), so `fynkel.Camera` and `game.Player` are shown by one code path.
2. **No custom Inspector widgets, no custom windows, no editor scripts in 1.0.** A field the hints cannot express is shown by the generic widget for its type; nothing a project ships can draw in the editor process. The editor's own panels are the only panels.
3. **The road to code extensions is a plugin SDK after 1.0** (`docs/ideas.md` §23.6): editor plugins compiled *into an editor build* (the way Unity compiles editor assemblies), using the authoring session's commands and never touching source files behind it. It waits for real extension patterns from real projects; a `capabilities: ["editor"]` package manifest is reserved for it.
4. **Runtime-only values** (a behaviour's non-exposed state during Play) are not Inspector fields in 1.0; runtime inspection is a separate, read-only view over the Play channel (§19.2e, built after V0) and never writes to a document except through an explicit "apply to source" command.

## Alternatives considered

- **Custom Inspector widgets as project Dart, loaded by the editor.** Impossible under Flutter AOT without shipping a Dart VM/JIT path in the editor and compiling project code into it — a second execution model, a crash surface inside the authoring process, and a violation of ADR-0016.
- **Per-project editor builds now.** Technically the Unity answer, but it makes every project carry an editor build target, its build times and its breakages, before a single real extension pattern exists.
- **A scripting language for inspectors** (an expression language in the hints, or a mini-UI DSL). Powerful, and a whole language to design, document and secure. `showIf` on a sibling boolean is the deliberate ceiling.
- **Nothing beyond `label`/`min`/`max`.** Cheapest, and it would make every real behaviour's Inspector a flat list of numbers — not the Unity-like editor the product principle asks for.

## Acceptance note (M4 item 4, 2026-09-21)

The hint set landed as decided for behaviours: `@Expose(group, order,
header, unit, kind, readOnly, hidden, showIf)` and
`@FynkelBehaviour(icon, category, description)` in `package:fynkel`,
emitted by `package:fynkel_generator` into `x-fynkel` and
`behaviours.g.json` (an unhinted class emits exactly what it did before;
`kind` is checked against the field's type, `showIf` against a sibling
boolean). One deviation, recorded here: **the native components' hints are
not Rust `#[fynkel(...)]` attributes yet.** `fynkel_schema_derive` and the
component structs in `fynkel_core`/`fynkel_physics`/`fynkel_scene` are the
M3 stream's territory and not on the editor's closed shared-touch list, so
the editor carries a table (`apps/editor/lib/src/inspector/native_hints.dart`,
`type → field → hints` in the same `x-fynkel` vocabulary) that is read
*underneath* whatever a schema's `x-fynkel` says. The Inspector still has
one code path for `fynkel.Camera` and `game.Player`; when the Rust
attributes land, the generated schemas carry the hints, they win, and the
table shrinks to nothing (`inspector_test.dart` fails when the table names
a field no schema has). The `asset(<kind>)` kinds grew `clip`, `skin` and
`rig` (the sub-asset kinds the importers produce) beyond the six listed
above.

## Amendment (2026-09-24, Unity parity pass): object fields and enums

Under the parity pass's standing rule ("default to Unity's concepts";
confirmed by the user 2026-09-25), `@Expose` takes three more field types, so a
script's Inspector has Unity's object field and enum dropdown:

- **A Dart `enum`** (`type: enum`): stored as the value's name, the
  schema's `enum` lists the names, the Inspector draws its dropdown.
- **`GameObject?`** (`type: gameObject`): stored as `{ "entity": id }`.
- **A `@FynkelBehaviour` class, nullable** (`type: behaviour`,
  `x-fynkel.behaviour: <type id>`): stored as `{ "entity": id,
  "component": id }`, that script on that object.

A reference must be nullable: it is None until one is picked and again
when its object is deleted. The Inspector shows `None (Type)`, the
object's name, or `Missing (Type)` for an id no longer in the document
(kept in the file, as Unity keeps it). A Hierarchy row dropped on the
field assigns it (a script field refuses an object without that script);
⊙ lists the candidates; × clears. At runtime the world resolves the
references after every behaviour of a load is attached, against that
load (the native report maps each referenced authoring id to its spawned
handle), so the order scripts attach in does not matter and a prefab
instance's references reach its own copies; prefab expansion derives
the component ids inside component data as it already derived the entity
ids. A target that is gone leaves the field null with the load warning
`FYN-B0003`. The older `kind: 'entity'` hint (a `String` holding a name)
stays for the `lineTo` gizmo and existing data.

## Consequences

- Adding a display hint is one annotation plus regeneration; a project's Inspector is fully described in a committed JSON file that agents can read and write.
- The editor process has one code path per field kind and no plugin loading, so an editor crash is always the editor's own.
- Some things Unity users expect (a button that calls a method, a custom preview) are not possible in 1.0; the Inspector says nothing about them rather than faking them.
- `package:fynkel` (`@Expose`), `package:fynkel_generator` (emission), `fynkel_schema_derive` (the Rust attributes) each gain the hint set — small, additive, shared-crate touches announced per §19.2e.
