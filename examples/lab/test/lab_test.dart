// Every scene of the lab runs, and every control works, without an error
// (but the buggy cube's, which is the point of it).

import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:lab/lab.dart';
import 'package:lab/scenes.dart';
import 'package:lab/scenes/character_basics.dart';
import 'package:lab/scenes/character_course.dart';
import 'package:lab/scenes/contacts_basics.dart';
import 'package:lab/scenes/input.dart';
import 'package:lab/scenes/joints_basics.dart';
import 'package:lab/scenes/joints_bridge.dart';
import 'package:lab/scenes/layer_matrix.dart';
import 'package:lab/scenes/layers_basics.dart';
import 'package:lab/scenes/mesh_colliders_basics.dart';
import 'package:lab/scenes/motion.dart';
import 'package:lab/scenes/motion_basics.dart';
import 'package:lab/scenes/physics_queries_basics.dart';
import 'package:lab/scenes/pointers.dart';
import 'package:lab/scenes/rigidbody_walls.dart';

/// The lab's assets, from its folder, as the app has them.
Future<Uint8List> files(String asset) async => File('assets/$asset').readAsBytesSync();

void main() {
  for (final make in scenes) {
    final title = make().title;
    test('$title runs, and every control works', () async {
      final world = World(source: files);
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

  test('Mesh colliders: the basics: the ball lands on the rock, and rolls off it onto the floor', () async {
    final world = World(source: files);
    addTearDown(world.dispose);
    final scene = world.add(MeshCollidersBasicsScene());
    expect(scene.rock.convex, isFalse, reason: 'asked before the model is in');
    await scene.loaded;
    var highest = 0.0;
    for (var i = 0; i < 60; i++) {
      world.update(1 / 60);
      if (i >= 45) highest = highest > scene.ball.position.y ? highest : scene.ball.position.y;
    }
    expect(highest, greaterThan(0.8), reason: 'on the rock\'s slope, not the floor');
    for (var i = 0; i < 600; i++) {
      world.update(1 / 60);
    }
    expect(scene.ball.position.y, closeTo(0.3, 0.05), reason: 'rolled off onto the floor');
    final Vec3(:x, :z) = scene.ball.position;
    expect(x.abs(), lessThan(2), reason: 'still in view');
    expect(z, inInclusiveRange(3, 6), reason: 'down the slope towards the camera, and stopped');
    scene.controls.whereType<Toggle>().first.onChanged(true);
    expect(scene.rock.convex, isTrue);
  });

  test('Character controller: the basics: up the low step, not the high one, and the crate is pushed', () async {
    final world = World(source: files);
    addTearDown(world.dispose);
    final scene = world.add(CharacterBasicsScene());
    void walkTo(Vec3 target) {
      scene.player.walker.target = target;
      for (var i = 0; i < 240; i++) {
        world.update(1 / 60);
      }
    }

    walkTo(Vec3(-2.5, 0, -1));
    expect(scene.player.position.y - 1.08, closeTo(0.25, 0.05), reason: 'on the low step');
    expect(scene.player.controller.isGrounded, isTrue);
    scene.reset();
    walkTo(Vec3(2.5, 0, -1));
    expect(scene.player.position.y - 1.08, closeTo(0, 0.05), reason: 'not on the high step');
    expect(scene.player.position.z, greaterThan(0), reason: 'stopped at it');
    scene.reset();
    walkTo(Vec3(0, 0, -1));
    expect(scene.crate.position.z, lessThan(1), reason: 'pushed back from z = 1.5');
  });

  test('Character controller: a course: the stairs and the gentle ramp, not the steep one, and coins', () async {
    final world = World(source: files);
    addTearDown(world.dispose);
    final scene = world.add(CharacterCourseScene());
    double walkTo(Vec3 target, {double seconds = 4}) {
      scene.player.walker.target = target;
      for (var i = 0; i < seconds * 60; i++) {
        world.update(1 / 60);
      }
      return scene.player.position.y - 1.08;
    }

    expect(walkTo(Vec3(1.2, 0, -3)), greaterThan(1), reason: 'up the 25° ramp');
    walkTo(Vec3(1.2, 0, 3));
    expect(walkTo(Vec3(3.4, 0, -3)), lessThan(0.5), reason: 'not up the 55° ramp');
    walkTo(Vec3(-5, 0, 2));
    expect(walkTo(Vec3(-5, 0, -4.4), seconds: 5), closeTo(0.8, 0.05), reason: 'up the stairs to the landing');
    expect(scene.log.value.map((l) => l.text), contains('picked up a coin: 1 of 4'));
  });

  test('Joints: the basics: the door swings shut, the chain hangs joined, and the crate settles', () async {
    final world = World(source: files);
    addTearDown(world.dispose);
    final scene = world.add(JointsBasicsScene());
    void frames(int n) {
      for (var i = 0; i < n; i++) {
        world.update(1 / 60);
      }
    }

    frames(60);
    scene.controls.whereType<Button>().first.onPressed();
    frames(20);
    expect(scene.hinge.angle.abs(), greaterThan(20), reason: 'pushed open');
    frames(600);
    expect(scene.hinge.angle.abs(), lessThan(3), reason: 'its spring swung it shut');
    expect(scene.door.position.y, closeTo(1.1, 0.05), reason: 'still on its hinge');
    for (final (i, link) in scene.links.indexed) {
      expect(link.position.y, closeTo(3.8 - i * 0.4, 0.05), reason: 'link ${i + 1} hangs where it was');
    }
    double below() => (scene.crate.position - JointsBasicsScene.hook).length;
    // 2 kg on a 40 N/m spring past its 1.5 m maxDistance: 1.5 + 2 × 9.81 / 40.
    expect(below(), closeTo(1.99, 0.05), reason: 'hangs its weight past its maxDistance');
    expect(scene.crate.position.y, greaterThan(1.5), reason: 'hanging, not on the floor');

    // Pulled down, it goes down, and the spring brings it back.
    scene.controls.whereType<Button>().firstWhere((b) => b.label == 'pull the crate down').onPressed();
    var furthest = below();
    for (var i = 0; i < 30; i++) {
      frames(1);
      furthest = math.max(furthest, below());
    }
    expect(furthest, greaterThan(2.3), reason: 'the pull stretched the spring');
    // Its damper is light and it moves freely within 1.5 m: it bobs a while.
    frames(300);
    var (low, high) = (below(), below());
    for (var i = 0; i < 120; i++) {
      frames(1);
      (low, high) = (math.min(low, below()), math.max(high, below()));
    }
    expect(high, lessThan(furthest), reason: 'the spring caught it, and its bobs are dying down');
    expect((low + high) / 2, closeTo(1.99, 0.3), reason: 'bobbing about where it hangs');
  });

  test('Joints: a bridge: it holds a crate, and with a low break force, breaks under it', () async {
    final world = World(source: files);
    addTearDown(world.dispose);
    final scene = world.add(JointsBridgeScene());
    void frames(int n) {
      for (var i = 0; i < n; i++) {
        world.update(1 / 60);
      }
    }

    frames(120);
    scene.dropCrate(Vec3(0, 4.5, 0));
    frames(300);
    final crate = scene.crates.single;
    expect(crate.position.y, greaterThan(0.8), reason: 'on the bridge, not on the floor');
    expect(scene.broken, 0);
    scene.rebuild();
    scene.setBreakForce(250);
    frames(120);
    scene.dropCrate(Vec3(0, 4.5, 0));
    frames(300);
    expect(scene.broken, greaterThan(0));
    expect(scene.log.value.map((l) => l.text), contains(startsWith('a joint on plank broke')));
    expect(scene.crates.single.position.y, lessThan(0.5), reason: 'fell through, onto the floor');
  });
}
