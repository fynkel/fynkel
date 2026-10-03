// ADR-0033 § 5 on a rig we did not make: the Player wearing a downloaded
// Mixamo character (its `mixamorig:` bones — tilted axes, the ankle above
// the sole, knees bent at rest). It runs only in the copy
// `tools/hero_copy.py` makes (`.hero-copy` marks it); Mixamo's files stay
// local, so everywhere else it is skipped.

import 'dart:io';
import 'dart:math' as math;

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/gameplay.dart';
import 'package:fynkel/testing.dart';
import 'package:reference_game/game/game.dart';
import 'package:reference_game/game/player.dart';

/// The Playground's ground under (x, z): the four steps, the terrace, the
/// ramp, else the floor.
double groundAt(double x, double z) {
  if (x >= 10.5 && x <= 13.5 && z >= 14 && z <= 16) return 0.2 * (((z - 14) / 0.5).floor().clamp(0, 3) + 1);
  if (x >= 10.5 && x <= 13.5 && z > 16 && z <= 19) return 0.8;
  if (x >= 12 && x <= 15 && z >= -13.5 && z <= -10.5) return (x - 12) / 3;
  return 0;
}

void main() {
  final projectDir = Directory.current.path;
  final hero = File('$projectDir/.hero-copy').existsSync();

  test('the hero\'s feet stand on the steps and the ramp, its head turns to a coin', () {
    cookProject(projectDir);
    final game = Game.load(projectDir);
    addTearDown(game.dispose);
    game.play();
    game.frame(1 / 60);
    final seen = <FynkelDiagnostic>[];
    game.world.onDiagnostics = seen.addAll;
    final player = game.world.behaviours.whereType<Player>().single;
    Vec3 at(String n) => game.world.find('mixamorig:$n')!.transform.world.position;
    void run(int n, [List<InputEvent> input = const []]) {
      for (var i = 0; i < n; i++) {
        game.frame(1 / 60, input: i == 0 ? input : const []);
      }
    }

    double ankle(String side) {
      final f = at('${side}Foot');
      return f.y - groundAt(f.x, f.z);
    }

    // The knee stays in front of the line from the hip to the foot: the
    // two-bone solve kept the bend Mixamo's rest pose has.
    void kneesForward(String where) {
      final forward = player.transform.rotation.rotate(const Vec3(0, 0, -1));
      for (final side in ['Left', 'Right']) {
        final mid = (at('${side}UpLeg') + at('${side}Foot')) * 0.5;
        expect((at('${side}Leg') - mid).dot(forward), greaterThan(0), reason: '$where: the $side knee bends forward');
      }
    }

    // Flat ground: the ankle's own height, what every place is checked against.
    player.transform.position = const Vec3(20, 0, 20);
    run(120);
    final rest = (ankle('Left') + ankle('Right')) / 2;
    expect(rest, inInclusiveRange(0.05, 0.15), reason: 'Mixamo\'s ankle sits above the sole');
    final hips = at('Hips').y - player.transform.position.y;

    // Across step 3 (0.6 m) and step 4 (0.8 m), in four facings: each ankle
    // on its own step, the hips down one step when a foot is on the lower.
    for (final yaw in [0.0, math.pi / 2, math.pi, -math.pi / 2]) {
      player.transform.position = const Vec3(12, 0.8, 15.5);
      player.transform.rotation = Quat.axisAngle(Vec3.up, yaw);
      run(120);
      final where = 'the step edge, yaw ${yaw.toStringAsFixed(2)}';
      expect(player.transform.position.y, closeTo(0.8, 0.03), reason: where);
      for (final side in ['Left', 'Right']) {
        expect(ankle(side), closeTo(rest, 0.02), reason: '$where: the $side foot on its step');
      }
      final lower = [at('LeftFoot'), at('RightFoot')].any((f) => groundAt(f.x, f.z) < 0.7);
      final drop = hips - (at('Hips').y - player.transform.position.y);
      expect(drop, lower ? closeTo(0.2, 0.03) : closeTo(0, 0.03), reason: '$where: the hips');
      kneesForward(where);
    }

    // Up the ramp and stopped: the feet planted on the slope.
    player.transform.position = const Vec3(10.5, 0, -12);
    player.transform.rotation = Quat.identity;
    run(30);
    run(40, const [InputEvent.down(InputDevice.keyboard, 'D')]);
    run(90, const [InputEvent.up(InputDevice.keyboard, 'D')]);
    expect(player.anim.feet, 1);
    for (final side in ['Left', 'Right']) {
      expect(ankle(side), closeTo(rest, 0.02), reason: 'the ramp: the $side foot on the slope');
    }
    kneesForward('the ramp');

    // A coin 3 m to the right: the head turns to it (at most the 60°
    // limit), the spine's share stays put.
    final coin = game.world.findAllWithTag('pickup').first.worldTransform!.position;
    player.transform.position = Vec3(coin.x - 3, 0, coin.z);
    player.transform.rotation = Quat.identity;
    double bow() {
      final r = game.world.find('mixamorig:Spine1')!.transform.value.rotation;
      return 2 * math.acos(r.w.abs().clamp(0, 1)) * 180 / math.pi;
    }

    run(60);
    expect(player.lookingAt, coin);
    // Mixamo's head faces +Z in the model; the Rig turned the model to -Z.
    final face = game.world.find('mixamorig:Head')!.transform.world.rotation.rotate(const Vec3(0, 0, 1));
    final yaw = math.atan2(face.x, -face.z) * 180 / math.pi;
    expect(yaw, closeTo(60, 6), reason: 'turned to the right, as far as the limit');
    final settled = bow();
    run(180);
    expect(bow(), closeTo(settled, 1), reason: 'the spine keeps its share');

    expect(seen.where((d) => d.code == DiagnosticCode.ikUnavailable), isEmpty, reason: 'every bone found');
  }, skip: hero ? false : 'only in the hero copy (python3 tools/hero_copy.py)');
}
