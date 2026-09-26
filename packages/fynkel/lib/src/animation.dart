// Animation value types (M3-19, docs/animation.md): what a game reads
// back from a `fynkel.Animator` and what a load registered. The animator
// and the skin themselves are the native components `Animator`/`Skin`
// (lib/generated/schema.g.dart), attached in a scene or through
// `GameObject.add`/`set`; playback goes through `GameObject.animation`.

import '../generated/schema.g.dart' as gen;
import 'asset_ref.dart';
import '../math.dart';

export '../generated/schema.g.dart' show AnimateOp, Animator, AnimatorParameterKind, Skin;

/// One layer of an Animator Controller as the last tick left it
/// (`GameObject.animation.stateOf`, ADR-0032; Unity's
/// `AnimatorStateInfo`). During a crossfade, [name] is the state fading
/// out and [next] the one fading in, as Unity reports them.
class AnimatorStateInfo {
  const AnimatorStateInfo({
    required this.layer,
    required this.name,
    required this.normalizedTime,
    required this.length,
    this.next,
    this.nextNormalizedTime = 0,
    this.transitionProgress = 0,
  });

  factory AnimatorStateInfo.fromRecord(gen.AnimatorLayerStateRecord r) => AnimatorStateInfo(
        layer: r.layer,
        name: r.state,
        normalizedTime: r.normalizedTime,
        length: r.length,
        next: r.inTransition ? r.next : null,
        nextNormalizedTime: r.nextNormalizedTime,
        transitionProgress: r.transitionProgress,
      );

  final int layer;

  /// The state playing (the one fading out during a crossfade).
  final String name;

  /// How far into the state, in lengths of its motion: 1.5 is halfway
  /// through the second loop.
  final double normalizedTime;

  /// The state's motion's length in seconds (0 for no motion).
  final double length;

  /// The state being faded to, or null when no crossfade runs.
  final String? next;
  final double nextNormalizedTime;

  /// How far the crossfade is, 0 → 1 (0 without one).
  final double transitionProgress;

  bool get isInTransition => next != null;

  @override
  String toString() => 'AnimatorStateInfo($layer: $name ${normalizedTime.toStringAsFixed(3)}'
      '${next == null ? '' : ' → $next ${(transitionProgress * 100).round()}%'})';
}

/// One parameter of an Animator Controller and its value after the last
/// tick (`GameObject.animation.parameters`, ADR-0032).
class AnimatorParameter {
  const AnimatorParameter(this.name, this.kind, this.value);

  factory AnimatorParameter.fromRecord(gen.AnimatorParameterRecord r) => AnimatorParameter(
        r.name,
        r.kind,
        switch (r.kind) {
          gen.AnimatorParameterKind.float => r.value,
          gen.AnimatorParameterKind.int => r.integer,
          gen.AnimatorParameterKind.bool || gen.AnimatorParameterKind.trigger => r.flag,
        },
      );

  final String name;
  final gen.AnimatorParameterKind kind;

  /// A `double`, an `int` or a `bool` (a trigger: whether it is set).
  final Object value;

  @override
  String toString() => 'AnimatorParameter($name: ${kind.name} = $value)';
}

/// An animator's playback as the last tick left it
/// (`GameObject.animation.state`, `FYNKEL_QUERY_ANIMATOR`).
class AnimatorState {
  const AnimatorState({
    required this.clip,
    required this.time,
    required this.duration,
    required this.speed,
    required this.fadeRemaining,
    required this.playing,
    required this.looping,
    this.weight = 1,
    this.parameter = 0,
    this.rootDelta = Vec3.zero,
    this.rootYaw = 0,
  });

  factory AnimatorState.fromRecord(gen.AnimatorStateRecord r) => AnimatorState(
        clip: r.clip,
        time: r.time,
        duration: r.duration,
        speed: r.speed,
        fadeRemaining: r.fadeRemaining,
        playing: r.playing,
        looping: r.looping,
        weight: r.weight,
        parameter: r.parameter,
        rootDelta: r.rootDelta,
        rootYaw: r.rootYaw,
      );

  /// The clip playing (the crossfade's target while one runs).
  final AssetRef clip;

  /// Seconds into [clip], `0 ≤ time ≤ duration`.
  final double time;

  /// The clip's length in seconds (0 while the clip is not registered).
  final double duration;

  /// Playback rate; negative plays backwards.
  final double speed;

  /// Seconds left of a crossfade, 0 when none runs.
  final double fadeRemaining;

  /// Advancing every tick; false after `pause`, `stop` or a one-shot's end.
  final bool playing;
  final bool looping;

  /// The layer weight (`AnimationControl.weight`), `0…1`.
  final double weight;

  /// The parameter a playing blend space reads (else the last value set
  /// with `setParameter`).
  final double parameter;

  /// Root motion taken out of the pose over the last tick, in the
  /// object's parent space; zero without `Animator.rootMotion`.
  final Vec3 rootDelta;

  /// Root yaw taken out over the last tick, radians.
  final double rootYaw;

  bool get isFading => fadeRemaining > 0;

  /// [time] as a fraction of [duration] (0 for an unregistered clip).
  double get progress => duration > 0 ? time / duration : 0;

  @override
  String toString() =>
      'AnimatorState($clip ${time.toStringAsFixed(3)}/${duration.toStringAsFixed(3)} s ×$speed'
      '${playing ? '' : ', paused'}${looping ? ', looping' : ''}${isFading ? ', fading ${fadeRemaining.toStringAsFixed(3)} s' : ''})';
}

/// What a load registered for the scene's animators and skins
/// (`LoadReport.animation`).
class AnimationAssets {
  const AnimationAssets({this.clips = 0, this.skins = 0});

  factory AnimationAssets.fromJson(Map<String, Object?> j) => AnimationAssets(
        clips: (j['clips'] as num?)?.toInt() ?? 0,
        skins: (j['skins'] as num?)?.toInt() ?? 0,
      );

  /// Clips registered from the cache for the scene's `Animator`s.
  final int clips;

  /// Skins registered for the scene's `Skin` components.
  final int skins;

  @override
  String toString() => 'animation($clips clips, $skins skins)';
}
