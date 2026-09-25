// The game's editor part (ADR-0028): what `fynkel editor` adds to the
// editor for this project. Only files under lib/editor/ may import
// package:fynkel_editor_api or this folder (FYN-P0015), so none of it
// ships in the game.

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import '../game/coin_kind.dart';
import '../generated/assets.g.dart';

void registerEditor(EditorExtensions editor) {
  editor.menuItem(
    'Tools/Code Only/List Coin Kinds',
    id: 'codeOnly.listCoinKinds',
    description: 'Logs what each kind of coin is worth, as the game makes them.',
    run: (context) {
      final kinds = CoinKinds();
      context.log('A coin is worth ${kinds.coin.value}, the big coin ${kinds.big.value} (${kinds.big.size}× the size).');
    },
  );

  // Every coin of the open scene selected (Unity's Selection.objects).
  editor.menuItem(
    'Tools/Code Only/Select Coins',
    id: 'codeOnly.selectCoins',
    description: 'Selects every object with a Collectible in the open scene.',
    enabled: (context) => context.scene != null,
    run: (context) {
      final coins = [
        for (final o in context.scene!.objects)
          if (o.has('game.Collectible')) o,
      ];
      context.select(coins);
      context.log('${coins.length} coins selected.');
    },
  );

  // A coin from the coin prefab, one undo step, as the Hierarchy's own
  // Create would add it: under the selected object, or at the top.
  editor.menuItem(
    'Tools/Code Only/Add Coin',
    id: 'codeOnly.addCoin',
    description: 'Adds a coin (the coin prefab) under the selected object.',
    enabled: (context) => context.scene != null && !context.scene!.isPrefab,
    run: (context) {
      final parent = context.selection.firstOrNull;
      final result = context.edit(
        'Add Coin',
        (edit) => edit.add(
          ObjectSpec.prefab(Assets.prefabs.coin, name: 'Coin', transform: const Transform(position: Vec3(0, 0.5, 0))),
          parent: parent,
        ),
      );
      if (result.ok) context.select(result.added);
    },
  );
}
