# ADR-0020: Tags and layers — several tags per object, one engine-wide layer per object

**Status:** Accepted (2026-09-23, the user's call: "ok let's go", on several tags per object and one layer per object used by physics, rendering and queries)  
**Date:** 2026-09-23  
**Related:** ADR-0006 (document format and migrations), ADR-0012 (codegen: the Rust component is the source of truth), ADR-0004 (wire format and fingerprint), ADR-0018 (Inspector hints), blueprint §19.2d item 8 (physics layers), `docs/physics.md`, `docs/editor.md`

## Context

Every engine a Fynkel user comes from has two cheap ways to say what an object *is*, without writing a component for it:

- **tags** — "this is the player", "this is a pickup": looked up by gameplay (`FindWithTag`, `other.CompareTag("Player")` in a trigger);
- **layers** — "this is on the Ground layer": used by physics (what collides with what), raycasts (what a ray may hit), cameras (what they draw) and lights (what they light).

Fynkel has neither as a property of the object:

- **No tags.** Gameplay finds objects by name (`world.find('Gate')`) or by probing for a component. A name is an identity, not a category, and a marker component per category is code for what should be data.
- **Layers exist only inside physics.** `fynkel.project.json` has `physics.layers` (up to 32 names). `fynkel.Collider` and `fynkel.CharacterController` each carry their own `layer` (membership bits) and `mask` (filter bits); `fynkel.TriggerZone` a `mask`; `world.raycast(mask:)` filters by those bits. Nothing outside physics can use them: a camera cannot skip a layer, a light cannot light only some objects, and an object's "layer" is whatever its collider says — or nothing, if it has none.

This is a document-format change (a new property on every object, a migration, a new fingerprint). It is cheaper now, before public release, than after. It also has to land before the package architecture: when physics becomes a package, layers must belong to the engine rather than to physics.

## Decision

1. **The project declares both lists** in `fynkel.project.json`:
   - `"layers": ["default", …]` — at most **32** names, index `i` is bit `i`. Index 0 is always `"default"`. This **replaces `physics.layers`**: the project migration moves the list unchanged, so every existing bit keeps its meaning.
   - `"tags": ["player", "pickup", …]` — at most **32** unique names. The cap keeps a tag set one 32-bit word, like a layer mask: a document number is only exact to 2⁵³ and a web client cannot hold 64 bits in a number. It is a documented limit, revisited (a second word) if real projects hit it.
   Validation rejects duplicates, empty names, too many entries, and — in scenes and prefabs — a layer or tag the project does not declare.

2. **Every object has a layer and a set of tags, stored as optional components.**
   - `fynkel.Layer { layer: u32 }` — the layer's index. Absent means layer 0 (`default`).
   - `fynkel.Tags { bits: u32 }` — bit `i` is the project's `tags[i]`. Absent means no tags.
   They are ordinary authored components (new ids), so everything components already have comes for free: codegen, the wire, prefab overrides (`setField`), the authoring service's transactions and undo, `fynkel validate`. In a document they are stored by index, like the collider bits they replace. The Inspector never shows the numbers: it draws them in the **object header** — a tag field with chips from the project's list, and a layer dropdown — not as two component cards. Renaming or reordering an entry in the editor's Tags and Layers settings rewrites the documents that use it, in the same transaction.
   Absent-is-default keeps existing scenes valid without touching them.

3. **Physics uses the object's layer.**
   - `fynkel.Collider` and `fynkel.CharacterController` **lose their `layer` field** (scene and prefab `formatVersion` 4, the precedent of `camera-exposure` and `animator-layers`): a collider's membership is `1 << layer` of its entity. `mask` stays on the collider, a per-collider filter as today.
   - `fynkel.TriggerZone`'s `mask` is unchanged.
   - Changing an object's layer at run time updates its colliders' collision groups in the same tick.
   - The **migration** (`scene/3-4/object-layers`) turns a collider's `layer` bits into its entity's `fynkel.Layer`, with an id derived from the entity's so a re-run gives the same document. Layer 0 needs no component. Every collider in the repository today has a single bit; if an entity's colliders disagree, or a collider has several bits, the lowest bit wins (the step's description says so; a migration is a pure rewrite and reports nothing else). The same applies to the `data` of an `addComponent` override; a `setField` of a collider's `layer` has no equivalent and fails the migration with its pointer.
   - A project-wide **collision matrix** (Unity's *Layer Collision Matrix*) is **not** part of this decision. Per-collider masks already express it, per object. A matrix can be added later as the default for colliders that leave `mask` at all bits.

4. **Rendering uses the object's layer.**
   - `fynkel.Camera` gains `culling_mask: u32` (default all bits). An object whose layer bit is clear is neither drawn nor casts shadows in that camera's frame.
   - `fynkel.PointLight` and `fynkel.SpotLight` gain `culling_mask: u32` (default all). The CPU light assignment (E1's eight per object) skips objects outside the light's mask. It costs nothing on the GPU.
   - `fynkel.DirectionalLight`'s mask is deferred: it needs a per-object bit in the shader and in the cascade pass.

5. **The Dart API speaks names.** `package:fynkel`:
   - `go.tags` (a `Set<String>`), `go.hasTag('pickup')`, `go.addTag(…)`, `go.removeTag(…)`;
   - `go.layer` (a `String`, settable);
   - `world.findWithTag('player')` and `world.findAllWithTag('pickup')`;
   - `world.layerMask(['ground', 'props'])` reading the project's `layers`.
   Lookups use the per-tick entity table `world.find` already uses: `EntityInfoRecord` gains `layer` and `tags`, so a tag lookup costs no extra FFI call. An unknown name throws, naming the project's list. Writes are ordinary component commands on the simulation lane.

6. **The editor.**
   - Tag chips and the layer dropdown in the Inspector's object header. Several selected objects show mixed values.
   - A **Tags and Layers** page in project settings: add, rename, reorder and remove entries, with the document rewrite from item 2.
   - The existing `layerMask` field widget reads `layers` instead of `physics.layers`.
   - Layer visibility and lock toggles in the Scene view are a later step.

## Alternatives considered

- **One tag per object (Unity).** Unity users routinely work around it with marker components or name conventions. Unreal's actor tags and Godot's groups allow several. A set costs one word.
- **Tags as free strings, no project list.** A typo becomes a silent miss at run time. The declared list gives validation, a dropdown, and a 32-bit set.
- **`layer` and `tags` as entity-level document fields, beside `name`.** Closer to Unity's header, but every prefab override, undo path, validator and wire record would need a new case for them. As components they reuse all of it, and the editor still presents them as header fields.
- **Keep per-collider membership bits and add a separate object layer.** Two meanings of "layer" on one object, which can disagree. One layer per object, as in Unity, Godot 4's render layers aside.
- **Strings in the document instead of indices.** More readable in a diff, but the Rust component (the source of truth, ADR-0012) has no string lists, and collider bits are already indices. The editor's rename rewrite covers the risk that reordering changes meaning.

## Consequences

- New component ids for `fynkel.Layer` (26) and `fynkel.Tags` (27), always allowed by a project's component registry. `fynkel.Collider` and `fynkel.CharacterController` lose `layer`; `fynkel.Camera`, `fynkel.PointLight` and `fynkel.SpotLight` gain `culling_mask`. `EntityInfoRecord` grows. **The wire fingerprint changes.** Diagnostics: `FYN-P0014` (the project's lists), `FYN-H0012` (an undeclared layer or tag).
- Scene and prefab `formatVersion` 4 (collider `layer` → `fynkel.Layer`, `culling_mask` defaults) and project `formatVersion` 2 (`physics.layers` → `layers`).
- `package:fynkel` gains the tag and layer API. Existing code keeps working through `world.layerMask`, which now reads `layers`.
- The work, each step its own commit with its own tests:
  - **L1** the format: the project lists, the two components, the new and removed fields, the migrations, validation, and physics membership from the layer (the collider's own bits are gone, so physics moves with them);
  - **L2** the Dart API: `EntityInfoRecord`, names, lookups and writes;
  - **L3** renderer: camera and light culling masks;
  - **L4** editor: object header, Tags and Layers settings, rename rewrite;
  - **L5** the reference game: its pickups and player found by tag, its triggers by layer — the end-to-end proof.
