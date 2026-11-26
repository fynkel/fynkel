// The promo film's footage of Pocket Hole, upright as a phone holds it: the
// hole glides from block to block, as a finger would drag it, and they
// tumble in.
// FYNKEL_FOOTAGE=<dir> flutter test test/promo_footage_test.dart

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:pocket_hole/game.dart';

import 'promo_recorder.dart';

void main() {
  test('pocket_hole', () async {
    const width = 1080, height = 2340;
    final world = World()..quality = Quality.high;
    addTearDown(world.dispose);
    world.input.simulate.resize(width.toDouble(), height.toDouble());
    final game = world.add(PocketGame());
    world.update(1 / 60);
    game.start();
    for (var i = 0; i < 30; i++) {
      world.update(1 / 60);
    }
    // The blocks in the order a player would sweep them: nearest first.
    final left = [for (final p in layout(1)) Vec3(p.x, 0, p.z)];
    var at = game.target;
    var goal = left.isEmpty ? at : left.first;
    await record(world, 'pocket_hole', 12, width: width, height: height, before: (t) {
      if ((goal - at).length < 0.15 && left.isNotEmpty) {
        left.sort((a, b) => (a - at).length.compareTo((b - at).length));
        goal = left.removeAt(0);
      }
      // A finger's pace: eased toward the next block, never faster than 6 m/s.
      final step = (goal - at) * 0.08;
      final most = 6 / 60;
      at = at + (step.length > most ? step.normalized * most : step);
      game.target = at;
    });
  }, skip: skipFootage, timeout: const Timeout(Duration(minutes: 10)));
}
