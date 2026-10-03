// The game plays headlessly: no window, no GPU. `flutter test` runs this;
// `fynkel test --headless --scene assets/scenes/playground.scene.json
// --input test/collect_all.input.json` replays the same recording on the
// Rust host without the Dart behaviours (the runtime walks the controller
// from its named actions). The world reads the cooked cache only, so the
// test cooks the project first (`cookProject` runs the `fynkel` CLI; a
// warm cache costs milliseconds). The project starts in the Menu
// (M3-15); [playing] takes a fresh game into the Playground the way the
// Menu's Play button does.

import 'dart:io';
import 'dart:math' as math;

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/gameplay.dart';
import 'package:fynkel/testing.dart';
import 'package:reference_game/game/collectible.dart';
import 'package:reference_game/game/game.dart';
import 'package:reference_game/game/game_state.dart';
import 'package:reference_game/game/gate.dart';
import 'package:reference_game/game/player.dart';
import 'package:reference_game/game/switch.dart';
import 'package:reference_game/generated/animators.g.dart';

/// A game in the Playground, one frame in (the transition the Menu's Play
/// requests lands at the start of that frame and the level's behaviours
/// start on its first tick).
Game playing(String projectDir) {
  final game = Game.load(projectDir);
  game.play();
  game.frame(1 / 60);
  return game;
}

