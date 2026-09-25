// The game's editor part (ADR-0028): what `fynkel editor` adds to the
// editor for this project. Only files under lib/editor/ may import
// package:fynkel_editor_api or this folder (FYN-P0015), so none of it
// ships in the game.

import 'dart:math' show cos, pi, pow, sin;

import 'package:flutter/material.dart' hide Transform;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import 'package:fynkel_flutter/fynkel_flutter.dart' show ModelBuilder;

import '../game/coin_kind.dart';
import '../game/collectible.dart';
import '../game/game_state.dart';
import '../game/player.dart';
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

  // Unity's CustomEditor: a coin kind's page shows the coin, in its colour
  // and at its size, above the default fields.
  editor.inspector<CoinKind>(
    (context, inspector) => Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        CoinPreview(color: inspector.value('color'), size: inspector.value('size'), value: inspector.value('value')),
        inspector.defaultInspector(),
      ],
    ),
  );

  // Unity's PropertyDrawer, by name: the player's jump speed as its
  // standard field, and how high that jump reaches.
  editor.namedDrawer(
    'jumpHeight',
    (context, field) => Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        field.defaultEditor(),
        Padding(
          padding: const EdgeInsets.only(bottom: 4),
          child: Text(
            JumpHeight.describe(field.isMixed ? null : field.value),
            key: const Key('jumpHeight.text'),
            textAlign: TextAlign.end,
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ),
      ],
    ),
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

/// A coin kind as it looks: a disc of its colour (the document's linear
/// RGB, shown in sRGB), [size] times a coin's, with what it is worth.
class CoinPreview extends StatelessWidget {
  const CoinPreview({super.key, required this.color, required this.size, required this.value});

  /// As the document holds them: `[r, g, b]`, a number, an integer.
  final Object? color;
  final Object? size;
  final Object? value;

  /// A coin of size 1, in logical pixels.
  static const base = 28.0;

  static double _srgb(num linear) {
    final c = linear.clamp(0, 1).toDouble();
    return c <= 0.0031308 ? c * 12.92 : 1.055 * pow(c, 1 / 2.4) - 0.055;
  }

  Color get swatch {
    final c = color;
    final rgb = c is List && c.length >= 3 ? [for (final x in c.take(3)) x is num ? x : 0] : const [1, 1, 1];
    return Color.from(alpha: 1, red: _srgb(rgb[0]), green: _srgb(rgb[1]), blue: _srgb(rgb[2]));
  }

  double get diameter => base * (size is num ? (size as num).toDouble() : 1).clamp(0.1, 3);

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      height: base * 3 + 16,
      child: Row(
        children: [
          SizedBox(
            width: base * 3,
            child: Center(
              child: Container(
                key: const Key('coinPreview.coin'),
                width: diameter,
                height: diameter,
                decoration: BoxDecoration(
                  color: swatch,
                  shape: BoxShape.circle,
                  border: Border.all(color: Colors.black26),
                ),
              ),
            ),
          ),
          const SizedBox(width: 12),
          Expanded(child: Text('Worth ${value ?? 1}', style: Theme.of(context).textTheme.titleSmall)),
        ],
      ),
    );
  }
}

/// How high a jump at a speed reaches: v² / 2g, at the default gravity.
abstract final class JumpHeight {
  static const gravity = 9.81;

  static String describe(Object? speed) => speed is num
      ? 'Reaches ${(speed * speed / (2 * gravity)).toStringAsFixed(2)} m'
      : 'Reaches — (the players differ)';
}

/// The Level Tools window: how many coins, how far out, and a button that
/// puts the ring under the selected object (or at the top) as one undo
/// step — the coins are the coin prefab, as `ObjectSpec`s. During Play it
/// works on the running game too ([PlayTools]).
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
        // During Play the running game's tools come first: the window is
        // often a short dock tab.
        if (widget.editor.play case final play?) ...[
          PlayTools(play: play),
          const Divider(height: 32),
        ],
        Text('Ring of $_count coins'),
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

/// Level Tools during Play: the running game's coins, and buttons that
/// collect them all or send the player back — the game's own methods on
/// its world, gone when Play stops (Live Play mode).
class PlayTools extends StatelessWidget {
  const PlayTools({super.key, required this.play});

  final EditorPlay play;

  /// Takes every coin still in the level, as the player touching it would.
  static int collectAll(World world) {
    final coins = [
      for (final c in world.findObjectsOfType<Collectible>())
        if (!c.collected) c,
    ];
    for (final c in coins) {
      c.collect();
    }
    return coins.length;
  }

  /// Back to the spawn point (a death, as the sweeper's).
  static void respawn(World world) => world.findObjectsOfType<Player>().firstOrNull?.die();

  @override
  Widget build(BuildContext context) {
    final world = play.world;
    final state = world.context.serviceOrNull<GameState>();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text('Playing', style: Theme.of(context).textTheme.titleSmall),
        if (state != null)
          ModelBuilder<int>(
            value: state.collected,
            builder: (context, collected, _) => ModelBuilder<int>(
              value: state.total,
              builder: (context, total, _) => Text('Coins: $collected of $total', key: const Key('playTools.coins')),
            ),
          ),
        const SizedBox(height: 8),
        FilledButton(
          key: const Key('playTools.collectAll'),
          onPressed: () => collectAll(play.world),
          child: const Text('Collect All Coins'),
        ),
        const SizedBox(height: 4),
        OutlinedButton(
          key: const Key('playTools.respawn'),
          onPressed: () => respawn(play.world),
          child: const Text('Respawn Player'),
        ),
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
