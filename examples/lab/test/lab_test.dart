// Every scene of the lab runs, and every control works, without an error
// (but the buggy cube's, which is the point of it).

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:lab/lab.dart';
import 'package:lab/scenes.dart';

void main() {
  for (final make in scenes) {
    final title = make().title;
    test('$title runs, and every control works', () async {
      final world = World();
      addTearDown(world.dispose);
      final scene = world.add(make());
      await scene.loaded;
      void frames(int n) {
        for (var i = 0; i < n; i++) {
          world.update(1 / 60);
        }
      }

      frames(300);
      for (final control in scene.controls) {
        switch (control) {
          case Button(:final onPressed):
            onPressed();
          case Toggle(:final value, :final onChanged):
            onChanged(!value());
            frames(30);
            onChanged(!value());
          case Slider(:final onChanged, :final min, :final max):
            onChanged(min + (max - min) / 3);
        }
        frames(60);
      }
      frames(300);
      await pumpEventQueue();
      final errors = scene.log.value.where((l) => l.error && !l.text.contains('a bug in buggy'));
      expect(errors, isEmpty);
      expect(scene.readout.value, title == 'Lifecycle' ? isEmpty : isNotEmpty);
    });
  }
}