void main() {
  final projectDir = Directory.current.path;
  setUpAll(() => cookProject(projectDir));

  test('the project starts in the Menu; Play loads the Playground with its world and behaviours', () {
    final game = Game.load(projectDir);
    addTearDown(game.dispose);

    expect(game.report.ok, isTrue);
    expect(game.report.diagnostics, isEmpty, reason: 'no load warnings expected');
    expect(game.report.sceneName, 'Menu');
    expect(game.report.behavioursAttached, 0, reason: 'the backdrop has no game logic');
    expect(game.state.screen.value, Screen.menu);
    expect(game.report.layersAndTags.layers, ['default', 'props', 'player', 'trigger']);
    expect(game.report.layersAndTags.tags, ['player', 'pickup']);
    expect(
      game.report.inputActions.map((a) => a.name),
      containsAll(['move', 'look', 'jump', 'sprint', 'interact', 'restart']),
    );
    game.frame(1 / 60);
    expect(game.world.find('Big coin')?.get<Rotator>()?.speed, closeTo(0.8, 1e-6), reason: 'the menu turns on native systems only');
    expect(game.world.find('Camera')?.parent, isNotNull, reason: 'the camera rides its rotating pivot');

    game.play();
    expect(game.scenes.state, SceneState.pending);
    game.frame(1 / 60); // the Playground loads and its behaviours start on the first tick
    expect(game.state.screen.value, Screen.playing);
    final report = game.scenes.lastReport!;
    expect(report.sceneName, 'Playground');
    expect(report.entityCount, greaterThanOrEqualTo(150));
    // The player, ten coins, the switch, the gate and SomethingFun2.
    expect(report.behavioursAttached, 14);
    expect(game.world.behaviours.whereType<Player>().single.walkSpeed, 4, reason: 'authored in the scene');
    expect(game.world.behaviours.whereType<Collectible>(), hasLength(10));
    expect(game.world.behaviours.whereType<Switch>().single.gate, same(game.world.find('Gate')!.behaviour<Gate>()),
        reason: 'a script reference (ADR-0036 §4)');
    expect(game.world.behaviours.whereType<Gate>().single.opened, isFalse);
    expect(game.state.total.value, 10);
    expect(game.state.collected.value, 0);
    expect(game.world.find('Sweeper')?.get<Oscillate>()?.frequency, 0.25);

    // Tags and layers (ADR-0020): the player and the coins found by tag,
    // the zones on the `trigger` layer that only the `player` layer meets.
    final me = game.world.findWithTag(playerTag)!;
    expect(me.behaviours.whereType<Player>(), hasLength(1));
    expect(me.layer, 'player');
    final coins = game.world.findAllWithTag(pickupTag);
    expect(coins, hasLength(10));
    for (final coin in coins) {
      expect(coin.behaviours.whereType<Collectible>(), hasLength(1));
      expect(coin.layer, 'trigger');
      expect(coin.get<Collider>()!.mask, game.world.layerMask(['player']));
    }
    final pad = game.world.find('Switch')!;
    expect(pad.layer, 'trigger');
    expect(pad.hasTag(playerTag), isFalse);
  });

  test('only the player collects: a prop is kept out by the layer, an untagged body on the player layer by the tag', () {
    final game = playing(projectDir);
    addTearDown(game.dispose);
    final coin = game.world.findAllWithTag(pickupTag).first;
    final at = coin.worldTransform!.position;

    GameObject drop(String name, String layer) => game.world.spawn(
          name: name,
          transform: Pose(position: Vec3(at.x, at.y + 2, at.z), rotation: Quat.identity, scale: const Vec3(1, 1, 1)),
        )
          ..add(const RigidBody(
              bodyType: BodyType.dynamic, mass: 1, linearDamping: 0, angularDamping: 0, gravityScale: 1, ccd: false))
          ..add(const Collider(
              shape: ColliderShape.sphere,
              size: Vec3(0.3, 0.3, 0.3),
              offset: Vec3.zero,
              friction: 0.5,
              restitution: 0,
              isSensor: false,
              mask: 0xffffffff))
          ..layer = layer;

    // A ball on `props` falls into the coin: its zone's mask is the player
    // layer only, so nothing happens.
    final ball = drop('Stray ball', 'props');
    for (var i = 0; i < 90; i++) {
      game.frame(1 / 60);
    }
    // Resting on the floor inside the coin's pickup sphere.
    bool inside(GameObject go) => (go.worldTransform!.position.y - at.y).abs() < 0.5;
    expect(inside(ball), isTrue, reason: 'it came to rest inside the zone: ${ball.worldTransform!.position}');
    expect(game.state.collected.value, 0);

    // A body on the `player` layer enters the zone, but it is not the
    // object tagged `player`: the coin stays.
    final impostor = drop('Impostor', 'player');
    for (var i = 0; i < 90; i++) {
      game.frame(1 / 60);
    }
    expect(inside(impostor), isTrue, reason: '${impostor.worldTransform!.position}');
    expect(game.state.collected.value, 0);
    expect(game.world.findAllWithTag(pickupTag), hasLength(10));
  });

  test('the recorded route collects all ten coins', () {
    final game = playing(projectDir);
    addTearDown(game.dispose);
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
    expect(game.world.findAllWithTag(pickupTag), isEmpty, reason: 'no pickup left');
    expect(hinted, isTrue, reason: 'the switch prompted while the player stood on it');
    expect(gate.opened, isTrue);
    expect(player.jumps, 1);
    expect(player.transform.position.x, lessThan(-16), reason: 'through the gate to the last coin');
    expect(game.state.elapsedTenths.value, greaterThan(200));
    expect(game.world.isPoisoned, isFalse);
  });

  test(
    'the character is posed from the controller: the Animator Controller\'s Locomotion and Jump, the wave one-shot, footsteps',
    () {
      final game = playing(projectDir);
      addTearDown(game.dispose);
      expect(game.scenes.lastReport!.animation.skins, greaterThanOrEqualTo(1));
      final player = game.world.behaviours.whereType<Player>().single;
      final me = player.gameObject;

      // Rig-agnostic (M3-19a/b): the committed box character authors its
      // joints under the Player; `tools/playground.py --hero <rig.json>`
      // instances a downloaded one under a `Rig` child instead — the scene
      // tells the Player its wave clip, the controller names the jump
      // clip. Either way a skinned
      // body sits somewhere below the Player and the clips pose the joints
      // — every entity under the Player is watched, not a named thigh.
      Iterable<GameObject> descendants(GameObject o) sync* {
        for (final child in o.children) {
          yield child;
          yield* descendants(child);
        }
      }

      final joints = descendants(me).toList();
      final bodies = joints.where((o) => o.has<Skin>()).toList();
      expect(bodies, isNotEmpty, reason: 'a skinned body under the Player');
      final boxRig = me.findChild('Rig') == null;
      if (boxRig) {
        expect(me.findChild('Body')!.get<Skin>()!.skeleton, same(me.findChild('hips')));
        expect(me.findChild('hips/spine/head/Visor'), isNotNull, reason: 'the visor rides the head joint');
        expect(
          game.scenes.lastReport!.animation.clips,
          11,
          reason: 'the Animator Controller (its blend space, the jump and the Aim tree\'s six strafe clips count once '
              'each, including idle, walk and run prepared with the blend space); the wave registers when it plays',
        );
      }
      List<Quat> pose() => [for (final j in joints) j.localTransform.rotation];
      double maxChange(List<Quat> a, List<Quat> b) {
        var m = 0.0;
        for (var i = 0; i < a.length; i++) {
          final d =
              (a[i].x - b[i].x).abs() + (a[i].y - b[i].y).abs() + (a[i].z - b[i].z).abs() + (a[i].w - b[i].w).abs();
          if (d > m) m = d;
        }
        return m;
      }

      // Standing: the controller starts in Locomotion, whose blend space
      // at speed 0 is the idle (the box character's legs straight, a 2 s
      // lap).
      var s = me.animation.stateOf()!;
      expect(s.name, 'Locomotion');
      expect(s.next, isNull);
      expect(me.animation.getFloat('speed'), 0);
      expect(me.animation.getBool('grounded'), isTrue);
      expect(s.length, greaterThan(0));
      final idle = pose();
      if (boxRig) {
        expect(s.length, closeTo(2, 1e-6));
        expect(me.findChild('hips/thigh_l')!.localTransform.rotation.x, closeTo(0, 1e-6));
      }

      // Walking: the controller reaches 4 m/s, the parameter follows it a
      // tick later and the space sits on the walk (a 1 s lap for the box
      // character, whose thigh peaks at ±30° = sin 15° about X); the
      // footsteps arrive as markers.
      final steps = game.state.footsteps.value;
      game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'W')]);
      for (var i = 0; i < 30; i++) {
        game.frame(1 / 60);
      }
      s = me.animation.stateOf()!;
      expect(s.name, 'Locomotion', reason: 'no state switch: the blend space plays on');
      expect(me.animation.getFloat('speed'), closeTo(4, 1e-3));
      if (boxRig) expect(s.length, closeTo(1, 1e-5));
      expect(maxChange(idle, pose()), greaterThan(0.1), reason: 'mid-stride: some joint left its idle pose');
      if (boxRig) {
        expect(me.findChild('hips/thigh_l')!.localTransform.rotation.x.abs(), greaterThan(0.1), reason: 'mid-stride');
      }
      expect(player.transform.position.z, lessThan(-1), reason: 'the controller moved, not the clip');
      for (var i = 0; i < 60; i++) {
        game.frame(1 / 60);
      }
      expect(game.state.footsteps.value, greaterThan(steps), reason: 'the walk\'s step markers counted');

      // Sprinting: 7 m/s is the run; between the two the lap length is
      // between the two, phase-synced.
      game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'ShiftLeft')]);
      game.frame(1 / 60);
      s = me.animation.stateOf()!;
      expect(me.animation.getFloat('speed'), closeTo(7, 1e-3));
      if (boxRig) expect(s.length, closeTo(0.6, 1e-5));
      game.frame(
        1 / 60,
        input: const [InputEvent.up(InputDevice.keyboard, 'ShiftLeft'), InputEvent.up(InputDevice.keyboard, 'W')],
      );
      game.frame(1 / 60);
      s = me.animation.stateOf()!;
      expect(me.animation.getFloat('speed'), 0, reason: 'released: the stride stops with the player');
      if (boxRig) expect(s.length, closeTo(2, 1e-5));

      // A jump: the Player only sets the `jump` trigger; the controller
      // enters the Airborne sub-state machine (ADR-0033 § 4) at its Jump,
      // holds the jump's last pose in Fall while in the air (the box
      // character's 0.8 s clip ends before the landing; a longer
      // downloaded jump is still playing when the feet touch), lands in
      // Land once `grounded` is true again, and leaves the machine for
      // Locomotion at Land's end. Code names the states by path.
      game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'Space')]);
      game.frame(1 / 60, input: const [InputEvent.up(InputDevice.keyboard, 'Space')]);
      s = me.animation.stateOf()!;
      expect(s.next ?? s.name, 'Airborne/Jump');
      expect(player.jumps, 1);
      var airborne = 0;
      final inAir = <String>{};
      while (!player.characterState()!.grounded && airborne < 180) {
        final now = me.animation.stateOf()!;
        inAir.add(now.next ?? now.name);
        game.frame(1 / 60);
        airborne++;
      }
      expect(airborne, inInclusiveRange(30, 179), reason: 'a 5.5 m/s jump is in the air about a second');
      expect(inAir.difference({'Airborne/Jump', 'Airborne/Fall'}), isEmpty, reason: 'held until grounded');
      if (boxRig) expect(inAir, contains('Airborne/Fall'), reason: 'the 0.8 s clip ended before the landing');
      for (var i = 0; i < 5; i++) {
        game.frame(1 / 60);
      }
      expect(player.characterState()!.grounded, isTrue);
      s = me.animation.stateOf()!;
      expect(s.next ?? s.name, 'Airborne/Land', reason: 'landing');
      expect(player.anim.nextState ?? player.anim.state, PlayerState.airborneLand, reason: 'the same, typed (ADR-0033 § 3)');
      var landing = 0;
      while ((me.animation.stateOf()!.next ?? me.animation.stateOf()!.name) != 'Locomotion' && landing < 180) {
        game.frame(1 / 60);
        landing++;
      }
      expect(me.animation.stateOf()!.next ?? me.animation.stateOf()!.name, 'Locomotion', reason: 'out of the machine at Land\'s end');

      // A new jump cuts a landing short: Fall → Land may be interrupted by
      // Land's transitions (`interruptibleBy: next`), and the fade starts
      // from the pose of that moment.
      String showing() => me.animation.stateOf()!.next ?? me.animation.stateOf()!.name;
      game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'Space')]);
      game.frame(1 / 60, input: const [InputEvent.up(InputDevice.keyboard, 'Space')]);
      var wait = 0;
      while (showing() != 'Airborne/Land' && wait < 240) {
        game.frame(1 / 60);
        wait++;
      }
      expect(showing(), 'Airborne/Land');
      s = me.animation.stateOf()!;
      expect(s.transitionProgress, lessThan(1), reason: 'still fading into the landing');
      game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'Space')]);
      game.frame(1 / 60, input: const [InputEvent.up(InputDevice.keyboard, 'Space')]);
      expect(player.jumps, 3);
      s = me.animation.stateOf()!;
      expect((s.name, s.next), ('Airborne/Land', 'Airborne/Jump'), reason: 'the landing interrupted');
      wait = 0;
      while ((me.animation.stateOf()!.name != 'Locomotion' || me.animation.stateOf()!.next != null) && wait < 400) {
        game.frame(1 / 60);
        wait++;
      }
      expect(me.animation.stateOf()!.name, 'Locomotion');

      // The wave: a one-shot in the controller's UpperBody slot (ADR-0033
      // § 2: a layer with no states, rooted at the spine) — the right arm
      // rises while the legs keep the idle.
      if (boxRig) {
        final arm = me.findChild('hips/spine/upperarm_r')!;
        expect(me.animation.isPlayingOneShot(slot: 'UpperBody'), isFalse);
        expect(me.animation.stateOf(layer: 1)!.slot, 'UpperBody');
        expect(arm.localTransform.rotation.z, inInclusiveRange(-0.07, -0.03), reason: 'at rest: the idle\'s breathing');
        game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'F')]);
        game.frame(1 / 60, input: const [InputEvent.up(InputDevice.keyboard, 'F')]);
        expect(player.waves, 1);
        expect(me.animation.isPlayingOneShot(slot: 'UpperBody'), isTrue);
        for (var i = 0; i < 18; i++) {
          game.frame(1 / 60);
        }
        expect(arm.localTransform.rotation.z, lessThan(-0.5), reason: 'the arm is raised (about Z, -160° at most)');
        expect(me.animation.stateOf(layer: 1)!.oneShot, player.wave);
        expect(me.animation.stateOf()!.name, 'Locomotion', reason: 'the machine plays on underneath');
        for (var i = 0; i < 60; i++) {
          game.frame(1 / 60);
        }
        expect(me.animation.isPlayingOneShot(slot: 'UpperBody'), isFalse, reason: 'the 1.2 s wave ended');
        expect(
          arm.localTransform.rotation.z,
          inInclusiveRange(-0.07, -0.03),
          reason: 'back to what the idle leaves: its -4°…-7° breathing (z = sin of half)',
        );
      }

      // The aim mode (ADR-0033): holding the right mouse button faces the
      // camera's yaw (the controller's turnSpeed 0 meanwhile) and strafes
      // on the Aim state's blend tree — D walks right, and `moveX` is the
      // walk's 4 m/s in the player's own frame.
      final turnSpeed = player.get<CharacterController>()!.turnSpeed;
      game.frame(1 / 60, input: const [InputEvent.down(InputDevice.mouse, 'right')]);
      expect(player.aiming, isTrue);
      expect(player.get<CharacterController>()!.turnSpeed, 0);
      game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'D')]);
      for (var i = 0; i < 30; i++) {
        game.frame(1 / 60);
      }
      s = me.animation.stateOf()!;
      expect((s.name, s.next), ('Aim', null));
      expect(me.animation.getBool('aim'), isTrue);
      expect(me.animation.getFloat('moveX'), closeTo(4, 1e-2));
      expect(me.animation.getFloat('moveY'), closeTo(0, 1e-2));
      final yaw = game.world.find('Camera')!.get<ThirdPersonCamera>()!.yaw;
      final facing = player.transform.rotation.rotate(const Vec3(0, 0, -1));
      expect(facing.x, closeTo(-math.sin(yaw), 1e-3), reason: 'facing where the camera looks');
      expect(facing.z, closeTo(-math.cos(yaw), 1e-3));
      if (boxRig) expect(s.length, closeTo(1, 1e-5), reason: 'the walk ring: walk_right alone');
      game.frame(
        1 / 60,
        input: const [InputEvent.up(InputDevice.keyboard, 'D'), InputEvent.up(InputDevice.mouse, 'right')],
      );
      for (var i = 0; i < 20; i++) {
        game.frame(1 / 60);
      }
      expect(player.aiming, isFalse);
      expect(player.get<CharacterController>()!.turnSpeed, turnSpeed, reason: 'restored');
      expect(me.animation.stateOf()!.name, 'Locomotion');
      expect(game.world.isPoisoned, isFalse);
    },
  );

  test('the sounds: the streamed music from the load, a footstep per marker, the pickup, the hit', () {
    final game = playing(projectDir);
    addTearDown(game.dispose);
    // Each scene names one clip through a source — the music, streamed
    // (its sidecar's `stream: true`, cooked to a STRM chunk): the Menu's
    // load registered it, the Playground's found it registered; the
    // behaviours' one-shots register as they are first played.
    expect(game.report.audio.sounds, 1);
    final report = game.scenes.lastReport!;
    expect(report.audio.sounds, 0, reason: 'the same music, still registered');
    expect(report.audio.streams, 1);
    expect(report.audio.device, isFalse, reason: 'no device in a test: the null backend keeps the clock');
    expect(report.audio.maxVoices, greaterThanOrEqualTo(32));

    final music = game.world.find('Music')!;
    final source = music.get<AudioSource>()!;
    expect(source.bus, AudioBus.music);
    expect(source.looping, isTrue);
    expect(source.playing, isTrue);
    final state = music.audio.state!;
    expect(state.playing, isTrue, reason: 'playing from the load');
    expect(state.duration, closeTo(74.25, 0.01), reason: 'the frame count of the streamed track');
    var stats = game.world.audio.stats;
    expect(stats.streams, 1);
    expect(stats.started, 1);
    expect(stats.voices, 1);

    // Walking straight ahead: every step marker plays its footstep
    // (left/right by name) and the first coin, 5 m out, its pickup.
    final player = game.world.behaviours.whereType<Player>().single;
    expect(player.stepLeft.isNone, isFalse);
    expect(player.stepRight.isNone, isFalse);
    expect(player.hit.isNone, isFalse);
    expect(game.world.behaviours.whereType<Collectible>().first.pickup.isNone, isFalse);
    game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'W')]);
    for (var i = 0; i < 100; i++) {
      game.frame(1 / 60);
    }
    expect(game.state.footsteps.value, greaterThan(2));
    expect(player.sounds, game.state.footsteps.value, reason: 'one one-shot per marker');
    expect(game.state.collected.value, 1);
    stats = game.world.audio.stats;
    expect(stats.started, 1 + player.sounds + 1, reason: 'the music, the footsteps, the pickup');
    expect(stats.refused, 0);
    expect(stats.starved, 0, reason: 'the decoder keeps a second ahead of the clock');
    expect(music.audio.state!.time, greaterThan(1.5), reason: 'the music went on under the effects');

    // A death plays the hit, on top of the music.
    final before = player.sounds;
    player.transform.position = const Vec3(0, -20, 0);
    game.frame(1 / 60);
    game.frame(1 / 60);
    expect(game.state.deaths.value, 1);
    expect(player.sounds, before + 1);
    expect(game.world.audio.stats.started, stats.started + 1);
    expect(music.audio.isPlaying, isTrue);
    expect(game.world.isPoisoned, isFalse);
  });

  test('the sweeper is a hazard: a hit respawns the player at the start', () {
    final game = playing(projectDir);
    addTearDown(game.dispose);
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
    final game = playing(projectDir);
    addTearDown(game.dispose);

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

    // The app's button does the same through `Game.play`.
    final report = game.play();
    game.frame(1 / 60);
    expect((await report).ok, isTrue);
  });

  test(
    'the flow (M3-15): Menu → Playground → the win → Results, with the run\'s numbers → Playground again → Menu',
    () {
      final game = playing(projectDir);
      addTearDown(game.dispose);
      expect(game.state.screen.value, Screen.playing);
      expect(game.state.lastRun.value, isNull);

      // The route wins; the flash stays `winDelay` (1.5 s = 90 ticks), then
      // the Player ends the run: the Results scene replaces the level.
      final recording = loadInputRecording('$projectDir/test/collect_all.input.json');
      var wonAt = -1;
      var frames = 0;
      recording.replay(
        game.world,
        update: () {
          frames++;
          if (wonAt < 0 && game.state.won.value) wonAt = frames;
        },
      );
      expect(game.state.won.value, isTrue);
      expect(wonAt, greaterThan(0));
      var after = frames - wonAt;
      while (game.state.screen.value == Screen.playing && after < 120) {
        game.frame(1 / 60);
        after++;
      }
      expect(game.state.screen.value, Screen.results);
      expect(after, inInclusiveRange(89, 92), reason: 'the results 1.5 s after the last coin, counted in ticks');
      expect(game.scenes.lastReport!.sceneName, 'Results');
      expect(game.world.behaviours, isEmpty, reason: 'the level and its behaviours are gone');
      final run = game.state.lastRun.value!;
      expect(run.won, isTrue);
      expect((run.collected, run.total, run.deaths), (10, 10, 0));
      expect(run.elapsedTenths, greaterThan(200));
      expect(run.footsteps, greaterThan(10));
      expect(run.clock, matches(RegExp(r'^\d\d:\d\d\.\d$')));
      // The scene plays the jingle once from its load, over the music.
      game.frame(1 / 60);
      final jingle = game.world.find('Jingle')!;
      expect(jingle.get<AudioSource>()!.looping, isFalse);
      expect(jingle.audio.state!.playing, isTrue);
      expect(game.world.audio.stats.streams, 1, reason: 'the music again');
      // The menus' click is a clip the cache holds, played outside the ticks.
      final started = game.world.audio.stats.started;
      game.world.audio.play(clickSound, presentation: true);
      expect(game.world.audio.stats.started, started + 1);
      expect(game.world.audio.stats.refused, 0);

      // Play again: a fresh level, the counters back to zero.
      game.play();
      game.frame(1 / 60);
      expect(game.state.screen.value, Screen.playing);
      expect((game.state.collected.value, game.state.total.value, game.state.won.value), (0, 10, false));
      expect(game.world.behaviours.whereType<Collectible>(), hasLength(10));

      // Giving up ends the run with what was collected (nothing).
      game.finish();
      game.frame(1 / 60);
      expect(game.state.screen.value, Screen.results);
      expect(game.state.lastRun.value!.won, isFalse);
      expect(game.state.lastRun.value!.collected, 0);

      // And the Menu again.
      game.toMenu();
      game.frame(1 / 60);
      expect(game.state.screen.value, Screen.menu);
      expect(game.scenes.lastReport!.sceneName, 'Menu');
      expect(game.world.find('Ring coin 8'), isNotNull);
      expect(game.world.isPoisoned, isFalse);
    },
  );

  test(
    'settings (M3-15): volumes, quality and a rebind go to settings.json, bind the actions and survive a relaunch',
    () {
      final dir = Directory.systemTemp.createTempSync('fynkel_reference_game_settings');
      addTearDown(() => dir.deleteSync(recursive: true));
      final path = '${dir.path}/settings.json';

      final game = Game.load(projectDir, settingsPath: path);
      expect(game.settings.current, SettingsDocument.defaults, reason: 'no file yet');
      expect(game.bindingsOf('jump'), [
        'keyboard:Space',
        'gamepad:south',
        'touch:overlay/jump',
      ], reason: 'the project\'s');
      // What the panel's controls do: sliders, the quality, a rebind of the
      // keyboard key with the other devices kept (lib/main.dart `_rebind`).
      game.settings.update(
        game.settings.current.copyWith(musicVolume: 0.3, muted: false, quality: 'high').rebind('jump', [
          'keyboard:J',
          'gamepad:south',
          'touch:overlay/jump',
        ]),
      );
      expect(File(path).existsSync(), isTrue);
      expect(game.bindingsOf('jump').first, 'keyboard:J');
      expect(game.bindingsOf('sprint').first, 'keyboard:ShiftLeft', reason: 'untouched');

      // In force in the level: J jumps, Space no longer does.
      game.play();
      game.frame(1 / 60);
      final jump = game.scenes.lastReport!.inputActions.singleWhere((a) => a.name == 'jump');
      expect(jump.rebound, isTrue);
      expect(jump.bindings, ['keyboard:J', 'gamepad:south', 'touch:overlay/jump']);
      final player = game.world.behaviours.whereType<Player>().single;
      game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'Space')]);
      game.frame(1 / 60, input: const [InputEvent.up(InputDevice.keyboard, 'Space')]);
      expect(player.jumps, 0);
      game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'J')]);
      game.frame(1 / 60, input: const [InputEvent.up(InputDevice.keyboard, 'J')]);
      expect(player.jumps, 1);
      // Reset: the project's key again.
      game.settings.update(game.settings.current.rebind('jump', const []));
      expect(game.bindingsOf('jump').first, 'keyboard:Space');
      game.settings.update(game.settings.current.rebind('jump', ['keyboard:J', 'gamepad:south', 'touch:overlay/jump']));
      game.dispose();

      // The next launch reads the file.
      final again = Game.load(projectDir, settingsPath: path);
      addTearDown(again.dispose);
      expect(again.settings.current.musicVolume, 0.3);
      expect(again.settings.current.quality, 'high');
      expect(again.settings.current.rebinds, {
        'jump': ['keyboard:J', 'gamepad:south', 'touch:overlay/jump'],
      });
      expect(again.bindingsOf('jump').first, 'keyboard:J');
    },
  );

  test('IK (ADR-0033 § 5): the feet on the Playground\'s steps, the head to the nearest coin', () {
    final game = playing(projectDir);
    addTearDown(game.dispose);
    final player = game.world.behaviours.whereType<Player>().single;
    Vec3 at(String name) => game.world.find(name)!.transform.world.position;
    void run(int frames) {
      for (var i = 0; i < frames; i++) {
        game.frame(1 / 60);
      }
    }

    // Across the edge of step 3 (top 0.6 m) and step 4 (0.8 m), turned so
    // the left foot is over the lower one: the controller stands on the
    // higher, the left foot goes down to its step and the hips with it.
    player.transform.position = const Vec3(12, 0.8, 15.5);
    player.transform.rotation = Quat.axisAngle(Vec3.up, math.pi / 2);
    run(90);
    final root = player.transform.position.y;
    expect(root, closeTo(0.8, 0.03), reason: 'on step 4');
    final left = at('foot_l').y, right = at('foot_r').y;
    expect(right - root, closeTo(0.06, 0.03), reason: 'the right foot\'s ankle on step 4, was $right');
    expect(left - root, closeTo(-0.14, 0.04), reason: 'the left foot\'s ankle on step 3, was $left');
    expect(at('hips').y - root, lessThan(0.85), reason: 'the hips go down as far as the lower foot needs');
    expect(player.anim.feet, 1, reason: 'standing: the feet are planted');

    // Back on flat ground the goal has nothing to do.
    player.transform.position = const Vec3(20, 0, 20);
    run(90);
    expect(at('hips').y - player.transform.position.y, closeTo(0.95, 0.05));
    expect(at('foot_l').y, closeTo(at('foot_r').y, 0.02));

    // The head turns to a coin 3 m to the right (within its 60°), the
    // nearest in 8 m; far from every coin the target is cleared.
    final coin = game.world.findAllWithTag('pickup').first.worldTransform!.position;
    player.transform.position = Vec3(coin.x - 3, 0, coin.z);
    player.transform.rotation = Quat.identity;
    run(60);
    expect(player.lookingAt, coin);
    final looking = game.world.find('head')!.transform.world.rotation.rotate(const Vec3(0, 0, -1));
    expect(looking.x, greaterThan(0.5), reason: 'turned to the +X side, was $looking');
    // The idle breathes the spine's position, not its rotation: the
    // spine's share of the turn stays put instead of adding up each tick.
    double bow() {
      final r = game.world.find('spine')!.transform.value.rotation;
      return 2 * math.acos(r.w.abs().clamp(0, 1)) * 180 / math.pi;
    }

    final settled = bow();
    run(180);
    expect(settled, lessThan(0.3 * 60 + 1), reason: 'the spine takes at most its share');
    expect(bow(), closeTo(settled, 0.5), reason: 'and keeps it');
    player.transform.position = const Vec3(20, 0, 20);
    run(30);
    expect(player.lookingAt, isNull);

    // Walking, foot placement is off: a stride's legs are the clip's.
    game.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'W')]);
    run(30);
    expect(player.anim.feet, 0, reason: 'faded out by ${player.feetFadeSpeed} m/s');

    // Stopped, the idle stands the legs at rest again (it keys them) and
    // the feet are planted side by side, not left mid-stride.
    game.frame(1 / 60, input: const [InputEvent.up(InputDevice.keyboard, 'W')]);
    run(60);
    expect(player.anim.feet, 1);
    final l = at('foot_l'), r = at('foot_r');
    expect((l.z - r.z).abs() + (l.y - r.y).abs(), lessThan(0.05), reason: 'feet together, was $l and $r');
  });
}
