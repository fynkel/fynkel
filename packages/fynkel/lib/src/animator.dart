part of 'world.dart';

/// One of a model's animations, as the file names it: a walk, a jump.
/// [Model.clips] lists them; an [Animator] plays them.
final class AnimationClip {
  const AnimationClip._(this.name, this.length);

  /// The file's name for it, or `animation N` (its place in the file)
  /// for one with none.
  final String name;

  /// Seconds.
  final double length;

  @override
  String toString() => "AnimationClip('$name', ${length.toStringAsFixed(3)} s)";
}

/// Plays a model's clips on a copy of it: [GameObject.animator] of a
/// model's copy ([Model.instantiate]) that has clips, as Unity gives a
/// model its Animator.
///
/// ```dart
/// final hero = add(await assets.instantiate('hero.glb'));
/// final animator = hero.animator!;
/// animator.play('idle');
/// // later, in onUpdate:
/// animator.crossFade(speed > 0.1 ? 'walk' : 'idle', duration: 0.2);
/// ```
///
/// The clips run in the engine, once a frame after the updates (after
/// [GameObject.onUpdate], before [GameObject.onLateUpdate]): each moves
/// the parts it moves, and nothing else. To turn a part the clip moves
/// (a head that looks at you), set it in onLateUpdate, after the clip.
/// Every copy plays on its own: its own clip, time and speed.
///
/// The game's code says what plays when; there's no state-machine file.
/// Calling [crossFade] with the clip already playing changes nothing, so
/// an update can say what it wants each frame.
final class Animator {
  Animator._(this.gameObject, Model model) : clips = model.clips {
    for (var i = clips.length - 1; i >= 0; i--) {
      _index[clips[i].name] = i;
    }
    final w = gameObject.world;
    final capacity = scratch.idCapacity;
    check(fy_animator_parts(w.pointer, gameObject.id, scratch.ids(), capacity, scratch.count));
    final count = scratch.count.value;
    if (count > capacity) check(fy_animator_parts(w.pointer, gameObject.id, scratch.ids(count), count, scratch.count));
    final ids = scratch.ids();
    _parts = [for (var i = 0; i < count; i++) w._object(ids[i])];
  }

  /// The model's copy whose parts it moves.
  final GameObject gameObject;

  /// The model's clips, in the file's order.
  final List<AnimationClip> clips;

  final _index = <String, int>{};
  late final List<GameObject> _parts;
  // Whether a clip may be moving the parts: from a play until a stop.
  var _moving = false;
  // The callbacks at times in clips (by clip index), and whether the
  // engine reports this animator's clips.
  Map<int, List<(double, void Function())>>? _marks;
  var _reporting = false;
  void Function(String clip)? _onEnd;

  /// The clip names, in the file's order.
  Iterable<String> get clipNames => clips.map((c) => c.name);

  int _clip(String name) {
    final index = _index[name];
    if (index != null) return index;
    throw ArgumentError.value(
      name,
      'clip',
      clips.isEmpty ? 'the model has no clips' : 'not one of the model\'s clips (${clipNames.join(', ')})',
    );
  }

  Pointer<FyWorld> get _world => gameObject._engine('its animator can\'t be used');

  /// Plays clip [name] from its start, at once, in place of whatever
  /// plays. [loop]: round again at its end (a walk), or hold its last pose
  /// (a jump, a fall), calling [onEnd].
  void play(String name, {bool loop = true}) {
    check(fy_animator_play(_world, gameObject.id, _clip(name), 0, loop));
    _moving = true;
  }

  /// Fades from what plays to clip [name] over [duration] seconds: [name]
  /// from its start, unless it's already playing or fading in, when it
  /// goes on from where it is. [loop] as [play].
  void crossFade(String name, {double duration = 0.25, bool loop = true}) {
    if (duration.isNaN || duration < 0) throw ArgumentError.value(duration, 'duration', 'must be 0 or more');
    check(fy_animator_play(_world, gameObject.id, _clip(name), duration, loop));
    _moving = true;
  }

  /// Stops every clip: the parts keep the pose they're in.
  void stop() {
    check(fy_animator_stop(_world, gameObject.id));
    _moving = false;
  }

  FyAnimatorState _state() {
    check(fy_animator_get(_world, gameObject.id, scratch.animatorState));
    return scratch.animatorState.ref;
  }

  /// The clip last played or faded to, or null when none plays.
  String? get current {
    final clip = _state().clip;
    return clip < 0 ? null : clips[clip].name;
  }

