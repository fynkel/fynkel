# pickups

The example Fynkel package of ADR-0029 (docs/decisions in the engine): what a
game gets by adding one line to its pubspec. `examples/code_only` depends on it
(`path: ../packages/pickups`) for the gem in its level.

- **A script and a data type.** `Pickup` (`pickups.Pickup`,
  `lib/src/pickup.dart`) takes its kind's colour and, when the object tagged
  `takenBy` (`player`) enters its `pickup` zone, tells the game's
  `PickupSink` (`world.context.install<PickupSink>(…)`), plays the kind's
  sound and goes. `PickupKind` (`pickups.PickupKind`, a data asset) holds
  the points, the colour and the sound. Both ids start with the package's
  namespace, `pickups` (`fynkel.package.json`).
- **Assets.** `assets/` holds the gem material, the gem kind (10 points)
  and the pickup prefab. A game sees them as `Packages/pickups/…`: its
  scenes place the prefab, the cook cooks it, and the Project browser shows
  it under Packages — editable in place for a `path:` dependency,
  read-only (Copy into Project) for a hosted or git one.
- **An editor part.** `lib/editor/editor.dart`: Tools ▸ Pickups ▸ Add
  Pickup, and a Pickup Kind's Inspector page shows the gem above its fields.

```sh
fynkel codegen --project .   # lib/generated/ (behaviours, assets) and schemas/, committed
dart run tool/assets.dart    # rewrite assets/ from lib/src/pickup_spec.dart
flutter test                 # the assets are what the tool writes; the gem preview
```

`tool/assets.dart` writes through `ProjectWriter`, which, in a folder with
`fynkel.package.json`, writes into its `assets` folder with ids of the
package's own (a game's `prefabs/pickup.prefab.json` gets another id) and no
layers or tags but `default`: the game's are not the package's to know, so
the pickup's sensor finds its taker by tag name.

In a game, import the package's `Assets` with a prefix, since the game has
its own: `import 'package:pickups/generated/assets.g.dart' as pickups;`.
