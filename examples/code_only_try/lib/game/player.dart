import 'dart:math' as math;

import 'package:fynkel/gameplay.dart';

import '../generated/assets.g.dart';
import 'flow.dart';
import 'game_state.dart';

/// The player: walks its `fynkel.CharacterController` from the `move`
/// action in the frame of the camera named [cameraName], jumps on the
/// `jump` press while grounded, dies on a `hazard` zone or below [fallY]
/// (back to where it started), restarts the run on `restart`, keeps the
/// clock and, [winDelay] seconds after the last coin, ends the run.
@FynkelBehaviour(id: 'game.Player')
class Player extends Behaviour {
  /// Metres per second on the ground.
  @Expose(min: 0, step: 0.1)
  double walkSpeed = 4;

  /// Upward speed of a jump.
  ///
  /// The editor shows how high it reaches (lib/editor/'s `jumpHeight`
  /// drawer).
  @Expose(min: 0, step: 0.1, drawer: 'jumpHeight')
  double jumpSpeed = 5.5;

  /// The object with the `fynkel.ThirdPersonCamera`.
  @Expose()
  String cameraName = 'Camera';

  /// Falling below this height is a death.
  @Expose(step: 0.5)
  double fallY = -6;

  /// Where a death puts the player back.
  late Vec3 spawn;

  int jumps = 0;
  double _elapsed = 0;
  double _sinceWin = 0;
  bool _dead = false;
  bool _finished = false;

  GameState get state => context.service<GameState>();
  Flow get flow => context.service<Flow>();

  @override
  void start() {
    spawn = transform.position;
  }

  @override
  void fixedUpdate(FixedUpdateContext context) {
    if (Input.pressed('restart')) {
      flow.play();
      return;
    }
    final me = characterState();
    if (me == null) return;

    if (transform.position.y < fallY) die();
    if (_dead) {
      // The teleport landed this tick; stand still for it.
      _dead = false;
      move(Vec3.zero);
      return;
    }

    // Forward is where the camera looks: −Z turned by its yaw.
    final yaw = world.find(cameraName)?.get<ThirdPersonCamera>()?.yaw ?? 0;
    final forward = Vec3(-math.sin(yaw), 0, -math.cos(yaw));
    final right = Vec3(math.cos(yaw), 0, -math.sin(yaw));
    var stick = Input.action2D('move');
    if (stick.length > 1) stick = stick.normalized;
    move((right * stick.x + forward * stick.y) * walkSpeed);

    if (Input.pressed('jump') && me.grounded) {
      jump(jumpSpeed);
      jumps++;
      world.audio.play(Assets.audio.jump, volume: 0.5);
    }

    if (!state.won.value) {
      _elapsed += context.dt;
      final tenths = (_elapsed * 10).floor();
      if (tenths != state.elapsedTenths.value) state.elapsedTenths.value = tenths;
    } else if (!_finished) {
      // The win banner's moment, counted in ticks so a replay ends the
      // run on the same tick every time.
      _sinceWin += context.dt;
      if (_sinceWin >= winDelay) {
        _finished = true;
        flow.finish();
      }
    }
  }

  /// The sweeper's blade touched us.
  @override
  void onZoneEnter(ZoneEvent event) {
    if (event.event == 'hazard') die();
  }

  /// Back to the spawn point: the controller is not physics-owned, so a
  /// transform write teleports it.
  void die() {
    if (_dead) return;
    _dead = true;
    state.deaths.value += 1;
    transform.position = spawn;
    world.audio.play(Assets.audio.hit, volume: 0.8, priority: 1);
  }
}