  /// Whether clip [name] is in the pose: playing, fading in, or fading
  /// out.
  bool isPlaying(String name) => weight(name) > 0;

  /// How much clip [name] is in the pose: 1 alone, 0 not playing, between
  /// while a crossfade runs.
  double weight(String name) {
    check(fy_animator_weight(_world, gameObject.id, _clip(name), scratch.number));
    return scratch.number.value;
  }

  /// Seconds into the [current] clip, 0 to its [length]. Setting it jumps
  /// there.
  double get time => _state().time;

  set time(double value) {
    if (!value.isFinite) throw ArgumentError.value(value, 'time', 'must be finite');
    check(fy_animator_time_set(_world, gameObject.id, value));
  }

  /// The [current] clip's length in seconds, 0 when none plays.
  double get length => _state().length;

  /// How fast the clips play: 1 as made, 2 twice as fast, 0 paused,
  /// negative backwards. Game time scales it too ([Time.scale], pausing).
  double get speed => _state().speed;

  set speed(double value) {
    if (!value.isFinite) throw ArgumentError.value(value, 'speed', 'must be finite');
    check(fy_animator_speed_set(_world, gameObject.id, value));
  }

  /// Called with the clip's name when a clip that doesn't loop reaches its
  /// end: a jump that landed, an attack that's done.
  void Function(String clip)? get onEnd => _onEnd;

  set onEnd(void Function(String clip)? value) {
    _onEnd = value;
    _report();
  }

  /// Calls [callback] each time clip [name] passes [time] seconds while
  /// it plays forwards: a footstep, the moment a sword hits. A time of 0
  /// comes each time a looping clip comes round, not as it starts.
  void at(String name, double time, void Function() callback) {
    final clip = _clip(name);
    if (!time.isFinite || time < 0 || time > clips[clip].length) {
      throw ArgumentError.value(time, 'time', 'must be within the clip, 0 to ${clips[clip].length}');
    }
    ((_marks ??= {})[clip] ??= []).add((time, callback));
    _report();
  }

  void _report() {
    final on = _onEnd != null || (_marks?.isNotEmpty ?? false);
    if (on == _reporting) return;
    check(fy_animator_report_set(_world, gameObject.id, on));
    _reporting = on;
  }

  /// What the engine said clip [clip] did this frame: from [from] to [to]
  /// seconds, coming round [loops] times, or [ended].
  void _event(int clip, double from, double to, int loops, bool ended) {
    final world = gameObject.world;
    if (_marks?[clip] case final marks?) {
      final length = clips[clip].length;
      final end = to + loops * length;
      for (final (at, callback) in marks) {
        for (var round = 0; round <= loops; round++) {
          final t = at + round * length;
          if (t > from && t <= end) world._safely(gameObject, callback);
        }
      }
    }
    if (ended) {
      if (_onEnd case final onEnd?) world._safely(gameObject, () => onEnd(clips[clip].name));
    }
  }

  @override
  String toString() => 'Animator(${gameObject.name ?? gameObject.id}, ${clips.length} clips)';
}

extension _Animate on World {
  Animator _addAnimator(GameObject object, Model model) {
    final animator = Animator._(object, model);
    _animators[object.id] = animator;
    return animator;
  }

  /// Plays the clips: the engine poses every animator's parts, then the
  /// parts' kept transforms are forgotten, so reading one reads the pose,
  /// and the clips' events go to the game.
  void _animate() {
    if (_animators.isEmpty) return;
    _sendTransforms();
    check(fy_world_animate(pointer, time.delta, scratch.count));
    final events = scratch.count.value;
    final version = ++_transformVersion;
    for (final animator in _animators.values) {
      if (!animator._moving) continue;
      for (final part in animator._parts) {
        if (part._gone) continue;
        part
          .._transform = null
          .._transformStamp = version;
        _logMoved(part);
      }
    }
    if (events > 0) _animationEvents(events);
  }

  void _animationEvents(int count) {
    final buf = scratch.animationEvents(count);
    check(fy_animation_events_get(pointer, buf, count, scratch.count));
    // Copied first: a callback may play, stop or destroy.
    final events = [
      for (var i = 0; i < count; i++) (buf[i].root, buf[i].clip, buf[i].from, buf[i].to, buf[i].loops, buf[i].ended),
    ];
    for (final (root, clip, from, to, loops, ended) in events) {
      final animator = _animators[root];
      if (animator == null || animator.gameObject._destroyed) continue;
      animator._event(clip, from, to, loops, ended);
      if (_disposeAsked) return;
    }
  }
}
