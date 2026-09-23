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
/// state (M3-19c): the `fynkel.Animator` the scene puts on the player
/// plays the locomotion blend space (assets/models/player.blend.json:
/// idle, walk, run along `speed`) and every tick this behaviour sets
/// `speed` to the controller's planar velocity — the stride follows the
/// speed, phase-synced, with no clip switching. The jump press plays
/// [jumpClip] once over it and the landing crossfades back. The `wave`
/// action plays the wave layer — a second, additive animator on the
/// entity named [waveLayer] — once. Footstep markers of the walk and
/// the run (`step_l`/`step_r`) count into [GameState.footsteps] and play
/// [stepLeft]/[stepRight] as one-shots (M3-11: `world.audio.play`, no
/// source on the entity; the pitch wanders a little with the count so a
/// run does not sound like a metronome); a death plays [hit]. The
/// clips are authored in place: the controller moves the entity, the
/// animator only poses the joints (docs/animation.md).
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

  /// Seconds the landing blends back into the locomotion over.
  @Expose(min: 0, step: 0.05)
  double blend = 0.15;

  /// The jump, played once over the locomotion: `anim/jump` of the box
  /// character, `anim/jump` of a rig document (`--hero` sets it), so the
  /// model can be swapped without touching the code.
  @Expose()
  AssetRef jumpClip = AssetRef.none;

  /// The entity below the player carrying the wave layer's animator
  /// (`spine` on the box character, `Rig` on a downloaded one); empty
  /// when the character has no wave.
  @Expose()
  String waveLayer = 'spine';

  /// The name of the blend space's parameter (player.blend.json).
  @Expose()
  String speedParameter = 'speed';

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

  /// The locomotion blend space: what the scene's animator started on.
  late AssetRef locomotion;

  /// Whether the jump one-shot is playing (held until the landing).
  bool inJump = false;

  /// Ticks the player has been in the air (for the HUD's readout and the
  /// tests).
  int jumps = 0;
  double _elapsed = 0;
  double _sinceWin = 0;
  bool _dead = false;

  GameState get state => context.service<GameState>();

  @override
  void start() {
    spawn = transform.position;
    locomotion = get<Animator>()?.clip ?? AssetRef.none;
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
    pose(jumped: jumped, grounded: me.grounded, velocity: me.velocity);
    if (Input.pressed('wave')) wave();

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

  /// The blend space's parameter is the controller's planar speed as
  /// the last tick applied it (so a wall stops the stride with the
  /// player); the jump plays once and holds its last frame until the
  /// player lands (the controller is airborne longer than the clip), then
  /// the locomotion blends back in.
  void pose({required bool jumped, required bool grounded, required Vec3 velocity}) {
    animation.setParameter(speedParameter, math.sqrt(velocity.x * velocity.x + velocity.z * velocity.z));
    if (jumped) {
      inJump = true;
      animation.play(jumpClip, looping: false);
      return;
    }
    if (!inJump || !grounded) return;
    inJump = false;
    animation.crossfade(locomotion, blend);
  }

  /// Plays the wave layer once (the additive animator on [waveLayer]).
  void wave() {
    if (waveLayer.isEmpty) return;
    final layer = gameObject.findDescendant(waveLayer);
    final clip = layer?.get<Animator>()?.clip;
    if (layer == null || clip == null) return;
    layer.animation.play(clip, looping: false);
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
