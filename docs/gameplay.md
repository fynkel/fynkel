# One gameplay workflow

Import `package:fynkel/gameplay.dart` for ordinary game code. Host/native tools
use `package:fynkel/advanced.dart`, which includes the gameplay API. F8 removes
the old `fynkel.dart` umbrella; migrate imports and regenerate project bindings.
Both supported surfaces export the same World and types, never separate stores.

## Objects and data

`World` owns the game. `GameObject` is a convenient handle, `EntityRef` is its
world-scoped identity, `Pose` is a transform value, and `Transform` is the live
object accessor. References target objects/components, not display names.
Use `get<T>()`, `set(value)`, `remove<T>()`, and registered `readData`/`writeData`
for component data. Behaviours handle local interaction; registered systems and
bulk queries handle populations. Both read and write the same gameplay state.

Ordinary reads see changes made in the current callback/edit. `world.edit`
groups changes atomically; an invalid edit cannot publish its earlier changes.
Do not catch an invalid engine edit and expect the rest of that edit to commit.
Completed physics results and presentation snapshots describe their named
completed phase, not an imagined second mutable gameplay world. `update` and
`lateUpdate` do not create a transient transform override layer.

## Reusable recipes

Use an `ObjectRecipe` factory for repeated creation:

```dart
final enemy = ObjectRecipe(() {
  final target = ObjectSpec('Target');
  return ObjectSpec('Enemy', children: [
    ObjectSpec('Follower', components: [Follow(target: target.object)]),
    target,
  ]);
});
final first = world.build(enemy.create());
final second = world.build(enemy.create());
```

Create spec nodes and behaviour instances inside the factory. Each call returns
fresh instances and internal references. The recipe rejects reused nodes,
reused behaviours and cycles. Do not use canonical `const ObjectSpec` nodes
inside a reusable factory. Immutable component values can be shared. A captured
live GameObject is an intentional external reference; build validation checks
its world and liveness. Runtime fields belong to fresh behaviour instances.

`world.buildAll(specs)` builds several roots atomically and resolves references
between them before attaching components. It creates no hidden parent. Single
`build` calls delegate to the same implementation.

`ObjectSpec` remains the single-use construction description accepted by
`World.build` and `ProjectWriter`. The older ability to rebuild a behaviour-free
spec preserves already captured handles; it does not rebind those handles to
the newest copy. Use recipes for reusable internally linked trees. A plain
factory function returning a fresh tree follows the same contract; ObjectRecipe
adds freshness validation without a second build implementation.

## Code and editor

Pass `recipe.create()` to `ProjectWriter.writePrefab`, or place it in a
`SceneSpec` for `writeScene`. The resulting documents can be edited, cooked,
loaded and instantiated normally. Cloning remaps internal references and makes
fresh registered behaviours. Register exposed behaviour/component schemas using
the generator before loading authored data.

Inspector changes and undo/redo go through authoring transactions. Play component-data field batches use one native transaction, including built-in
and registered components. A backend without atomic batch support refuses the
edit before changing data. Stale authoring revisions are rejected. Structural
and arbitrary Dart-script operations retain the F2 independent-command boundary:
completed commands remain if a later independent command fails. A Dart setter
can perform arbitrary application work, so this is not advertised as rollback-safe
data. Scene document undo remains separate from live execution. Apply to Scene writes supported live values
back through authoring commands and revisions; runtime-only execution state is
not implicitly serialized. Paused Step advances the named simulation phase.
Hot reload preserves compatible execution state; changes requiring a new runtime
use Restart to Apply. Failure must preserve a usable previous state or expose a
recoverable error. Editor packages remain excluded from runtime dependency paths.

## Upgrading a project

Run `fynkel migrate <project>` (preview with `--dry-run`), then regenerate code,
update the lock with `fynkel project lock`, validate, cook and test. The migrator
keeps original documents as `.bak`; asset discovery ignores recognized document
backups without deleting them. Unknown authored fields remain preserved by the
document tooling. User-defined field-type changes require an explicit migration
or regeneration from their owning code; never silently replace user content.

The authoring ABI is 6 for atomic live component batches; runtime ABI 5 and
built-in document versions remain unchanged. The existing ABI/schema checks still reject incompatible libraries
and documents. Follow [ADR-0058](decisions/ADR-0058-unified-gameplay-workflow.md)
for the compatibility policy and the [F7 gate report](foundation-f7-gate-report.md) for tested workflows.
