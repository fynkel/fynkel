# ADR-0025: Physics materials

**Status:** Accepted — proposed by the assistant 2026-09-25, confirmed by the user 2026-09-25  
**Date:** 2026-09-25  
**Related:** ADR-0012 (schema codegen), R3a's volume profile (`ae03864`, the pattern this follows), `docs/physics.md` § Physics materials, `docs/documents.md` § Physics materials, `docs/unity-parity.md` (Physics Material rows)

## Context

In Unity, a surface's friction and bounciness live in a **Physics Material**
asset. Many colliders share it, so one edit changes them all, and it also
chooses how two surfaces combine (`Average`, `Minimum`, `Multiply`,
`Maximum`). In Fynkel, `friction` and `restitution` were fields of each
`fynkel.Collider`, and the combine rule was always Rapier's default
(average). Ice, rubber or a bouncy pad had to be typed into every
collider, and a surface had no way to say "whatever touches me does not
slide". The parity checklist marked it ❌ and said an asset needed an ADR.

## Decision

1. **A document kind, not a component.** `<name>.physicsmaterial.json`
   (`DocumentKind::PhysicsMaterial`, kind name `physics_material`, the
   hand-written `schemas/physics_material.schema.json`) holds:
   - `friction`, default 0.6, `>= 0`
   - `bounciness`, default 0, `0..=1`
   - `friction_combine`, default `average`
   - `bounce_combine`, default `average`

   The defaults are Unity's default material. Every field is optional,
   and unknown fields are refused. It follows the volume profile: a
   lenient read view (`PhysicsMaterialDocument`, `FYN-D0800` for what the
   schema cannot see), `fynkel document new`, the authoring session's
   `newDocument` and `Editor::open`, the cook's generic JSON chunk (a
   scene depends on it, and its errors fail the scene), and
   `Assets::physics_material` in the runtime store. The lock's
   `documents` gains `physics_material: 1`.
2. **The Collider names it.** `fynkel.Collider` gains `material:
   Option<AssetRef>`. When it is set, the material's values replace the
   collider's own `friction` and `restitution`. Those two fields stay:
   existing scenes keep working without a migration, since an
   `Option<AssetRef>` is not a required field. A collider without a
   material behaves exactly as before and combines with `average`. The
   component grows from 72 to 96 bytes on the wire, and the schema
   fingerprint changes (`0xa6bee88bec527d9b` → `0xc9dc361258a0c148`).
3. **Registered with the physics world, like mesh colliders.**
   `Physics::register_material(key, material)` validates the material and
   keeps it. The loader (`Assets::register_mesh_colliders`) registers
   every material a scene's colliders name before the first step.
   Registering new values under a key changes every live collider that
   names it, in place, and wakes its body, so a reload after an edit
   needs no respawn. A collider whose material is not registered is
   still created with its own values and reports one `INVALID_PAYLOAD`
   diagnostic on `fynkel.Collider`. No new ABI diagnostic code is added.
4. **Unity's combine priority, which is Rapier's.** Unity chooses the
   rule of the two colliders with the higher priority:
   `Average < Minimum < Multiply < Maximum`. Rapier's
   `CoefficientCombineRule` does the same thing. Its variants are ordered
   `Average = 0, Min = 1, Multiply = 2, Max = 3`, and the effective rule
   for a contact pair is `rule1.max(rule2)`. Fynkel's `CombineRule` maps
   one-to-one onto those four variants and hands Rapier the per-collider
   rules. Fynkel never combines anything itself, so the two engines
   cannot disagree. (`CombineRule::combine` computes the same value for
   tests and tools.) Rapier's extra rules (`ClampedSum`, `GeometricMean`)
   are not exposed, because Unity has no equivalent.
5. **One friction coefficient.** Unity has `staticFriction` and
   `dynamicFriction`. Rapier's contact solver has a single Coulomb
   friction coefficient per contact and no separate static value. A
   `static_friction` field would be accepted and then silently ignored,
   so the document has `friction` only. The schema refuses
   `static_friction` and `dynamic_friction` as unknown fields. A Unity
   user's `dynamicFriction` is Fynkel's `friction`.

## Consequences

- Surfaces are shared and edited in one place, and a floor can decide
  how everything slides on it (`minimum` for ice, `maximum` for glue),
  as in Unity.
- Two ways to set a surface. The docs recommend a material, and the
  Collider's own fields remain for quick prototypes and old scenes. The
  Collider's fallback defaults (friction 0.5) differ from the material's
  (0.6), as before, so no existing scene changes.
- Objects cannot stick until pushed harder than they slide, because
  there is no separate static friction. If Rapier gains a separate
  static coefficient, it can be added as an optional field without
  breaking documents.
- The editor has to add the UI: the Collider's `material` picker
  (`asset(physics_material)`), a Project browser kind, a Create menu
  entry, an icon, and the document's Inspector. The preview world does
  not simulate physics, so there is no live-edit override path like the
  volume's `setVolume`.
