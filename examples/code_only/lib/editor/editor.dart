// The game's editor part (ADR-0028): what `fynkel editor` adds to the
// editor for this project. Only files under lib/editor/ may import
// package:fynkel_editor_api or this folder (FYN-P0015), so none of it
// ships in the game.

import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import '../game/coin_kind.dart';

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
}
