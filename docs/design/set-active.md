# SetActive / activeInHierarchy — design note

**Status:** Accepted (2026-09-24; roadmap Phase 1 item 2; Unity's
`GameObject.SetActive` is the reference, per the project's standing
"default to Unity" rule).
**Related:** `docs/documents.md` § Scene documents and § Prefab overrides,
`crates/fynkel_scene` (hierarchy, `propagate`), `crates/fynkel_core/src/world.rs`
(`step`, `extract`), `crates/fynkel_physics`, `packages/fynkel/lib/src/behaviour.dart`
(`settle`), the editor's Inspector header and Hierarchy.

## The problem

A game cannot switch an object off. It can hide a mesh
(`MeshRenderer.visible`), disable a behaviour (`Behaviour.enabled`) or a
volume, but a door that is closed, a pickup already taken, a menu
object not shown yet all need the whole object and its children to stop
existing for the frame — not drawn, not simulated, no behaviours — and
come back later with their state. Destroying and re-instantiating loses
that state and costs a load.

## Decision

### 1. `active` is the entity's own flag

- **Document:** an entity may carry `"active": false`. Absent means
  `true`, so every existing document stays valid and unchanged — no
  `formatVersion` bump, no migration. The writer omits it when true.
- **activeSelf / activeInHierarchy:** an entity is active in the
  hierarchy when it and every ancestor are active. Deactivating a parent
  switches off the subtree without touching the children's own flags;
  reactivating it brings back exactly the children that were active.
- **Prefab instances:** a new override op
  `{ "op": "setActive", "entity": <id>, "value": false }` — the entity
  flag is not a component field, so `setField` cannot carry it. Apply,
  Revert, "Apply to Prefab" and the override listing treat it like a
  `setField`. The instance root's own `active` is the instance entity's
  flag in the scene (no override needed).
- **Authoring command:** `setObjectActive { id, active }` (one undo
  step), next to `renameObject`; `false` writes `"active": false` after
  the name, `true` removes the key. The `instance` query reports each
  expanded entity's `active` and `activeOverridden`.

### 2. What an inactive object does at runtime

Nothing, as in Unity:

- **Not drawn:** no mesh, light, camera, volume or particles from the
  subtree (`World::extract` skips it; the editor's Scene view follows).
- **Not simulated:** its rigid bodies, colliders and characters leave
  the physics world (no contacts, no queries hit them; a body command
  sent to it is dropped, not rejected) and come back **at rest** from
  their transform when reactivated — Unity resets a deactivated body's
  velocity the same way; animators,
  movers (rotate, oscillate, follow, look-at, camera rigs), lifetimes and
  emitters do not tick; audio sources stop their voice (a looping source
  starts again when reactivated, as `playOnAwake`-like sources do).
- **Still there:** it keeps its id, components, transform and children;
  queries by id, `find`, parenting and `destroy` work on it. Its
  transform still propagates, so a reactivated child is where its parent
  says.

### 3. Behaviours (Dart)

- A behaviour is **live** when `enabled && gameObject.activeInHierarchy`.
  `onEnable`/`onDisable` fire on every change of that, in hierarchy
  order (parents first on enable, children first on disable).
- An object created or loaded inactive does **not** run `onCreate` until
  it is first activated (Unity's `Awake`); `start` still runs before the
  first update it gets. `onDestroy` of a behaviour whose `onCreate` never
  ran is not called.
- API: `GameObject.activeSelf`, `GameObject.activeInHierarchy`,
  `GameObject.setActive(bool)` — a command (applied at the tick
  boundary like every write); `activeSelf` reads back the requested value
  at once, `activeInHierarchy` after the tick applies it.

### 4. Runtime shape

- ECS markers in `fynkel_scene`: `InactiveSelf` (the flag) and `Inactive`
  (derived: not active in the hierarchy). The world recomputes `Inactive`
  (`scene::refresh_active`) after the commands of a tick that changed a
  flag or the hierarchy (spawn under a parent, reparent), after a load or
  an instantiate, and after `apply_now` — a DFS from the roots, cheap next
  to `propagate`, and nothing at all while no entity is inactive. Every
  system query takes `without::<Inactive>()`; `World::extract` skips it.
  The editor's preview syncs the flag like a name, and neither the
  preview nor the live channel picks an inactive entity.
- Command `SetActive { entity, active }` (wire kind 13), allowed on the
  simulation lane. The entities query (`QUERY_ENTITIES`) gains an
  `active` byte (activeSelf) so Dart's entity table knows the flag of
  loaded objects; Dart derives `activeInHierarchy` from the table's
  parents. Schema fingerprint changes; no document format changes.

### 5. The editor

- The Inspector header gets Unity's checkbox before the name
  (`inspector.active`): one `setObjectActive` command, one undo step; on
  a prefab instance it records a `setActive` override (bold, revertable).
- The Hierarchy greys an inactive object and its subtree (the name in the
  disabled colour; a child that is itself active but under an inactive
  parent is greyed too, as in Unity).
- The Scene view does not draw inactive objects (picking skips them); the
  selection outline still works from the Hierarchy.
- During Play, the checkbox changes the running game (the live channel)
  like any field.

## Steps

1. **A1 — Format and authoring.** `active` in the scene/prefab schema,
   `fynkel_document` Entity, `setActive` override op (apply, revert,
   listing), `setObjectActive` command, `EntityDescription.active`.
2. **A2 — Runtime.** Markers, recompute, every system filtered,
   physics enable/disable, audio, `SetActive` command + wire + entities
   query. Tests per system.
3. **A3 — Dart API.** `activeSelf`/`activeInHierarchy`/`setActive`,
   behaviour lifecycle (deferred `onCreate`, ordered `onEnable`/
   `onDisable`). Tests in `packages/fynkel`.
4. **A4 — Editor.** Checkbox, greyed Hierarchy, Scene view, overrides,
   live Play.
5. **A5 — Docs.** documents.md, the Dart API docs, editor.md, template
   `AGENTS.md`, status.
