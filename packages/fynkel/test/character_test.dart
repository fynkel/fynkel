// The character controller and third-person camera from Dart (M3-09,
// blueprint §19.2d item 9): `move`/`jump` commands and `characterState()`
// on the M3-08 body path, the runtime's own action driver and camera rig
// on the fixture's Playground scene, and the `PlayerController` example
// behaviour as the pattern a game writes when it wants its own rules.

import 'dart:io';
import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

import '../example/player_controller.dart';

const playground = 'c0000000-0000-4000-8000-000000000031';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-character-cache-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    return w;
  }

  /// The Playground: Floor, a Platform 0.8 m high at z −4.5, the Player
  /// (a controller driven by `move`/`jump`) and the Camera rig on it.
  (World, GameObject, GameObject) loaded() {
    final w = fresh();
    w.onLoadDiagnostics = (_) {};
    final report = w.loadScene(playground, cacheDir: cache);
    expect(report.ok, isTrue, reason: '$report');
    // The project's actions come with the cache index.
    expect(report.inputActions.map((a) => a.name), ['move', 'jump', 'look', 'pause']);
    return (w, w.find('Player')!, w.find('Camera')!);
  }

  /// A 20×1×20 slab whose top is y = 0.
  GameObject floor(World w) => w.spawn(name: 'floor')
    ..add(const Collider(
        shape: ColliderShape.box,
        size: Vec3(10, 0.5, 10),
        offset: Vec3(0, -0.5, 0),
        friction: 0.6,
        restitution: 0,
        isSensor: false,
        layer: 1,
        mask: 0xffffffff));

  /// A controller nothing drives: the game sends `move`/`jump`.
  const bare = CharacterController(
      radius: 0.4,
      height: 1.8,
      center: Vec3(0, 0.9, 0),
      skinWidth: 0.02,
      maxSlope: 0.7853982,
      maxStepHeight: 0.3,
      snapToGround: 0.2,
      gravityScale: 1,
      mass: 70,
      speed: 4,
      jumpSpeed: 5,
      turnSpeed: 10,
      moveAction: '',
      jumpAction: '',
      layer: 1,
      mask: 0xffffffff);

  test('the recorded walk lands the Player on the Platform and the rig follows', () {
    final (w, player, camera) = loaded();
    expect(player.get<CharacterController>()!.moveAction, 'move');
    expect(camera.get<ThirdPersonCamera>()!.target, 'Player');
    final recording = loadInputRecording('$fixture/test/platform.input.json');
    var airborne = 0;
    recording.replay(w, fixedUpdate: (_) {
      if (!(player.characterState()?.grounded ?? true)) airborne++;
    });
    expect(airborne, inInclusiveRange(40, 60), reason: 'one jump of ~0.8 s');
    final state = player.characterState()!;
    expect(state.grounded, isTrue);
    expect(state.sliding, isFalse);
    expect(state.velocity, Vec3.zero, reason: 'W released');
    final p = player.localTransform.position;
    expect(p.y, closeTo(0.82, 5e-3), reason: 'skinWidth above the 0.8 m top');
    expect(p.z, inInclusiveRange(-6, -3));
    expect(p.x.abs(), lessThan(1e-3));
    // The rig: pitch 0.35 and 5 m from the pivot 1.5 m above the feet,
    // yaw/pitch held in the component.
    final rig = camera.get<ThirdPersonCamera>()!;
    expect(rig.yaw, 0);
    expect(rig.pitch, closeTo(0.35, 1e-6));
    final c = camera.localTransform.position;
    final pivot = p + const Vec3(0, 1.5, 0);
    expect((c - pivot).length, closeTo(5, 1e-3));
    expect(c.z, greaterThan(p.z), reason: 'behind the player');
    expect(c.y, closeTo(pivot.y + 5 * math.sin(0.35), 1e-3));
    // The rig turns with `look` (stick x = 1 for one tick).
    w.frame(1 / 60, input: const [InputEvent.axis(InputDevice.gamepad, 'rightStick', 1, 0)]);
    expect(camera.get<ThirdPersonCamera>()!.yaw, closeTo(-2.5 / 60, 1e-5));
    // Same recording, same landing.
    final again = loaded();
    recording.replay(again.$1);
    expect(again.$2.localTransform.position, p);
  });

  test('move and jump are commands, the state is a query, mistakes are diagnostics', () {
    final w = fresh();
    final ground = floor(w);
    final hero = w.spawn(name: 'hero')..add(bare);
    w.frame(1 / 60); // the controller exists; a fresh one stands still
    expect(hero.characterState(), isNotNull);
    expect(ground.characterState(), isNull);
    expect(hero.bodyState(), isNull, reason: 'a character is not a rigid body');
    for (var i = 0; i < 30; i++) {
      hero.move(const Vec3(3, 0, 0));
      w.frame(1 / 60);
    }
    var s = hero.characterState()!;
    expect(s.grounded, isTrue);
    expect(s.velocity.x, closeTo(3, 1e-3));
    expect(hero.localTransform.position.x, closeTo(30 * 3 / 60, 1e-2));
    // Facing +X (turned from −Z).
    final facing = hero.localTransform.rotation.rotate(const Vec3(0, 0, -1));
    expect(facing.x, closeTo(1, 1e-3));
    // No move: standing still. A jump lifts; gravity brings it back.
    w.frame(1 / 60);
    expect(hero.characterState()!.velocity, Vec3.zero);
    hero.jump();
    w.frame(1 / 60);
    s = hero.characterState()!;
    expect(s.grounded, isFalse);
    expect(s.velocity.y, closeTo(5, 1e-3), reason: 'gravity from the next tick');
    var ticks = 0;
    while (!hero.characterState()!.grounded) {
      w.frame(1 / 60);
      expect(++ticks, lessThan(90));
    }
    expect(ticks, inInclusiveRange(55, 65), reason: '2·5/9.81 s in the air');
    expect(hero.localTransform.position.y, closeTo(0.02, 5e-3));
    hero.jump(2);
    w.frame(1 / 60);
    expect(hero.characterState()!.velocity.y, closeTo(2, 1e-3));

    w.throwOnDiagnostics = false;
    final seen = <FynkelDiagnostic>[];
    w.onDiagnostics = seen.addAll;
    ground.move(Vec3.up); // no controller
    hero.applyImpulse(Vec3.up); // a character takes no body command
    hero.move(const Vec3(double.nan, 0, 0)); // not finite
    w.frame(1 / 60);
    expect(seen.map((d) => d.code),
        [DiagnosticCode.missingComponent, DiagnosticCode.missingComponent, DiagnosticCode.invalidPayload]);
    expect(seen.map((d) => d.component),
        [CharacterController.componentId, RigidBody.componentId, CharacterController.componentId]);
    seen.clear();
    // A RigidBody next to the controller is reported once and ignored.
    hero.add(const RigidBody(
        bodyType: BodyType.dynamic, mass: 1, linearDamping: 0, angularDamping: 0, gravityScale: 1, ccd: false));
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(seen.map((d) => (d.code, d.component)), [(DiagnosticCode.invalidPayload, CharacterController.componentId)]);
    expect(hero.characterState(), isNotNull);
    // Removing the controller ends the character; the body takes over.
    hero.remove<CharacterController>();
    w.frame(1 / 60);
    expect(hero.characterState(), isNull);
    expect(hero.bodyState(), isNotNull);
    hero.destroy();
    w.frame(1 / 60);
    expect(() => hero.move(Vec3.up), throwsStateError, reason: 'destroyed');
  });

  test('the PlayerController example walks in the rig\'s frame and double-jumps', () {
    final (w, player, camera) = loaded();
    // Hand the Player to the behaviour: the runtime's driver steps aside
    // when the controller names no actions.
    player.set(player.get<CharacterController>()!.copyWith(moveAction: '', jumpAction: ''));
    final pc = player.addBehaviour(PlayerController()..walkSpeed = 3);
    w.frame(1 / 60);
    // Turn the rig a quarter turn to the left over a second, then walk
    // "forward": the player goes where the camera looks.
    for (var i = 0; i < 60; i++) {
      w.frame(1 / 60, input: const [InputEvent.axis(InputDevice.gamepad, 'rightStick', -math.pi / 2 / 2.5, 0)]);
    }
    final yaw = camera.get<ThirdPersonCamera>()!.yaw;
    expect(yaw, closeTo(math.pi / 2, 1e-3));
    w.frame(1 / 60, input: const [
      InputEvent.axis(InputDevice.gamepad, 'rightStick', 0, 0),
      InputEvent.down(InputDevice.keyboard, 'W'),
    ]);
    final start = player.localTransform.position;
    for (var i = 0; i < 30; i++) {
      w.frame(1 / 60);
    }
    final moved = player.localTransform.position - start;
    expect(moved.x, closeTo(-30 * 3 / 60, 1e-2), reason: 'forward at yaw π/2 is −X');
    expect(moved.z.abs(), lessThan(1e-2));
    expect(player.characterState()!.velocity.x, closeTo(-3, 1e-3));
    // Jump, then jump again in the air; a third press does nothing.
    w.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'Space')]);
    expect(player.characterState()!.grounded, isFalse);
    expect(pc.jumpsLeft, 1);
    w.frame(1 / 60, input: const [InputEvent.up(InputDevice.keyboard, 'Space')]);
    for (var i = 0; i < 10; i++) {
      w.frame(1 / 60);
    }
    expect(player.characterState()!.velocity.y, lessThan(3.5), reason: 'slowing');
    w.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'Space')]);
    expect(player.characterState()!.velocity.y, closeTo(0.8 * 5, 1e-3), reason: 'the weaker second jump');
    expect(pc.jumpsLeft, 0);
    w.frame(1 / 60, input: const [InputEvent.up(InputDevice.keyboard, 'Space')]);
    w.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'Space')]);
    expect(pc.jumpsLeft, 0);
    var ticks = 0;
    while (!player.characterState()!.grounded) {
      w.frame(1 / 60, input: const [InputEvent.up(InputDevice.keyboard, 'W')]);
      expect(++ticks, lessThan(200));
    }
    w.frame(1 / 60); // the behaviour sees the landing one tick later
    expect(pc.jumpsLeft, 2, reason: 'refilled on the ground');
  });
}
