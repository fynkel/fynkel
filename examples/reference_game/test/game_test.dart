// The Playground plays headlessly: no window, no GPU. `flutter test` runs
// this; `fynkel test --headless --input test/collect_all.input.json` replays
// the same recording on the Rust host without the Dart behaviours (the
// runtime walks the controller from its named actions). The world reads the
// cooked cache only, so the test cooks the project first (`cookProject` runs
// the `fynkel` CLI; a warm cache costs milliseconds).

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:reference_game/game/collectible.dart';
import 'package:reference_game/game/game.dart';
import 'package:reference_game/game/gate.dart';
import 'package:reference_game/game/player.dart';
import 'package:reference_game/game/switch.dart';

void main() {
  final projectDir = Directory.current.path;
  setUpAll(() => cookProject(projectDir));

  test('the Playground loads with its world and behaviours', () {
    final game = Game.load(projectDir);
    addTearDown(game.dispose);

    expect(game.report.ok, isTrue);
    expect(game.report.diagnostics, isEmpty, reason: 'no load warnings expected');
    expect(game.report.sceneName, 'Playground');
    expect(game.report.entityCount, greaterThanOrEqualTo(150));
    // The player, ten coins, the switch and the gate.
    expect(game.report.behavioursAttached, 13);
    expect(game.report.physics.layers, ['default', 'props', 'player', 'trigger']);
    expect(
      game.report.inputActions.map((a) => a.name),
      containsAll(['move', 'look', 'jump', 'sprint', 'interact', 'restart']),
    );

    game.frame(1 / 60); // the behaviours start on the first frame
    expect(game.world.behaviours.whereType<Player>().single.walkSpeed, 4, reason: 'authored in the scene');
    expect(game.world.behaviours.whereType<Collectible>(), hasLength(10));
    expect(game.world.behaviours.whereType<Switch>().single.target, 'Gate');
    expect(game.world.behaviours.whereType<Gate>().single.opened, isFalse);
    expect(game.state.total.value, 10);
    expect(game.state.collected.value, 0);
    expect(game.world.find('Sweeper')?.get<Oscillate>()?.frequency, 0.25);
  });

  test('the recorded route collects all ten coins', () {
    final game = Game.load(projectDir);
    addTearDown(game.dispose);
    game.frame(1 / 60);
    final player = game.world.behaviours.whereType<Player>().single;
    final gate = game.world.behaviours.whereType<Gate>().single;

    // test/collect_all.input.json (tools/route.py): W to coins 1–2, D over
    // the ramp and platform to coins 3–4, W past the sweeper to coins 5–6,
    // a wait for the launcher's ball, A across the lane (coins 7–8) with a
    // jump onto the block (coin 9), E on the switch, through the gate to
    // coin 10.
    final recording = loadInputRecording('$projectDir/test/collect_all.input.json');
    var bursts = 0;
    var hinted = false;
    var seen = 0;
    final plans = recording.replay(
      game.world,
      update: () {
        if (game.state.collected.value > seen) {
          seen = game.state.collected.value;
          // A coin leaves its sparks behind (the burst prefab, spawned at runtime).
          if (game.world.find('Burst') != null) bursts++;
        }
        if (game.state.hint.value.isNotEmpty) hinted = true;
      },
    );
    expect(plans, hasLength(recording.frames.length));

    expect(game.state.collected.value, 10);
    expect(game.state.won.value, isTrue);
    expect(game.state.deaths.value, 0, reason: 'the route passes the sweeper and the lane unharmed');
    expect(bursts, 10);
    expect(game.world.behaviours.whereType<Collectible>(), isEmpty, reason: 'each coin destroyed its prefab instance');
    expect(hinted, isTrue, reason: 'the switch prompted while the player stood on it');
    expect(gate.opened, isTrue);
    expect(player.jumps, 1);
    expect(player.transform.position.x, lessThan(-16), reason: 'through the gate to the last coin');
    expect(game.state.elapsedTenths.value, greaterThan(200));
    expect(game.world.isPoisoned, isFalse);
  });

  test('the character is posed from the controller: idle, walk, run, jump', () {
    final game = Game.load(projectDir);
    addTearDown(game.dispose);
    expect((game.report.animation.clips, game.report.animation.skins), (1, 1), reason: 'idle from the scene; one skin');
    game.frame(1 / 60);
    final player = game.world.behaviours.whereType<Player>().single;
    final me = player.gameObject;
    final character = me.get<Animator>()!.clip.asset;
    final thigh = me.findChild('hips')!.findChild('thigh_l')!;
    final head = me.findChild('hips')!.findChild('spine')!.findChild('head')!;
    expect(me.findChild('Body')!.get<Skin>()!.skeleton, 'hips');
    expect(head.findChild('Visor'), isNotNull, reason: 'the visor rides the head joint');

    // Standing: idle, the legs straight.
    var s = me.animation.state!;
    expect(s.clip, AssetRef(character, sub: 'anim/0'));
    expect((s.playing, s.looping), (true, true));
    expect(s.duration, closeTo(2, 1e-6));
    expect(thigh.localTransform.rotation.x, closeTo(0, 1e-6));

    // Walking: the clip switches with a blend and the legs swing (the
    // walk's thigh peaks at ±30° = sin 15° about X).
    game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'W')]);
    s = me.animation.state!;
    expect(s.clip, AssetRef(character, sub: 'anim/1'));
    expect(s.isFading, isTrue);
    for (var i = 0; i < 30; i++) {
      game.frame(1 / 60);
    }
    expect(me.animation.state!.isFading, isFalse);
    expect(thigh.localTransform.rotation.x.abs(), greaterThan(0.1), reason: 'mid-stride');
    expect(player.transform.position.z, lessThan(-1), reason: 'the controller moved, not the clip');

    // Sprinting: run.
    game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'ShiftLeft')]);
    expect(me.animation.state!.clip, AssetRef(character, sub: 'anim/2'));
    expect(me.animation.state!.duration, closeTo(0.6, 1e-6));
    game.frame(1 / 60, input: const [InputEvent.up(InputDevice.keyboard, 'ShiftLeft'), InputEvent.up(InputDevice.keyboard, 'W')]);
    expect(me.animation.state!.clip, AssetRef(character, sub: 'anim/0'), reason: 'released: back to idle');

    // A jump: the one-shot plays through, is held in the air, and idle
    // returns on landing.
    game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'Space')]);
    game.frame(1 / 60, input: const [InputEvent.up(InputDevice.keyboard, 'Space')]);
    s = me.animation.state!;
    expect(s.clip, AssetRef(character, sub: 'anim/3'));
    expect(s.looping, isFalse);
    expect(player.jumps, 1);
    var ended = false;
    for (var i = 0; i < 90 && !ended; i++) {
      game.frame(1 / 60);
      ended = !me.animation.state!.playing;
    }
    expect(ended, isTrue, reason: 'the 0.8 s clip ended before the landing');
    expect(me.animation.state!.clip, AssetRef(character, sub: 'anim/3'), reason: 'held until grounded');
    for (var i = 0; i < 60; i++) {
      game.frame(1 / 60);
    }
    expect(player.characterState()!.grounded, isTrue);
    expect(me.animation.state!.clip, AssetRef(character, sub: 'anim/0'));
    expect(game.world.isPoisoned, isFalse);
  });

  test('the sweeper is a hazard: a hit respawns the player at the start', () {
    final game = Game.load(projectDir);
    addTearDown(game.dispose);
    game.frame(1 / 60);
    final player = game.world.behaviours.whereType<Player>().single;

    // Straight into the blade's path (a transform write teleports the
    // controller); it sweeps by within its 4 s period.
    player.transform.position = const Vec3(17.5, 0, -21.5);
    var frames = 0;
    while (game.state.deaths.value == 0 && frames < 300) {
      game.frame(1 / 60);
      frames++;
    }
    expect(game.state.deaths.value, 1);
    expect(frames, lessThan(300));
    game.frame(1 / 60);
    final p = player.transform.position;
    expect((p.x.abs() < 0.01) && (p.z.abs() < 0.01), isTrue, reason: 'back at the spawn point, was $p');
    expect(game.state.collected.value, 0);

    // Falling out of the arena is one too.
    player.transform.position = const Vec3(0, -20, 0);
    game.frame(1 / 60);
    game.frame(1 / 60);
    expect(game.state.deaths.value, 2);
  });

  test('restart reloads the Playground and resets the state', () async {
    final game = Game.load(projectDir);
    addTearDown(game.dispose);
    game.frame(1 / 60);

    // Walk to the first two coins, then R.
    game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'W')]);
    for (var i = 0; i < 3 * 60; i++) {
      game.frame(1 / 60);
    }
    expect(game.state.collected.value, 2);
    expect(game.world.behaviours.whereType<Collectible>(), hasLength(8));
    game.frame(
      1 / 60,
      input: const [InputEvent.up(InputDevice.keyboard, 'W'), InputEvent.down(InputDevice.keyboard, 'R')],
    );
    expect(game.scenes.state, SceneState.pending);
    game.frame(1 / 60, input: const [InputEvent.up(InputDevice.keyboard, 'R')]);
    expect(game.scenes.state, SceneState.ready);
    game.frame(1 / 60);

    expect(game.state.collected.value, 0);
    expect(game.state.total.value, 10);
    expect(game.state.won.value, isFalse);
    expect(game.state.deaths.value, 0);
    expect(game.world.behaviours.whereType<Collectible>(), hasLength(10));
    final player = game.world.behaviours.whereType<Player>().single;
    expect(player.transform.position.z, greaterThan(-0.1), reason: 'back at the start');

    // The app's button does the same through `Game.restart`.
    final report = game.restart();
    game.frame(1 / 60);
    expect((await report).ok, isTrue);
  });
}
