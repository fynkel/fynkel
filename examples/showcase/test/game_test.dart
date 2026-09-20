// The showcase loads headlessly and its behaviours run: no window, no GPU.
// `flutter test` runs this; `fynkel test --headless` runs the same scene on
// the Rust host without the Dart behaviours, and `fynkel render` draws it.
// The world reads the cooked cache only, so the test cooks the project
// first (`cookProject` runs the `fynkel` CLI; a warm cache costs
// milliseconds).

import 'dart:io';
import 'dart:math' as math;

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/testing.dart';
import 'package:showcase/game/game.dart';
import 'package:showcase/game/orbit.dart';
import 'package:showcase/game/spinner.dart';

void main() {
  final projectDir = Directory.current.path;
  setUpAll(() => cookProject(projectDir));

  test('the showcase loads, the robot spins and the camera orbits', () {
    final game = Game.load(projectDir);
    addTearDown(game.dispose);

    expect(game.report.ok, isTrue);
    expect(game.report.diagnostics, isEmpty, reason: 'no load warnings expected');
    expect(game.report.entityCount, 34);
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
  });
}
