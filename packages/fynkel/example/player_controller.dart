/// The `PlayerController` pattern (blueprint §3.3, §19.2d item 9): a
/// behaviour that walks a `fynkel.CharacterController` from the `move`
/// action in the frame of the `fynkel.ThirdPersonCamera` that follows it,
/// and jumps on the `jump` press while grounded.
///
/// A controller whose `moveAction`/`jumpAction` are named is walked by the
/// runtime with exactly this logic and needs no behaviour; write one when
/// the game wants its own rules — a stamina-limited sprint, a double jump,
/// a dash — as this example sketches. A `move` from a behaviour in the
/// same tick wins over the runtime's.
library;

import 'dart:math' as math;

import 'package:fynkel/gameplay.dart';

@FynkelBehaviour(id: 'game.PlayerController')
class PlayerController extends Behaviour {
  /// Metres per second on the ground.
  @Expose(min: 0, step: 0.1)
  double walkSpeed = 4;

  /// Multiplier while the `sprint` action is held.
  @Expose(min: 1, step: 0.1)
  double sprintFactor = 1.75;

  /// Upward speed of the first jump; the second is weaker.
  @Expose(min: 0, step: 0.1)
  double jumpSpeed = 5;

  /// Name of the camera rig entity (`fynkel.ThirdPersonCamera` on it).
  @Expose()
  String cameraName = 'Camera';

  int jumpsLeft = 0;

  @override
  void fixedUpdate(FixedUpdateContext context) {
    final state = characterState();
    if (state == null) return; // not spawned yet, or no controller

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

    // The press edge reaches exactly one tick (§5.1); the ground refills
    // the double jump.
    if (state.grounded) jumpsLeft = 2;
    if (Input.pressed('jump') && jumpsLeft > 0) {
      jump(jumpsLeft == 2 ? jumpSpeed : jumpSpeed * 0.8);
      jumpsLeft--;
    }
  }
}
