// Every scene of the lab runs, and every control works, without an error
// (but the buggy cube's, which is the point of it).

import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:flutter/material.dart' show Brightness;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:lab/lab.dart';
import 'package:lab/palette.dart';
import 'package:lab/scenes.dart';
import 'package:lab/scenes/character_basics.dart';
import 'package:lab/scenes/character_course.dart';
import 'package:lab/scenes/addons_basics.dart';
import 'package:lab/scenes/colour_basics.dart';
import 'package:lab/scenes/contacts_basics.dart';
import 'package:lab/scenes/flutter_basics.dart';
import 'package:lab/scenes/input.dart';
import 'package:lab/scenes/joints_basics.dart';
import 'package:lab/scenes/joints_bridge.dart';
import 'package:lab/scenes/joints_chain.dart';
import 'package:lab/scenes/joints_character.dart';
import 'package:lab/scenes/layer_matrix.dart';
import 'package:lab/scenes/layers_basics.dart';
import 'package:lab/scenes/mesh_colliders_basics.dart';
import 'package:lab/scenes/motion.dart';
import 'package:lab/scenes/motion_basics.dart';
import 'package:lab/scenes/moving_basics.dart';
import 'package:lab/scenes/physics_queries_basics.dart';
import 'package:lab/scenes/pointers.dart';
import 'package:lab/scenes/controls_basics.dart';
import 'package:lab/scenes/pooling_basics.dart';
import 'package:lab/scenes/riding_basics.dart';
import 'package:lab/scenes/rigidbody_walls.dart';
import 'package:lab/scenes/shapes_basics.dart';
import 'package:lab/scenes/theme_basics.dart';
import 'package:lab/scenes/world_widgets_basics.dart';

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
    world.input.simulate.resize(400, 300);
    world.update(1 / 60);

    final crate = world.all<GameObject>().firstWhere((o) => o.name == 'crate 2');
    final camera = scene.camera;
    final from = crate.transform.position;
    // Where the crate is on the view: aim at it until a ray hits it.
    ViewPoint? on;
    for (var y = 0.0; y < 300 && on == null; y += 2) {
      final p = ViewPoint(200, y, width: 400, height: 300);
      if (world.raycast(camera.screenPointToRay(p))?.gameObject == crate) on = p;
    }
    world.input.simulate.fingerDown(1, on!.x, on.y);
    world.update(1 / 60);
    for (var i = 1; i <= 10; i++) {
      world.input.simulate.fingerMove(1, on.x + i * 10, on.y);
      world.update(1 / 60);
    }
    world.input.simulate.fingerUp(1, on.x + 100, on.y);
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
        if (world.raycast(camera.screenPointToRay(q))?.gameObject == pad) padAt = q;
      }
    }
    for (var i = 0; i < 2; i++) {
      world.input.simulate
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
    world.input.simulate.keyDown(KeyCode.d);
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

  test('Input: Flutter controls and pause: the stick and the button drive the cube; pause stops it all', () {
    final world = World(source: files);
    addTearDown(world.dispose);
    final scene = world.add(ControlsBasicsScene());
    void frames(int n) {
      for (var i = 0; i < n; i++) {
        world.update(1 / 60);
      }
    }

    frames(1);
    final [stickX, _, jump, pause] = scene.controls;
    (stickX as Slider).onChanged(1);
    frames(30);
    expect(scene.cube.position.x, closeTo(2, 0.01), reason: 'the stick moves it 4 m/s');
    stickX.onChanged(0);
    (jump as Button).onPressed();
    frames(10);
    expect(scene.cube.position.y, greaterThan(0.5));
    expect(scene.log.value.map((e) => e.text), contains('jump'));
    frames(60);
    expect(scene.cube.position.y, 0.5, reason: 'back down');

    (pause as Toggle).onChanged(true);
    final turn = scene.spinner.rotation;
    stickX.onChanged(1);
    frames(30);
    expect(scene.spinner.rotation, turn);
    expect(scene.cube.position.x, closeTo(2, 0.01), reason: 'paused: the stick reads nothing');
    pause.onChanged(false);
    frames(31);
    expect(scene.cube.position.x, closeTo(4, 0.01), reason: 'the first frame after counts no time');
  });

  test('Flutter: beside the game: the stick moves the cube, and P pauses and goes on, heard by the banner', () {
    final world = World(source: files);
    addTearDown(world.dispose);
    final scene = world.add(FlutterBasicsScene());
    void frames(int n) {
      for (var i = 0; i < n; i++) {
        world.update(1 / 60);
      }
    }

    frames(1);
    final banner = <bool>[];
    world.pausedListenable.addListener(() => banner.add(world.pausedListenable.value));
    world.input.setAxis2('move', const Vec2(1, 0));
    frames(30);
    expect(scene.cube.position.x, closeTo(2, 0.01), reason: 'the stick moves it 4 m/s');
    world.input.setAxis2('move', Vec2.zero);
    world.input.simulate.keyDown(KeyCode.p);
    frames(1);
    world.input.simulate.keyUp(KeyCode.p);
    frames(1);
    expect(world.paused, isTrue);
    world.input.simulate.keyDown(KeyCode.p);
    frames(1);
    expect(world.paused, isTrue, reason: 'paused, the game hears no keys');
    world.paused = false; // the banner's tap
    frames(1);
    expect(banner, [true, false]);
    expect(scene.log.value.map((e) => e.text), containsAllInOrder(['paused', 'going on']));
  });

  test('Theme: light and dark: the sky, floor and cubes follow the theme, from build on', () {
    final world = World(source: files)..theme = labTheme(Brightness.dark);
    addTearDown(world.dispose);
    final scene = world.add(ThemeBasicsScene());
    world.update(1 / 60);
    final dark = labTheme(Brightness.dark).colorScheme;
    expect(scene.camera.clearColor, dark.surface.toFynkel(), reason: 'worn from build');
    expect(scene.cubes[0].color, dark.primary.toFynkel());
    world.theme = labTheme(Brightness.light);
    final light = labTheme(Brightness.light).colorScheme;
    expect(scene.camera.clearColor, light.surface.toFynkel());
    expect(scene.floor.color, light.outlineVariant.toFynkel());
    expect(scene.cubes[2].color, light.tertiary.toFynkel());
    expect(scene.readout.value, startsWith('light'));
  });

  test('Widgets in the world: the basics: every cube has its tag; hidden, its tag goes; a click pops a +1', () async {
    final world = World(source: files);
    addTearDown(world.dispose);
    final scene = world.add(WorldWidgetsBasicsScene());
    await scene.loaded;
    world.update(1 / 60);
    expect(scene.cubes.every((c) => c.has<WorldWidget>()), isTrue);
    final [hide] = scene.controls.cast<Toggle>();
    hide.onChanged(true);
    world.update(1 / 60);
    expect(scene.cubes[0].get<WorldWidget>()!.enabled, isTrue, reason: 'on, but its object is inactive');
    expect(scene.cubes[0].activeInHierarchy, isFalse);
    hide.onChanged(false);
    world.input.simulate.resize(400, 300);
    world.update(1 / 60);
    final on = world.camera!.worldToScreenPoint(scene.cubes[1].position)!;
    world.input.simulate
      ..fingerDown(1, on.x, on.y)
      ..fingerUp(1, on.x, on.y);
    world.update(1 / 60);
    final pop = world.all<Pop>().single;
    expect(pop.has<WorldWidget>(), isTrue);
    for (var i = 0; i < 60; i++) {
      world.update(1 / 60);
    }
    expect(pop.isDestroyed, isTrue, reason: 'risen, and gone');
  });

  test('Shapes: the basics: six shapes, the sphere as light as Unity\'s, and the quad\'s back on and off', () {
    final world = World(source: files);
    addTearDown(world.dispose);
    final scene = world.add(ShapesBasicsScene());
    world.update(1 / 60);
    expect(scene.shapes.map((s) => s.name), ['cube', 'sphere', 'capsule', 'cylinder', 'plane', 'quad']);
    expect(scene.readout.value, allOf(contains('cube 12'), contains('sphere 720'), contains('quad 2')));
    scene.controls.whereType<Toggle>().single.onChanged(true);
    expect(scene.quadMaterial.doubleSided, isTrue);
  });

  test('Pooling: the basics: the fountain reuses its balls, and the handle lets go of one that went back', () {
    final world = World(source: files);
    addTearDown(world.dispose);
    final scene = world.add(PoolingBasicsScene());
    void frames(int n) {
      for (var i = 0; i < n; i++) {
        world.update(1 / 60);
      }
    }

    frames(5);
    final first = scene.followed!.object!;
    frames(240);
    final made = scene.made;
    expect(made, inInclusiveRange(20, 25), reason: 'a ball every 0.08 s, each back after 1.6 s');
    expect(scene.balls.countAll, made);
    expect(scene.followed!.object, isNot(same(first)));
    expect(scene.log.value.map((e) => e.text), contains('the ball the cube followed went back: its handle is null'));
    frames(240);
    expect(scene.made, made, reason: 'none made once the pool has enough');
    final [usePool, ...] = scene.controls;
    (usePool as Toggle).onChanged(false);
    frames(120);
    expect(scene.made, greaterThan(made + 10));
    expect(world.all<Ball>().length, lessThan(25), reason: 'the ones made without the pool are destroyed');
  });

  test('Moving: the basics: forward goes the cube\'s way, and off and on the table it stays put', () async {
    final world = World();
    addTearDown(world.dispose);
    final scene = world.add(MovingBasicsScene());
    await scene.loaded;
    final [forward, forwardInWorld, ..., off, on, _, startOver] = scene.controls.cast<Button>();
    final cube = scene.cube;
    final way = cube.forward;
    final start = cube.position;
    forward.onPressed();
    expect(cube.position.closeTo(start + way, 1e-9), isTrue, reason: 'the way it looks, turned with the table');
    forwardInWorld.onPressed();
    expect(cube.position.closeTo(start + way + Vec3.forward, 1e-9), isTrue);
    final at = cube.position;
    off.onPressed();
    expect(cube.parent, scene);
    expect(cube.position.closeTo(at, 1e-9), isTrue, reason: 'parent = keeps where it is');
    on.onPressed();
    expect(cube.parent, scene.table);
    expect(cube.position.closeTo(at, 1e-9), isTrue);
    startOver.onPressed();
    expect(cube.localPosition, const Vec3(0, 0.75, 0));
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
    expect(world.camera!.cullingMask, LayerMask.except([blueLayer]));
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
    expect(scene.hinge.angle.inDegrees.abs(), greaterThan(20), reason: 'pushed open');
    frames(600);
    expect(scene.hinge.angle.inDegrees.abs(), lessThan(3), reason: 'its spring swung it shut');
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

  group('Joints: a chain and a weight', () {
    late World world;
    late JointsChainScene scene;
    void frames(int n) {
      for (var i = 0; i < n; i++) {
        world.update(1 / 60);
      }
    }

    setUp(() {
      world = World(source: files);
      addTearDown(world.dispose);
      scene = world.add(JointsChainScene());
      world.input.simulate.resize(400, 300);
      frames(300);
    });

    test('it hangs straight down from the hook, stretched a little by the weight', () {
      final length = JointsChainScene.count * JointsChainScene.linkLength + JointsChainScene.radius;
      expect(scene.weight!.position.x.abs(), lessThan(0.01));
      expect(JointsChainScene.hook.y - scene.weight!.position.y, closeTo(length, 0.1));
      expect(scene.stretch.total, lessThan(0.1), reason: '1.5 kg on 0.2 kg links, at 6 iterations');
    });

    test('a finger drags the weight across, and lets go of it', () {
      final weight = scene.weight!;
      // Where the weight is on the view: aim at it until a ray hits it.
      ViewPoint? on;
      for (var y = 0.0; y < 300 && on == null; y += 2) {
        final p = ViewPoint(200, y, width: 400, height: 300);
        if (world.physics.raycast(scene.camera.screenPointToRay(p))?.gameObject == weight) on = p;
      }
      world.input.simulate.fingerDown(1, on!.x, on.y);
      frames(1);
      expect(scene.held?.$1.gameObject, weight);
      for (var i = 1; i <= 30; i++) {
        world.input.simulate.fingerMove(1, on.x + i * 3, on.y);
        frames(1);
      }
      frames(60);
      expect(weight.position.x, greaterThan(0.5), reason: 'it followed the finger to the right');
      world.input.simulate.fingerUp(1, on.x + 90, on.y);
      frames(1);
      expect(scene.held, isNull);
      expect(weight.get<SpringJoint>(), isNull, reason: 'the spring to the hand is taken off');
      frames(60);
      expect(weight.position.x, lessThan(0.5), reason: 'let go, it swings back');
    });

    test('the held weight follows the finger closely, going round', () {
      final weight = scene.weight!;
      final p = scene.camera.worldToScreenPoint(weight.position)!;
      world.input.simulate.fingerDown(1, p.x, p.y);
      frames(1);
      // A circle 40 px across, once round in two seconds, near where it hangs.
      var worst = 0.0;
      for (var i = 0; i < 240; i++) {
        final a = i / 60 * math.pi;
        world.input.simulate.fingerMove(1, p.x + 20 * math.sin(a), p.y - 20 + 20 * math.cos(a));
        frames(1);
        final held = weight.transformPoint(scene.held!.$2.anchor);
        if (i > 30) worst = math.max(worst, (scene.hand.gameObject.position - held).length);
      }
      expect(worst, lessThan(0.06), reason: 'within a few centimetres of the finger');
    });

    test('heavy, the chain stretches between its links; more solver iterations hold it', () {
      final slider = scene.controls.whereType<Slider>().firstWhere((s) => s.label == 'the weight (kg)');
      slider.onChanged(50);
      frames(300);
      expect(scene.stretch.total, greaterThan(0.05), reason: '250× a link, at 6 iterations');
      scene.controls.whereType<Slider>().firstWhere((s) => s.label == 'solver iterations').onChanged(30);
      frames(300);
      expect(scene.stretch.total, lessThan(0.02));
      scene.destroy();
      frames(1);
      expect(world.physics.defaultSolverIterations, 6, reason: 'put back for the next scene');
      expect(world.time.fixedDelta, 1 / 50);
    });
  });

  test('Joints: a character joint: the limited arm stops at its 40°, the free one swings on', () {
    (double, double) swing(String button, {double? limit}) {
      final world = World(source: files);
      addTearDown(world.dispose);
      final scene = world.add(JointsCharacterScene());
      if (limit != null) scene.controls.whereType<Slider>().single.onChanged(limit);
      for (var i = 0; i < 30; i++) {
        world.update(1 / 60);
      }
      scene.controls.whereType<Button>().firstWhere((b) => b.label == button).onPressed();
      var (limited, free) = (0.0, 0.0);
      for (var i = 0; i < 90; i++) {
        world.update(1 / 60);
        limited = math.max(limited, JointsCharacterScene.swing(scene.limited));
        free = math.max(free, JointsCharacterScene.swing(scene.free));
      }
      return (limited, free);
    }

    for (final button in ['swing them sideways', 'swing them forward']) {
      final (limited, free) = swing(button);
      expect(limited, inInclusiveRange(35, 45), reason: '$button: stopped at 40°');
      expect(free, greaterThan(60), reason: '$button: free, it swings on');
    }
    final (freed, _) = swing('swing them sideways', limit: 180);
    expect(freed, greaterThan(60), reason: 'its limits at 180°, it swings as the free one');
  });

  test('Colour: the basics: a click flashes one cube alone, and the shared colour reaches the others', () async {
    final world = World(source: files);
    addTearDown(world.dispose);
    final scene = world.add(ColourBasicsScene());
    await scene.loaded;
    world.input.simulate.resize(400, 300);
    world.update(1 / 60);
    final [first, second, middle, _, last] = scene.cubes;
    final on = world.camera!.get<Camera>()!.worldToScreenPoint(first.position)!;
    world.input.simulate
      ..fingerDown(1, on.x, on.y)
      ..fingerUp(1, on.x, on.y);
    world.update(1 / 60);
    expect(first.renderer.color, isNotNull, reason: 'flashing');
    expect(second.renderer.color, isNull);
    expect(scene.shared.color, const Color(0xFF4CA6E0), reason: 'the material is untouched');
    for (var i = 0; i < 30; i++) {
      world.update(1 / 60);
    }
    expect(first.renderer.color, isNull, reason: 'the material\'s again');

    final [tint as Toggle, change as Button, own as Toggle] = scene.controls;
    tint.onChanged(true);
    own.onChanged(true);
    change.onPressed();
    expect(middle.renderer.color, const Color(0xFFE05A5A), reason: 'its own colour holds');
    expect(last.renderer.material, isNot(same(scene.shared)));
    expect(last.renderer.material!.color, const Color(0xFFFFD040), reason: 'its own material is its own');
    expect(scene.shared.color, isNot(const Color(0xFF4CA6E0)));
  });

  test('Riding a platform: the player rides the sliding platform, and stays put with moveWithGround off', () async {
    final world = World(source: files);
    addTearDown(world.dispose);
    final scene = world.add(RidingBasicsScene());
    await scene.loaded;
    void frames(int n) {
      for (var i = 0; i < n; i++) {
        world.update(1 / 60);
      }
    }

    frames(30);
    expect(scene.player.controller.ground?.gameObject, same(scene.slider));
    final offset = scene.player.position - scene.slider.position;
    frames(60);
    expect((scene.player.position - scene.slider.position - offset).length, lessThan(0.05), reason: 'rode along');
    expect(scene.slider.position.z, lessThan(-1), reason: 'and it did move');

    (scene.controls.first as Toggle).onChanged(false);
    final at = scene.player.position;
    frames(30);
    expect(Vec3(at.x, 0, at.z).closeTo(Vec3(scene.player.position.x, 0, scene.player.position.z), 0.02), isTrue);
  });

  test('Add-ons: the basics: the add-on counts components, and the invisible button takes clicks', () async {
    final world = World(source: files);
    addTearDown(world.dispose);
    final scene = world.add(AddonsBasicsScene());
    await scene.loaded;
    world.input.simulate.resize(400, 300);
    world.update(1 / 60);
    final [add as Button, remove as Button, _, taps as Toggle] = scene.controls;
    final (added, removed) = (scene.counter.added, scene.counter.removed);
    add.onPressed();
    add.onPressed();
    remove.onPressed();
    expect(scene.counter.added - added, 4, reason: 'two cubes, a renderer and a collider each');
    expect(scene.counter.removed - removed, 2);

    void click() {
      final on = world.camera!.get<Camera>()!.worldToScreenPoint(scene.button.position)!;
      world.input.simulate
        ..fingerDown(1, on.x, on.y)
        ..fingerUp(1, on.x, on.y);
      world.update(1 / 60);
    }

    click();
    expect(scene.button.clicks, 1, reason: 'a collider with nothing drawn');
    taps.onChanged(false);
    click();
    expect(scene.button.clicks, 1, reason: 'colliders take no taps now');
  });
}
