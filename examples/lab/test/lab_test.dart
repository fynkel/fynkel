// Every scene of the lab runs, and every control works, without an error
// (but the buggy cube's, which is the point of it).

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:lab/lab.dart';
import 'package:lab/scenes.dart';
import 'package:lab/scenes/contacts_basics.dart';
import 'package:lab/scenes/input.dart';
import 'package:lab/scenes/layer_matrix.dart';
import 'package:lab/scenes/layers_basics.dart';
import 'package:lab/scenes/motion.dart';
import 'package:lab/scenes/motion_basics.dart';
import 'package:lab/scenes/physics_queries_basics.dart';
import 'package:lab/scenes/pointers.dart';
import 'package:lab/scenes/rigidbody_walls.dart';

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
      if (world.raycast(camera.screenToRay(p))?.object == crate) on = p;
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
        if (world.raycast(camera.screenToRay(q))?.object == pad) padAt = q;
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

  test('Motion: the basics: move goes there, and all in a row comes back down', () async {
    final world = World();
    addTearDown(world.dispose);
    final scene = world.add(MotionBasicsScene());
    await scene.loaded;
    void frames(int n) {
      for (var i = 0; i < n; i++) {
        world.update(1 / 60);
      }
    }

    final [move, ..., row] = scene.controls.cast<Button>();
    move.onPressed();
    frames(60);
    expect(scene.cube.position, const Vec3(2, 0.5, 0));
    row.onPressed();
    frames(60);
    expect(scene.cube.position.y, greaterThan(0.5), reason: 'still on its way');
    frames(60);
    expect(scene.cube.position, const Vec3(0, 0.5, 0));
  });

  test('Motion: the door is awaited, the race is won by the wait, and the patrol comes round again', () async {
    final world = World();
    addTearDown(world.dispose);
    final scene = world.add(MotionScene());
    await scene.loaded;
    void seconds(double s) {
      for (var i = 0; i < s * 60; i++) {
        world.update(1 / 60);
      }
    }

    seconds(0.1);
    final start = scene.guard.position;
    // The square's side is 4 at 2 a second, plus a 0.4 s turn: 9.6 s round.
    seconds(9.6);
    expect(scene.guard.position.closeTo(start, 0.05), isTrue, reason: '${scene.guard.position}');

    Button control(String label) => scene.controls.whereType<Button>().firstWhere((b) => b.label.startsWith(label));
    final doorY = scene.door.position.y;
    control('open the door').onPressed();
    seconds(1.2);
    expect(scene.door.position.y, closeTo(doorY + 1.9, 1e-9));
    seconds(2);
    expect(scene.door.position.y, closeTo(doorY, 1e-9));
    control('race').onPressed();
    seconds(1.2);
    final lines = scene.log.value.map((l) => l.text);
    expect(lines, containsAllInOrder(['door: opening…', startsWith('door: open'), 'door: closed']));
    expect(lines, contains(startsWith('race: the 1 s wait won')));
    expect(scene.runner.position.x, lessThan(-0.5), reason: 'stopped about a third of the way');
  });

  test('Layers and debug: the basics: the ray hits the blue cube until told to go through layer 1', () async {
    final world = World();
    addTearDown(world.dispose);
    final scene = world.add(LayersBasicsScene());
    await scene.loaded;
    world.update(1 / 60);
    expect(scene.readout.value, contains('the ray hits: blue cube'));
    Toggle control(String label) => scene.controls.whereType<Toggle>().firstWhere((c) => c.label == label);

    control('ray hits layer 1').onChanged(false);
    world.update(1 / 60);
    expect(scene.readout.value, contains('the ray hits: orange cube'));

    control('camera draws layer 1').onChanged(false);
    expect(world.camera!.get<Camera>()!.cullingMask, LayerMask.except([blueLayer]));
    expect(scene.blue.layer, blueLayer);

    // A line and a sphere's 96 a frame; the box's 12 stay two seconds.
    scene.controls.whereType<Button>().single.onPressed();
    world.update(1 / 60);
    expect(world.debug.count, 12);
    control('debug lines').onChanged(false);
    world.update(1 / 60);
    expect(world.debug.count, 0);
  });

  test('Physics queries: the basics: what a ray finds, as the controls change it', () async {
    final world = World();
    addTearDown(world.dispose);
    final scene = world.add(PhysicsQueriesBasicsScene());
    await scene.loaded;
    world.update(1 / 60);
    final physics = world.physics;
    expect(physics.colliderCount, 3);
    expect(physics.drawColliders, isTrue);
    GameObject? along(double x) => physics.raycast(Ray(Vec3(x, 0.5, 4), Vec3(0, 0, -1)))?.gameObject;
    expect([along(-2), along(0), along(2)], [scene.cube, scene.ball, scene.pill]);
    Toggle control(String label) => scene.controls.whereType<Toggle>().firstWhere((c) => c.label == label);

    control('cube is a trigger').onChanged(true);
    expect(along(-2), scene.cube, reason: 'queries hit triggers unless told not to');
    control('queries hit triggers').onChanged(false);
    expect(along(-2), isNull);
    control('ball on layer 1, the ray skips it').onChanged(true);
    expect(scene.ball.layer, ballLayer);
    expect(physics.raycast(Ray(Vec3(0, 0.5, 4), Vec3(0, 0, -1)), layers: LayerMask.except([ballLayer])), isNull);

    control('queries hit triggers').onChanged(true);
    scene.controls.whereType<Button>().single.onPressed();
    expect(along(-2), isNull, reason: 'moved up, out of the ray, before any frame');
    expect(physics.raycast(Ray(Vec3(-2, 2, 4), Vec3(0, 0, -1)))?.gameObject, scene.cube);
  });

  test('Rigidbodies: a ball through walls: it passes the moving walls, unless continuous', () async {
    final world = World();
    addTearDown(world.dispose);
    final scene = world.add(RigidbodyWallsScene());
    await scene.loaded;
    world.update(1 / 60);
    void fire() {
      scene.controls.whereType<Button>().single.onPressed();
      for (var i = 0; i < 60; i++) {
        world.update(1 / 60);
      }
    }

    fire();
    expect(scene.readout.value, contains('it went through every wall'));
    scene.controls.whereType<Toggle>().single.onChanged(true);
    fire();
    expect(scene.readout.value, contains('before wall 1'));
  });

  test('Triggers and collisions: the basics: the slab hears the ball pass, the ball hears the floor', () async {
    final world = World();
    addTearDown(world.dispose);
    final scene = world.add(ContactsBasicsScene());
    await scene.loaded;
    for (var i = 0; i < 120; i++) {
      world.update(1 / 60);
    }
    expect(
      scene.log.value.map((l) => l.text),
      containsAllInOrder([
        'slab: onTriggerEnter(ball)',
        'slab: onTriggerExit(ball)',
        startsWith('ball: onCollisionEnter(floor collider)'),
      ]),
    );
    expect(scene.zone.inside, isEmpty);
    scene.controls.whereType<Toggle>().single.onChanged(false);
    scene.controls.whereType<Button>().single.onPressed();
    for (var i = 0; i < 120; i++) {
      world.update(1 / 60);
    }
    expect(scene.ball.position.y, closeTo(2.6, 0.05), reason: 'on the slab, now solid');
  });

  test('The layer matrix: layer 8 falls through the floor, and so does blue alone', () async {
    final world = World();
    addTearDown(world.dispose);
    final scene = world.add(LayerMatrixScene());
    await scene.loaded;
    void frames(int n) {
      for (var i = 0; i < n; i++) {
        world.update(1 / 60);
      }
    }

    Toggle control(String label) => scene.controls.whereType<Toggle>().firstWhere((c) => c.label == label);
    frames(120);
    expect(scene.red.position.y, closeTo(0.5, 0.05));
    control('layer 8 meets the floor').onChanged(false);
    control('blue meets the floor').onChanged(false);
    scene.controls.whereType<Button>().single.onPressed();
    frames(70);
    expect(scene.red.position.y, lessThan(0));
    expect(scene.blue.position.y, lessThan(0));
    expect(scene.readout.value, contains('layer 8 meets layer 0: false'));
  });
}
