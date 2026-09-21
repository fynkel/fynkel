// Animation value types (M3-19, docs/animation.md): what a game reads
// back from a `fynkel.Animator` and what a load registered. The animator
// and the skin themselves are the native components `Animator`/`Skin`
// (lib/generated/schema.g.dart), attached in a scene or through
// `GameObject.add`/`set`; playback goes through `GameObject.animation`.

import '../generated/schema.g.dart' as gen;
import 'asset_ref.dart';

export '../generated/schema.g.dart' show AnimateOp, Animator, Skin;

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
  });

  factory AnimatorState.fromRecord(gen.AnimatorStateRecord r) => AnimatorState(
        clip: r.clip,
        time: r.time,
        duration: r.duration,
        speed: r.speed,
        fadeRemaining: r.fadeRemaining,
        playing: r.playing,
        looping: r.looping,
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
