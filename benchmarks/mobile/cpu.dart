// Copied into a generated mobile app's tool/cpu.dart. Mac CPU isolation only.
// Same scene and tick sequence for each population; no rendering or catch-up.
import 'dart:convert';

import 'package:fynkel_stress/game/game.dart';
import 'package:fynkel_stress/game/wobble.dart';

void main(List<String> args) {
  final project = args.first;
  final sampleCount = args.length > 1 ? int.parse(args[1]) : 120;
  for (final enabled in (args.length > 1 ? [300] : [0, 30, 300])) {
    final game = Game.load(project);
    try {
      final scripts = game.world.behaviours.whereType<Wobble>().toList();
      if (scripts.length != 300) throw StateError('Expected 300 Wobble instances, got ${scripts.length}');
      for (var i = 0; i < scripts.length; i++) {
        scripts[i].enabled = i < enabled;
      }
      final samples = <Map<String, Object?>>[];
      for (var i = 0; i < 40 + sampleCount; i++) {
        game.world.stepFrame();
        final times = game.world.frameTimes;
        if (i >= 40) {
          samples.add({
            'totalUs': times.totalUs,
            'behavioursUs': times.behavioursUs,
            'nativeUs': times.nativeUs,
            'ticks': times.ticks,
          });
        }
      }
      print(
        jsonEncode({
          'enabledWobbles': enabled,
          'authoredWobbles': scripts.length,
          'entities': game.world.stats().liveEntities,
          'samples': samples,
          'warmupTicks': 40,
          'measuredTicks': sampleCount,
          'scope': 'Mac CPU single fixed tick; no rendering; fresh world per variant',
        }),
      );
    } finally {
      game.dispose();
    }
  }
}
