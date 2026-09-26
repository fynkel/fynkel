import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import 'game.dart';
import 'game_state.dart';

/// The player: walks its `fynkel.CharacterController` from the `move`
/// action in the frame of the `fynkel.ThirdPersonCamera` named
/// [cameraName], sprints while `sprint` is held, jumps on the `jump` press
/// while grounded, dies on a `hazard` zone or below [fallY] (respawns at
/// the start), restarts the run on `restart`, keeps the clock and, [winDelay]
/// after the last coin, ends the run (`finishRun`: the Results scene).
///
/// Its rigged character (tools/character.py) is posed from the same
/// state: the `fynkel.Animator` on the player runs an Animator Controller
/// (ADR-0032; assets/models/player.controller.json) whose Locomotion state
/// plays the blend space (assets/models/player.blend.json: idle, walk, run
/// along `speed`) and whose Jump state plays the jump once. Every tick
/// this behaviour only sets the parameters — `speed` to the controller's
/// planar velocity, `grounded`, and the `jump` trigger on a jump — and
/// the controller does the rest: the stride follows the speed,
/// phase-synced, and the landing goes back to Locomotion. The `wave`
/// action plays [wave] once as a one-shot in the controller's `UpperBody`
/// slot (ADR-0033 § 2: a layer with no states, rooted at the spine), so
/// the arm waves while the legs keep walking. Footstep markers of the walk and
/// the run (`step_l`/`step_r`) count into [GameState.footsteps] and play
/// [stepLeft]/[stepRight] as one-shots (M3-11: `world.audio.play`, no
/// source on the entity; the pitch wanders a little with the count so a
/// run does not sound like a metronome); a death plays [hit]. The
/// clips are authored in place: the controller moves the entity, the
/// animator only poses the joints (docs/animation.md).
///
/// Holding `aim` (the right mouse button) is the aim mode (ADR-0033): the
/// player faces where the camera looks — the controller's `turnSpeed` is
/// 0 meanwhile and this behaviour turns it — and the Aim state strafes on
/// a Freeform Directional blend tree over `moveX`/`moveY`, the planar
/// velocity in the player's own frame.
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

  /// The clip the `wave` action plays in the `UpperBody` slot (none when
  /// the character has no wave).
  @Expose()
  AssetRef wave = AssetRef.none;

  /// The footstep sounds, one per `step_l`/`step_r` marker (assets/audio;
  /// none when unset: the markers still count).
  @Expose()
  AssetRef stepLeft = AssetRef.none;
  @Expose()
  AssetRef stepRight = AssetRef.none;

  /// The sound of a death (a hazard or the fall).
  @Expose()
  AssetRef hit = AssetRef.none;

  /// Gain of a footstep (`0…1`).
  @Expose(min: 0, max: 1, step: 0.05)
  double stepVolume = 0.6;

  /// Ticks the player has been in the air (for the HUD's readout and the
  /// tests).
  int jumps = 0;

  /// Whether the aim mode is on (for the tests).
  bool aiming = false;

  /// The controller's `turnSpeed` before the aim mode set it to 0.
  double _turnSpeed = 0;
  double _elapsed = 0;
  double _sinceWin = 0;
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

    aim(Input.held('aim'), yaw);

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
    pose(jumped: jumped, grounded: me.grounded, velocity: me.velocity);
    if (Input.pressed('wave')) waveOnce();

    if (!state.won.value) {
      _elapsed += context.dt;
      final tenths = (_elapsed * 10).floor();
      if (tenths != state.elapsedTenths.value) state.elapsedTenths.value = tenths;
    } else {
      // The win banner's moment, then the Results scene (M3-15): counted
      // in ticks, so a replay ends the run on the same tick every time.
      _sinceWin += context.dt;
      if (_sinceWin >= winDelay && world.scenes.state != SceneState.pending) finishRun(world);
    }
  }

  /// Turns the aim mode on or off, and while it is on faces the camera's
  /// [yaw].
  void aim(bool on, double yaw) {
    if (on != aiming) {
      final controller = get<CharacterController>();
      if (controller != null) {
        if (on) _turnSpeed = controller.turnSpeed;
        set(controller.copyWith(turnSpeed: on ? 0 : _turnSpeed));
      }
      aiming = on;
    }
    if (aiming) transform.rotation = Quat.axisAngle(Vec3.up, yaw);
  }

  /// The Animator Controller's parameters: `speed` is the character
  /// controller's planar speed as the last tick applied it (so a wall
  /// stops the stride with the player), `grounded` whether it stands, and
  /// a jump sets the `jump` trigger. The controller plays the jump once,
  /// holds its last frame in the air and goes back to Locomotion when
  /// `grounded` is true again. `aim` and the velocity in the player's
  /// frame (`moveX` right, `moveY` forward) drive the Aim state.
  void pose({required bool jumped, required bool grounded, required Vec3 velocity}) {
    final facing = transform.rotation;
    final forward = facing.rotate(const Vec3(0, 0, -1));
    final right = facing.rotate(const Vec3(1, 0, 0));
    animation
      ..setFloat('speed', math.sqrt(velocity.x * velocity.x + velocity.z * velocity.z))
      ..setBool('grounded', grounded && !jumped)
      ..setBool('aim', aiming)
      ..setFloat('moveX', velocity.x * right.x + velocity.z * right.z)
      ..setFloat('moveY', velocity.x * forward.x + velocity.z * forward.z);
    if (jumped) animation.setTrigger('jump');
  }

  /// Plays [wave] once over the upper body; a wave already playing
  /// blends into the new one.
  void waveOnce() {
    if (wave.isNone) return;
    animation.playOneShot(wave, slot: 'UpperBody', blendIn: 0.15, blendOut: 0.25);
    waves++;
  }

  /// Waves played (for the HUD's readout and the tests).
  int waves = 0;

  /// A footstep of the walk or the run (their `step_l`/`step_r` markers,
  /// from whichever clip dominates the blend at the moment): counted,
  /// and its sound played once — the left or the right file by the
  /// marker's name, a hair sharper every third step.
  @override
  void onAnimationMarker(AnimationMarker event) {
    if (!event.name.startsWith('step')) return;
    final n = state.footsteps.value;
    state.footsteps.value = n + 1;
    final clip = event.name == 'step_r' ? stepRight : stepLeft;
    if (clip == AssetRef.none) return;
    world.audio.play(clip, volume: stepVolume, pitch: 0.95 + 0.05 * (n % 3));
    sounds++;
  }

  /// One-shots this behaviour played (for the tests).
  int sounds = 0;

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
    if (hit != AssetRef.none) {
      world.audio.play(hit, volume: 0.8, priority: 1);
      sounds++;
    }
  }
}
