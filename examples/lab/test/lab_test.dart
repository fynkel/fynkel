// Every scene of the lab runs, and every control works, without an error
// (but the buggy cube's, which is the point of it).

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:lab/lab.dart';
import 'package:lab/scenes.dart';
import 'package:lab/scenes/input.dart';
import 'package:lab/scenes/pointers.dart';

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

  test('Input on objects: a crate dragged off itself follows the finger, and a pad counts clicks', () async {
    final world = World();
    addTearDown(world.dispose);
    final scene = world.add(PointersScene());
    await scene.loaded;
    world.input.resize(400, 300);
    world.update(1 / 60);

    final crate = world.all<GameObject>().firstWhere((o) => o.name == 'crate 2');
    final camera = scene.camera;
    final from = crate.transform.position;
    // Where the crate is on the view: aim at it until a ray hits it.
    ViewPoint? on;
    for (var y = 0.0; y < 300 && on == null; y += 2) {
      final p = ViewPoint(200, y, width: 400, height: 300);
      if (world.raycast(camera.rayThrough(p))?.object == crate) on = p;
    }
    world.input.fingerDown(1, on!.x, on.y);
    world.update(1 / 60);
    for (var i = 1; i <= 10; i++) {
      world.input.fingerMove(1, on.x + i * 10, on.y);
      world.update(1 / 60);
    }
    world.input.fingerUp(1, on.x + 100, on.y);
    world.update(1 / 60);
    final to = crate.transform.position;
    expect(to.x, greaterThan(from.x + 0.5), reason: 'dragged right');
    expect(to.y, from.y, reason: 'over the floor, at its height');
    expect(
      scene.log.value.map((l) => l.text),
      containsAllInOrder(['crate 2: drag taken', startsWith('crate 2: dropped')]),
    );

    final pad = world.all<Pad>().first;
    final p = ViewPoint(200, 0, width: 400, height: 300);
    ViewPoint? padAt;
    for (var x = 0.0; x < 400 && padAt == null; x += 2) {
      for (var y = 0.0; y < 300 && padAt == null; y += 4) {
        final q = ViewPoint(x, y, width: p.width, height: p.height);
        if (world.raycast(camera.rayThrough(q))?.object == pad) padAt = q;
      }
    }
    for (var i = 0; i < 2; i++) {
      world.input
        ..fingerDown(2 + i, padAt!.x, padAt.y)
        ..fingerUp(2 + i, padAt.x, padAt.y);
      world.update(1 / 60);
    }
    expect(pad.clicks, 2);
    expect(scene.log.value.map((l) => l.text), contains('${pad.name}: click ×2'));
  });

  test('Input: fingers and keys: the move axis moves the puck', () async {
    final world = World();
    addTearDown(world.dispose);
    await world.add(InputScene()).loaded;
    final puck = world.only<Puck>();
    world.update(1 / 60);
    final from = puck.transform.position;
    world.input.keyDown(KeyCode.d);
    for (var i = 0; i < 30; i++) {
      world.update(1 / 60);
    }
    expect(puck.transform.position.x, closeTo(from.x + 2, 0.1), reason: '4 a second for half a second');
  });
}
