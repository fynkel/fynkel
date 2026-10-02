import 'dart:io';

import 'package:fynkel/authoring.dart';
import 'package:fynkel/testing.dart';
import 'package:fynkel/fynkel.dart';
import 'package:registered_gameplay/combat.dart';
import 'package:registered_gameplay/generated/behaviours.g.dart';
import 'package:test/test.dart';

void main() {
  test(
    'generated fields follow class, rightmost mixin and superclass precedence',
    () {
      expect(gameHealthType.fields.map((f) => f.name), [
        'hp',
        'invulnerable',
        'target',
      ]);
      expect(gameHealthType.fields.map((f) => f.defaultValue), [
        100.0,
        false,
        null,
      ]);
      final world = World.create();
      addTearDown(world.dispose);
      registerComponents(world);
      final owner = world.spawn()..addData(gameHealthType);
      expect(owner.readData(gameHealthType)!.hp, 100);
      expect(owner.readData(gameHealthType)!.invulnerable, isFalse);
    },
  );
  test('generated component object access and bulk system share storage', () {
    final world = World.create();
    addTearDown(world.dispose);
    registerCombat(world);
    final player = world.build(
      ObjectSpec(
        'Player',
        data: [ComponentData(gameHealthType, Health()..hp = 10)],
      ),
    );
    final door = world.spawn(name: 'Locked');
    door.addBehaviour(Door()..player = player);
    world.frame(world.fixedDt);
    expect(player.readData(gameHealthType)!.hp, 0);
    expect(door.name, 'Unlocked');
  });
  test('authored scene and prefab round trip generated data and forward references', () {
    final project = Directory.systemTemp.createTempSync('fynkel-f3-authored-');
    addTearDown(() => project.deleteSync(recursive: true));
    final fixture = Directory('../../crates/fynkel_document/fixtures/project')
        .absolute
        .path;
    final copied = Process.runSync('cp', ['-R', '$fixture/.', project.path]);
    expect(copied.exitCode, 0);
    final schemas = Directory('${project.path}/schemas/registered')
      ..createSync(recursive: true);
    File('schemas/registered/game.Health.schema.json')
        .copySync('${schemas.path}/game.Health.schema.json');
    final writer = ProjectWriter(project.path);
    final player = ObjectSpec(
      'Player',
      data: [ComponentData(gameHealthType, Health()..hp = 20)],
    );
    final ally = ObjectSpec(
      'Ally',
      data: [ComponentData(gameHealthType, Health()..target = player.object)],
    );
    final actorData = Health()..hp = 7;
    final actor = ObjectSpec(
      'Actor',
      data: [ComponentData(gameHealthType, actorData)],
    );
    actorData.target = actor.object;
    final prefab = writer.writePrefab('prefabs/health', actor);
    final scene = writer.writeScene(
      'scenes/health',
      SceneSpec('Health', [player, ally, ObjectSpec.prefab(prefab.ref)]),
    );
    final cache = cookProject(project.path);
    final world = World.create();
    addTearDown(world.dispose);
    registerComponents(world);
    world.loadScene(scene.ref.asset, cacheDir: cache);
    final livePlayer = world.find('Player')!, liveAlly = world.find('Ally')!;
    expect(livePlayer.readData(gameHealthType)!.hp, 20);
    expect(liveAlly.readData(gameHealthType)!.target, same(livePlayer));
    final liveActor = world.find('Actor')!;
    expect(liveActor.readData(gameHealthType)!.target, same(liveActor));
    final spawned = world.instantiate(prefab.ref);
    expect(spawned.readData(gameHealthType)!.target, same(spawned));
    final missing = World.create();
    addTearDown(missing.dispose);
    final kept = missing.spawn(name: 'kept');
    expect(
      () => missing.loadScene(scene.ref.asset, cacheDir: cache),
      throwsA(isA<FynkelLoadError>()),
    );
    expect(kept.isAlive, isTrue);
  });
}
