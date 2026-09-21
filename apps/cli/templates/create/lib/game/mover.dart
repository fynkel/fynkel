import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import 'hud_model.dart';

/// Walks its object with the `move` action and hops on `jump`.
///
/// Input reaches a behaviour through the project's actions
/// (`fynkel.project.json` → `inputActions`, docs/input.md), never through
/// keys: `move` is an `axis2d` bound to WASD, the left stick and the touch
/// overlay's stick; `jump` a `button` bound to Space, the south gamepad
/// button and the overlay's jump button. `Input.action2D` and
/// `Input.held` read the state as of this tick; `Input.pressed` is true
/// on exactly one tick per press, however many ticks a frame runs.
/// `test/game_test.dart` replays `test/walk.input.json` through this class.
///
/// Sound (docs/audio.md): a take-off plays the object's own
/// `fynkel.AudioSource` — the scene puts `assets/audio/jump.ogg` on the
/// cube — through [audio]; a landing plays the [land] clip once through
/// `world.audio.play`, a one-shot that needs no component. Both are
/// simulation-lane commands: they apply at the next tick, and the null
/// backend of a headless run or a test counts them like a device would.
///
/// After the frame's ticks, [update] writes what the HUD shows into the
/// [HudModel] the game installed (docs/hud.md): the widgets bound to those
/// fields rebuild when a value changed, and only then.
@FynkelBehaviour(id: 'game.Mover')
class Mover extends Behaviour {
  @Expose(min: 0, step: 0.1, tooltip: 'Walking speed in metres per second')
  double speed = 2.0;

  @Expose(min: 0, step: 0.1, tooltip: 'How high a jump lifts the object, in metres')
  double jumpHeight = 1.0;

  @Expose(tooltip: 'Played once on landing (an audio asset, or none)')
  AssetRef land = AssetRef.none;

  /// Seconds a jump lasts, take-off to landing.
  static const jumpDuration = 0.5;

  /// Presses seen so far (not exposed: runtime state).
  int jumps = 0;

  /// Seconds into the current jump, or null on the ground.
  double? airTime;

  /// The ground height the object rests at (its authored y).
  double? _groundY;

  @override
  void fixedUpdate(FixedUpdateContext context) {
    // `move.y` is forward (W, stick up); forward is -z in this scene.
    final move = Input.action2D('move');
    final takeOff = airTime == null && Input.pressed('jump');
    // Idle on the ground: no transform write (no command, no snapshot
    // churn), and the authored pose stays exactly what the scene says.
    if (move == Vec2.zero && airTime == null && !takeOff) return;

    final p = transform.position;
    final ground = _groundY ??= p.y;
    var x = p.x + move.x * speed * context.dt;
    var z = p.z - move.y * speed * context.dt;

    if (takeOff) {
      jumps++;
      airTime = 0;
      if (has<AudioSource>()) audio.play();
    }
    var y = ground;
    if (airTime case final t?) {
      final next = t + context.dt;
      if (next >= jumpDuration) {
        airTime = null;
        if (!land.isNone) world.audio.play(land);
      } else {
        airTime = next;
        y = ground + jumpHeight * math.sin(math.pi * next / jumpDuration);
      }
    }
    transform.position = Vec3(x, y, z);
  }

  @override
  void update(UpdateContext context) {
    final hud = world.context.serviceOrNull<HudModel>();
    if (hud == null) return;
    hud.position.value = transform.position;
    hud.jumps.value = jumps;
    hud.airborne.value = airTime != null;
  }
}
