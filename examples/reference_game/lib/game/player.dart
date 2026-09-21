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
/// Its rigged character (tools/character.py) is posed from the same
/// state: the `fynkel.Animator` the scene puts on the player starts on
/// [idleAnim], and every tick this behaviour crossfades to the clip the
/// controller's state calls for — walk or run on the ground with the stick
/// moving, jump (one-shot) on the jump press — and back to idle. The clips
/// are authored in place: the controller moves the entity, the animator
/// only poses the joints (docs/animation.md).
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

  /// Seconds a clip change blends over.
  @Expose(min: 0, step: 0.05)
  double blend = 0.15;

  /// The clips' sub-asset names on the character the scene's animator
  /// names: `anim/[index]` for a model whose file carries them (the box
  /// character), `anim/[name]` for a rig document (M3-19b; `--hero
  /// [name].rig.json` sets these). So the model can be swapped without
  /// touching the code.
  @Expose()
  String idleAnim = 'anim/0';
  @Expose()
  String walkAnim = 'anim/1';
  @Expose()
  String runAnim = 'anim/2';
  @Expose()
  String jumpAnim = 'anim/3';

  /// The clips resolved against the animator's asset.
  late AssetRef idleClip, walkClip, runClip, jumpClip;

  /// The clip last chosen (what the animator plays).
  AssetRef? clip;

  /// Ticks the player has been in the air (for the HUD's readout and the
  /// tests).
  int jumps = 0;
  double _elapsed = 0;
  bool _dead = false;

  GameState get state => context.service<GameState>();

  @override
  void start() {
    spawn = transform.position;
    final character = get<Animator>()?.clip.asset ?? AssetRef.none.asset;
    idleClip = AssetRef(character, sub: idleAnim);
    walkClip = AssetRef(character, sub: walkAnim);
    runClip = AssetRef(character, sub: runAnim);
    jumpClip = AssetRef(character, sub: jumpAnim);
    clip = idleClip; // the scene's animator starts there
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
    final jumped = Input.pressed('jump') && me.grounded;
    if (jumped) {
      jump(jumpSpeed);
      jumps++;
    }
    pose(jumped: jumped, grounded: me.grounded, moving: stick.length > 0.05, sprinting: Input.held('sprint'));

    if (!state.won.value) {
      _elapsed += context.dt;
      final tenths = (_elapsed * 10).floor();
      if (tenths != state.elapsedTenths.value) state.elapsedTenths.value = tenths;
    }
  }

  /// Picks the clip for this tick's state and crossfades to it when it
  /// changes. The jump plays once and holds its last frame until the
  /// player lands (the controller is airborne longer than the clip), so
  /// in the air nothing is chosen over it.
  void pose({required bool jumped, required bool grounded, required bool moving, required bool sprinting}) {
    if (jumped) {
      clip = jumpClip;
      animation.play(jumpClip, looping: false);
      return;
    }
    if (!grounded && clip == jumpClip) return;
    final next = !moving
        ? idleClip
        : sprinting
            ? runClip
            : walkClip;
    if (next == clip) return;
    clip = next;
    animation.crossfade(next, blend);
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
