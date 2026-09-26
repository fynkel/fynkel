import 'package:fynkel/fynkel.dart';

/// Waves the runner's right arm every [every] seconds while it keeps
/// running: a clip made in code ([ClipBuilder], no file) played as a
/// one-shot in the Animator Controller's `UpperBody` slot (ADR-0033 § 2 —
/// assets/models/runner.controller.json's second layer, rooted at the
/// spine, has no states: it shows only its one-shots).
@FynkelBehaviour(id: 'game.Waver')
class Waver extends Behaviour {
  @Expose(min: 0.5, step: 0.5, tooltip: 'Seconds between two waves')
  double every = 3.0;

  /// The clip, made at start.
  late final AssetRef clip;

  /// Waves played so far (runtime state).
  int waves = 0;

  double _wait = 0;

  /// The right arm raised sideways, the forearm waving twice, lowered
  /// again: 1 s, with a `hello` marker at the top.
  static ClipData waveClip() {
    Quat z(double degrees) => Quat.euler(Vec3(0, 0, degrees));
    return (ClipBuilder()
          ..rotation('upperarm_r', {0: z(-4), 0.25: z(-150), 0.75: z(-150), 1: z(-4)})
          ..rotation('forearm_r', {0: z(0), 0.25: z(25), 0.4: z(-25), 0.55: z(25), 0.75: z(0)})
          ..marker(0.25, 'hello'))
        .build();
  }

  @override
  void start() {
    clip = world.createClip(waveClip());
    _wait = every;
  }

  @override
  void fixedUpdate(FixedUpdateContext context) {
    _wait -= context.dt;
    if (_wait > 0) return;
    _wait += every;
    animation.playOneShot(clip, slot: 'UpperBody', blendIn: 0.15, blendOut: 0.2);
    waves++;
  }
}
