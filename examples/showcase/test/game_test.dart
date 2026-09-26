// The showcase loads headlessly and its behaviours run: no window, no GPU.
// `flutter test` runs this; `fynkel test --headless` runs the same scene on
// the Rust host without the Dart behaviours, and `fynkel render` draws it.
// The world reads the cooked cache only, so the test cooks the project
// first (`cookProject` runs the `fynkel` CLI; a warm cache costs
// milliseconds).

import 'dart:io';
import 'dart:math' as math;

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:showcase/game/dissolve.dart';
import 'package:showcase/game/game.dart';
import 'package:showcase/game/orbit.dart';
import 'package:showcase/game/spinner.dart';

void main() {
  final projectDir = Directory.current.path;
  setUpAll(() => cookProject(projectDir));

  test('the showcase loads, the robot spins, the camera orbits and the runner runs', () {
    final game = Game.load(projectDir);
    addTearDown(game.dispose);

    expect(game.report.ok, isTrue);
    expect(game.report.diagnostics, isEmpty, reason: 'no load warnings expected');
    expect(game.report.entityCount, 48, reason: 'the Runner instance is the glTF scene root (ADR-0022), with Body and the joints under it; the Global Volume (R3a) is one more');
    expect((game.report.animation.clips, game.report.animation.skins), (2, 1), reason: 'the runner\'s controller: run and jump, one skin');
    expect(game.report.behavioursAttached, 2);
    final spinner = game.world.behaviours.whereType<Spinner>().single;
    final orbit = game.world.behaviours.whereType<Orbit>().single;
    expect(spinner.speed, 0.6, reason: 'authored in assets/scenes/main.scene.json');
    expect(orbit.radius, 12.0);
    expect(orbit.phase, 0.6);

    for (var i = 0; i < 30; i++) {
      game.frame(1 / 60);
    }
    expect(game.world.tick, greaterThan(0));
    expect(spinner.angle, closeTo(0.6 * game.world.tick / 60, 1e-9));
    expect(orbit.angle, closeTo(0.6 + 0.12 * game.world.tick / 60, 1e-9));
    // The camera stays on its circle around the target.
    final eye = orbit.transform.position - orbit.target;
    expect(math.sqrt(eye.x * eye.x + eye.z * eye.z), closeTo(orbit.radius, 1e-4));
    expect(eye.y, closeTo(orbit.height, 1e-4));

    // The runner's joints follow the run clip (in place: the Runner stays).
    // Since M3-19a the Runner instances `character.glb#prefab`: the glTF's
    // own hierarchy, its scene root being the instance itself (ADR-0022),
    // then `Body` and the joints, the suit as a material override.
    final runner = game.world.find('Runner')!;
    final character = runner;
    final thigh = character.findChild('hips/thigh_l')!;
    expect(character.findChild('Body')!.get<MeshRenderer>()!.material.asset, '0605f86c-548f-5a17-83a4-d07f21f52896',
        reason: 'the suit, set by the instance\'s override on node/Character/Body/renderer');
    expect(character.findChild('Body')!.get<Skin>()!.skeleton, 'hips');
    // Its Animator Controller (assets/models/runner.controller.json) needs
    // no code: Run loops four laps, then Jump plays once and Run comes
    // back, both on exit times.
    var s = runner.animation.stateOf()!;
    expect(s.name, 'Run');
    expect(s.length, closeTo(0.6, 1e-6));
    expect(s.normalizedTime, closeTo((game.world.tick - 1) / 60 / 0.6, 1e-3));
    expect(thigh.localTransform.rotation.x.abs(), greaterThan(0.05), reason: 'a leg swings');
    expect(runner.transform.position.z, closeTo(2.6, 1e-5), reason: 'in place');
    while (game.world.tick < 4 * 36 + 10) {
      game.frame(1 / 60);
    }
    s = runner.animation.stateOf()!;
    expect(s.next ?? s.name, 'Jump', reason: 'four laps of 0.6 s, then the jump');
    for (var i = 0; i < 60; i++) {
      game.frame(1 / 60);
    }
    s = runner.animation.stateOf()!;
    expect(s.next ?? s.name, 'Run', reason: 'the 0.8 s jump played once, then the run again');
  });

  test('the shader scenes load, and Dissolve drives its material from code (ADR-0031)', () {
    final game = Game.load(projectDir);
    addTearDown(game.dispose);

    for (final MapEntry(key: name, value: id) in Game.scenes.entries.skip(1)) {
      final report = game.loadScene(id);
      expect(report.ok, isTrue, reason: name);
      expect(report.diagnostics, isEmpty, reason: name);
    }

    game.loadScene(Game.scenes['Dissolve']!);
    game.frame(0); // the earlier scenes' behaviours leave on the next frame
    final dissolve = game.world.behaviours.whereType<Dissolve>().single;
    expect((dissolve.period, dissolve.hold), (6.0, 1.0), reason: 'authored in shader_dissolve.scene.json');
    for (var i = 0; i < 150; i++) {
      game.frame(1 / 60);
    }
    // 2.5 s in: whole for 1 s, then half of the 3 s burn.
    expect(dissolve.amount, closeTo(0.5, 0.02));
    final material = dissolve.get<MeshRenderer>()!.material;
    expect(game.world.materialValue(material, 'amount'), dissolve.amount);

    expect(dissolve.amountAt(0.5), 0);
    expect(dissolve.amountAt(4.5), 1);
    expect(dissolve.amountAt(6.5), closeTo(0.5, 1e-9));
    expect(dissolve.amountAt(8.5), 0, reason: 'a new cycle');
  });
}
