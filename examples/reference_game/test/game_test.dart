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
    expect(game.report.animation.clips, 1, reason: 'idle from the scene');
    expect(game.report.animation.skins, greaterThanOrEqualTo(1));
    game.frame(1 / 60);
    final player = game.world.behaviours.whereType<Player>().single;
    final me = player.gameObject;
    final character = me.get<Animator>()!.clip.asset;

    // Rig-agnostic (M3-19a): the committed box character authors its
    // joints under the Player; `tools/playground.py --hero <glb>` instances
    // a downloaded one under a `Rig` child instead. Either way a skinned
    // body sits somewhere below the Player and the clips pose the joints
    // — every entity under the Player is watched, not a named thigh.
    Iterable<GameObject> descendants(GameObject o) sync* {
      for (final c in o.children) {
        final child = game.world.attach(c);
        yield child;
        yield* descendants(child);
      }
    }

    final joints = descendants(me).toList();
    final bodies = joints.where((o) => o.has<Skin>()).toList();
    expect(bodies, isNotEmpty, reason: 'a skinned body under the Player');
    final boxRig = me.findChild('Rig') == null;
    if (boxRig) {
      expect(me.findChild('Body')!.get<Skin>()!.skeleton, 'hips');
      expect(me.findChild('hips/spine/head/Visor'), isNotNull, reason: 'the visor rides the head joint');
    }
    List<Quat> pose() => [for (final j in joints) j.localTransform.rotation];
    double maxChange(List<Quat> a, List<Quat> b) {
      var m = 0.0;
      for (var i = 0; i < a.length; i++) {
        final d = (a[i].x - b[i].x).abs() + (a[i].y - b[i].y).abs() + (a[i].z - b[i].z).abs() + (a[i].w - b[i].w).abs();
        if (d > m) m = d;
      }
      return m;
    }

    // Standing: idle (the box character's legs straight).
    var s = me.animation.state!;
    expect(s.clip, AssetRef(character, sub: 'anim/0'));
    expect((s.playing, s.looping), (true, true));
    expect(s.duration, greaterThan(0));
    final idle = pose();
    if (boxRig) {
      expect(s.duration, closeTo(2, 1e-6));
      expect(me.findChild('hips/thigh_l')!.localTransform.rotation.x, closeTo(0, 1e-6));
    }

    // Walking: the clip switches with a blend and the joints swing (the
    // box walk's thigh peaks at ±30° = sin 15° about X).
    game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'W')]);
    s = me.animation.state!;
    expect(s.clip, AssetRef(character, sub: 'anim/1'));
    expect(s.isFading, isTrue);
    for (var i = 0; i < 30; i++) {
      game.frame(1 / 60);
    }
    expect(me.animation.state!.isFading, isFalse);
    expect(maxChange(idle, pose()), greaterThan(0.1), reason: 'mid-stride: some joint left its idle pose');
    if (boxRig) {
      expect(me.findChild('hips/thigh_l')!.localTransform.rotation.x.abs(), greaterThan(0.1), reason: 'mid-stride');
    }
    expect(player.transform.position.z, lessThan(-1), reason: 'the controller moved, not the clip');

    // Sprinting: run.
    game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'ShiftLeft')]);
    expect(me.animation.state!.clip, AssetRef(character, sub: 'anim/2'));
    if (boxRig) expect(me.animation.state!.duration, closeTo(0.6, 1e-6));
    game.frame(1 / 60, input: const [InputEvent.up(InputDevice.keyboard, 'ShiftLeft'), InputEvent.up(InputDevice.keyboard, 'W')]);
    expect(me.animation.state!.clip, AssetRef(character, sub: 'anim/0'), reason: 'released: back to idle');

    // A jump: the one-shot is held in the air (the box character's 0.8 s
    // clip ends before the landing and holds its last frame; a longer
    // downloaded jump is still playing when the feet touch), and idle
    // returns on landing.
    game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'Space')]);
    game.frame(1 / 60, input: const [InputEvent.up(InputDevice.keyboard, 'Space')]);
    s = me.animation.state!;
    expect(s.clip, AssetRef(character, sub: 'anim/3'));
    expect(s.looping, isFalse);
    expect(player.jumps, 1);
    var airborne = 0;
    while (!player.characterState()!.grounded && airborne < 180) {
      expect(me.animation.state!.clip, AssetRef(character, sub: 'anim/3'), reason: 'held until grounded');
      game.frame(1 / 60);
      airborne++;
    }
    expect(airborne, inInclusiveRange(30, 179), reason: 'a 5.5 m/s jump is in the air about a second');
    if (boxRig) {
      expect(me.animation.state!.playing, isFalse, reason: 'the 0.8 s clip ended before the landing');
    }
    for (var i = 0; i < 5; i++) {
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
