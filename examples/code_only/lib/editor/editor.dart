// The game's editor part (ADR-0028): what `fynkel editor` adds to the
// editor for this project. Only files under lib/editor/ may import
// package:fynkel_editor_api or this folder (FYN-P0015), so none of it
// ships in the game.

import 'dart:math' show cos, pi, sin;

import 'package:flutter/material.dart' hide Transform;
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

  // Unity's EditorWindow: a docked panel that places a ring of coins.
  editor.window(
    'codeOnly.levelTools',
    title: 'Level Tools',
    menu: 'Tools/Code Only/Level Tools',
    build: (context, editor) => LevelTools(editor: editor),
  );

  // A right click on coins in the Hierarchy: they become big coins (the
  // big coin data asset in their Collectible), one undo step.
  editor.hierarchyMenuItem(
    'Make Big Coin',
    id: 'codeOnly.makeBigCoin',
    withComponent: 'game.Collectible',
    run: (context, coins) => context.edit('Make Big Coin', (edit) {
      for (final coin in coins) {
        edit.setField(coin, 'game.Collectible', 'kind', Assets.data.bigCoin);
      }
    }),
  );
}

/// The Level Tools window: how many coins, how far out, and a button that
/// puts the ring under the selected object (or at the top) as one undo
/// step — the coins are the coin prefab, as `ObjectSpec`s.
class LevelTools extends StatefulWidget {
  const LevelTools({super.key, required this.editor});

  final EditorContext editor;

  @override
  State<LevelTools> createState() => _LevelToolsState();
}

class _LevelToolsState extends State<LevelTools> {
  var _count = 8;
  var _radius = 3.0;

  void _place() {
    final editor = widget.editor;
    final result = editor.edit(
      'Place Coin Ring',
      (edit) => edit.add(LevelToolsRing.of(_count, _radius), parent: editor.selection.firstOrNull),
    );
    if (result.ok) editor.select(result.added);
  }

  @override
  Widget build(BuildContext context) {
    final scene = widget.editor.scene;
    final under = widget.editor.selection.firstOrNull;
    return ListView(
      padding: const EdgeInsets.all(12),
      children: [
        Text('Coins: $_count'),
        Slider(
          value: _count.toDouble(),
          min: 3,
          max: 24,
          divisions: 21,
          onChanged: (v) => setState(() => _count = v.round()),
        ),
        Text('Radius: ${_radius.toStringAsFixed(1)} m'),
        Slider(value: _radius, min: 1, max: 10, onChanged: (v) => setState(() => _radius = v)),
        const SizedBox(height: 8),
        FilledButton(
          key: const Key('levelTools.place'),
          onPressed: scene == null || scene.isPrefab ? null : _place,
          child: Text(under == null ? 'Place Ring' : 'Place Ring under ${under.name}'),
        ),
        if (scene == null) const Padding(padding: EdgeInsets.only(top: 8), child: Text('Open a scene first.')),
      ],
    );
  }
}

/// The ring Level Tools places: an empty object with [count] coins (the
/// coin prefab) [radius] metres around its centre.
abstract final class LevelToolsRing {
  static ObjectSpec of(int count, double radius) => ObjectSpec(
        'Coin Ring',
        children: [
          for (var i = 0; i < count; i++)
            ObjectSpec.prefab(
              Assets.prefabs.coin,
              name: 'Coin ${i + 1}',
              transform: Transform(
                position: Vec3(radius * cos(2 * pi * i / count), 0.5, radius * sin(2 * pi * i / count)),
              ),
            ),
        ],
      );
}
