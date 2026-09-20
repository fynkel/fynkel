import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import 'game.dart';
import 'game_state.dart';

/// The player: walks its `fynkel.CharacterController` from the `move`
/// action in the frame of the `fynkel.ThirdPersonCamera` named
/// [cameraName], sprints while `sprint` is held, jumps on the `jump` press
/// while grounded, dies on a `hazard` zone or below [fallY] (respawns at
/// the start), restarts the run on `restart`, and keeps the clock.
///
/// The controller's own `moveAction`/`jumpAction` are left empty in the
/// scene so this behaviour is the one mover (a behaviour's `move` in the
/// same tick would win anyway; docs/physics.md).
@FynkelBehaviour(id: 'game.Player')
class Player extends Behaviour {
  /// Metres per second on the ground.
  @Expose(min: 0, step: 0.1)
  double walkSpeed = 4;

  /// Multiplier while the `sprint` action is held.
  @Expose(min: 1, step: 0.1)
  double sprintFactor = 1.75;

  /// Upward speed of a jump.
  @Expose(min: 0, step: 0.1)
  double jumpSpeed = 5.5;

  /// Name of the camera rig entity (`fynkel.ThirdPersonCamera` on it).
  @Expose()
  String cameraName = 'Camera';

  /// Falling below this height is a death.
  @Expose(step: 0.5)
  double fallY = -6;

  /// Where a death puts the player back: the scene position.
  late Vec3 spawn;

  /// Ticks the player has been in the air (for the HUD's readout and the
  /// tests).
  int jumps = 0;
  double _elapsed = 0;
  bool _dead = false;

  GameState get state => context.service<GameState>();

  @override
  void start() {
    spawn = transform.position;
  }

  @override
  void fixedUpdate(FixedUpdateContext context) {
    if (Input.pressed('restart')) {
      // The reload happens at the next frame; this tick finishes as usual.
      restartRun(world);
      return;
    }
    final me = characterState();
    if (me == null) return; // not spawned yet, or no controller

    // Fell off the world: back to the start.
    if (transform.position.y < fallY) die();
    if (_dead) {
      // The teleport landed this tick; stand still for it.
      _dead = false;
      move(Vec3.zero);
      return;
    }

    // The rig's yaw gives "forward": the camera looks along −Z rotated by
    // yaw, so forward = (−sin yaw, 0, −cos yaw) and right = (cos yaw, 0,
    // −sin yaw). Without a rig the world axes do.
    final rig = world.find(cameraName)?.get<ThirdPersonCamera>();
    final yaw = rig?.yaw ?? 0;
    final forward = Vec3(-math.sin(yaw), 0, -math.cos(yaw));
    final right = Vec3(math.cos(yaw), 0, -math.sin(yaw));

    var stick = Input.action2D('move');
    if (stick.length > 1) stick = stick.normalized;
    final speed = walkSpeed * (Input.held('sprint') ? sprintFactor : 1);
    move((right * stick.x + forward * stick.y) * speed);

    // The press edge reaches exactly one tick (§5.1).
    if (Input.pressed('jump') && me.grounded) {
      jump(jumpSpeed);
      jumps++;
    }

    if (!state.won.value) {
      _elapsed += context.dt;
      final tenths = (_elapsed * 10).floor();
      if (tenths != state.elapsedTenths.value) state.elapsedTenths.value = tenths;
    }
  }

  /// A hazard's zone (the sweeper's blade, a launched ball) touched us.
  @override
  void onZoneEnter(ZoneEvent event) {
    if (event.event == 'hazard') die();
  }

  /// Back to the spawn point; the controller is not physics-owned, so a
  /// transform write teleports it (docs/physics.md).
  void die() {
    if (_dead) return;
    _dead = true;
    state.deaths.value += 1;
    transform.position = spawn;
  }
}
