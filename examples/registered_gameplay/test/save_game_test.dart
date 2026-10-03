import 'dart:convert';

import 'package:fynkel/gameplay.dart';
import 'package:registered_gameplay/combat.dart';
import 'package:registered_gameplay/generated/behaviours.g.dart';
import 'package:registered_gameplay/save_game.dart';
import 'package:test/test.dart';

void main() {
  World makeWorld() {
    final world = World.create();
    registerCombat(world);
    addTearDown(world.dispose);
    return world;
  }

  test(
    'a new session restores saved data, forward/self references and gameplay',
    () {
      final old = makeWorld();
      final player = old.spawn(name: 'Same name');
      final ally = old.spawn(name: 'Same name');
      player.writeData(
        gameHealthType,
        Health()
          ..hp = 40
          ..target = player,
      );
      ally.writeData(
        gameHealthType,
        Health()
          ..hp = 60
          ..target = player
          ..invulnerable = true,
      );
      final saved = saveCombat({'ally': ally, 'player': player});
      final next = makeWorld();
      final restored = restoreCombat(next, saved);
      expect(restored['player']!.entity, isNot(player.entity));
      expect(
        restored['ally']!.readData(gameHealthType)!.target,
        same(restored['player']),
      );
      expect(
        restored['player']!.readData(gameHealthType)!.target,
        same(restored['player']),
      );
      next.frame(next.fixedDt);
      expect(restored['player']!.readData(gameHealthType)!.hp, 30);
      expect(restored['ally']!.readData(gameHealthType)!.hp, 60);
      expect(player.readData(gameHealthType)!.hp, 40);
      expect(saveCombat(restored), isNot(contains(player.entity.toString())));
    },
  );

  test('invalid versions, duplicate IDs, bad fields and missing references never publish', () {
    final world = makeWorld();
    final row = {
      'id': 'one',
      'name': 'never',
      'hp': 1,
      'invulnerable': false,
      'target': null,
    };
    final good = {
      'format': 'fynkel.combat-save',
      'version': 1,
      'objects': [row],
    };
    for (final bad in [
      {...good, 'version': 2},
      {...good, 'version': 1.0},
      {
        ...good,
        'objects': [row, row],
      },
      {
        ...good,
        'objects': [
          row,
          {...row, 'id': 'two', 'hp': 'bad'},
        ],
      },
      {
        ...good,
        'objects': [
          {...row, 'target': 'absent'},
        ],
      },
      {
        ...good,
        'objects': [
          {...row, 'extra': 1},
        ],
      },
    ]) {
      expect(
        () => restoreCombat(world, jsonEncode(bad)),
        throwsFormatException,
      );
      expect(world.find('never'), isNull);
    }
  });

  test('engine validation rolls back restore and external references cannot be saved', () {
    final source = makeWorld();
    final player = source.spawn(name: 'never');
    final target = source.spawn();
    player.writeData(gameHealthType, Health()..target = target);
    expect(() => saveCombat({'player': player}), throwsStateError);
    player.writeData(gameHealthType, Health());
    final saved = saveCombat({'player': player});
    final unregistered = World.create();
    addTearDown(unregistered.dispose);
    expect(() => restoreCombat(unregistered, saved), throwsStateError);
    expect(unregistered.find('never'), isNull);
  });
}
