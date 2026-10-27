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

/// What plays on one of an animator's layers: one clip, a blend of clips,
/// or a crossfade between them. The [Animator] itself is its base layer;
/// [Animator.layer] gives the others.
mixin AnimatorPlays {
  Animator get _animator;
  int get _layer;

  Pointer<FyWorld> get _world => _animator.gameObject._engine('its animator can\'t be used');
  int get _root => _animator.gameObject.id;

  /// Plays clip [name] from its start, at once, in place of whatever
  /// plays here. [loop]: round again at its end (a walk), or hold its last
  /// pose (a jump, a fall), calling [Animator.onEnd].
  void play(String name, {bool loop = true}) {
    scratch.number.value = 0;
    check(fy_animator_play(_world, _root, _layer, _animator._clip(name), scratch.number, loop));
    _animator._moves(_layer);
  }

  /// Fades from what plays here to clip [name] over [duration] seconds,
  /// [name] from its start. The fade starts from the pose shown, however
  /// it got there (mid-fade, mid-blend, held after a stop). Fading to the
  /// [current] clip changes nothing (a fade to it that runs goes on), so
  /// an update can ask each frame; to one still fading out, it fades back
  /// in from where it is. [loop] as [play].
  void crossFade(String name, {double duration = 0.25, bool loop = true}) {
    _duration(duration);
    // The numbers through native memory: one passed to a call is boxed,
    // each frame, in the JIT.
    scratch.number.value = duration;
    check(fy_animator_play(_world, _root, _layer, _animator._clip(name), scratch.number, loop));
    _animator._moves(_layer);
  }

  /// Plays the blend [name]: [clips] mixed by one number, [at], so a walk
  /// can be any speed between a stroll and a run, as Unity's 1D blend tree.
  /// Each clip sits at its threshold ([thresholds], or 0, 1, 2… in order);
  /// [at] between two mixes those two, at an end or past it the end's clip
  /// alone. The clips keep one pace, each as far through itself, so feet
  /// land together whatever the mix.
  ///
  /// ```dart
  /// // In onUpdate: idle at rest, walk at 1.1 m/s, run at 3.
  /// animator.blend('move', const ['idle', 'walk', 'run'], thresholds: const [0, 1.1, 3], at: speed);
  /// ```
  ///
  /// Call it each frame with the number: the blend is made the first time
  /// (it always loops), and a fade of [duration] seconds runs only when
  /// something else was playing, as [crossFade]. A blend's name is its
  /// own: it can't be one of the model's clips, and it's made once, with
  /// the same clips each time.
  void blend(String name, List<String> clips, {required double at, List<double>? thresholds, double duration = 0.25}) {
    _duration(duration);
    if (!at.isFinite) throw ArgumentError.value(at, 'at', 'must be finite');
    final blend = _animator._blend(name, clips, thresholds);
    scratch.numbers[0] = at;
    scratch.numbers[1] = duration;
    check(fy_animator_blend(_world, _root, _layer, blend, scratch.numbers));
    _animator._moves(_layer);
  }

  /// Stops what plays here: at once, the parts keeping the pose they're
  /// in, or fading out over [duration] seconds to what's underneath (on
  /// the base layer, the pose the fade began from).
  void stop({double duration = 0}) {
    _duration(duration);
    scratch.number.value = duration;
    check(fy_animator_stop(_world, _root, _layer, scratch.number));
    if (duration == 0) _animator._stops(_layer);
  }

  FyAnimatorState _state() {
    check(fy_animator_get(_world, _root, _layer, scratch.animatorState));
    return scratch.animatorState.ref;
  }

  /// The clip or blend last played or faded to here, or null when none
  /// plays.
  String? get current {
    final state = _state();
    if (state.blend >= 0) return _animator._blendNames[state.blend];
    return state.clip < 0 ? null : _animator.clips[state.clip].name;
  }

  /// Whether clip [name] is in this layer's pose: playing, fading in or
  /// out, or in a blend's mix.
  bool isPlaying(String name) => weight(name) > 0;

  /// How much clip [name] is in this layer's pose: 1 alone, 0 not
  /// playing, between while a crossfade runs or a blend mixes it.
  double weight(String name) {
    check(fy_animator_weight(_world, _root, _layer, _animator._clip(name), scratch.number));
    return scratch.number.value;
  }

  /// Seconds into the [current] clip, 0 to its [length]; for a blend,
  /// into a round of it. Setting it jumps there.
  double get time => _state().time;

  set time(double value) {
    if (!value.isFinite) throw ArgumentError.value(value, 'time', 'must be finite');
    check(fy_animator_time_set(_world, _root, _layer, value));
  }

  /// The [current] clip's length in seconds, 0 when none plays; for a
  /// blend, a round's, at its number now.
  double get length => _state().length;
}

void _duration(double duration) {
  if (duration.isNaN || duration < 0) throw ArgumentError.value(duration, 'duration', 'must be 0 or more');
}

/// One of an [Animator]'s layers above the base: it plays its own clips
/// over the base's, on the whole model or on a part and everything below
/// it ([mask]), in place of what's underneath or added to it
/// ([additive]), as much as its [strength] says.
///
/// ```dart
/// // Wave with the right arm while the legs walk.
/// animator.layer(1, mask: 'upperarm.r').crossFade('wave');
/// // Stop waving: the arm goes back to the walk.
/// animator.layer(1).stop(duration: 0.3);
/// ```
///
/// Layers are drawn up in order, each over the ones below it.
final class AnimatorLayer with AnimatorPlays {
  AnimatorLayer._(this._animator, this.index);

  @override
  final Animator _animator;

  /// Its place: 1 to 7, over the lower ones.
  final int index;

  @override
  int get _layer => index;

  String? _mask;
  var _additive = false;
  var _strength = 1.0;

  /// The part it moves, with everything below it, by name ([GameObject.find]
  /// finds it); null for the whole model.
  String? get mask => _mask;
  set mask(String? value) => _set(value, _additive, _strength);

  /// Whether it adds its clips' movement to what's underneath (a breath, a
  /// flinch over any pose), measured from each clip's first pose, rather
  /// than taking the parts' place.
  bool get additive => _additive;
  set additive(bool value) => _set(_mask, value, _strength);

  /// How much of it shows: 1 all of it, 0 none, between part-way to it
  /// from what's underneath. Unity's layer weight.
  double get strength => _strength;
  set strength(double value) => _set(_mask, _additive, value);

  void _set(String? mask, bool additive, double strength) {
    if (!strength.isFinite) throw ArgumentError.value(strength, 'strength', 'must be finite');
    strength = strength.clamp(0.0, 1.0);
    if (mask == _mask && additive == _additive && strength == _strength) return;
    final length = mask == null ? 0 : scratch.utf8(mask);
    final status = fy_animator_layer_set(_world, _root, index, scratch.bytes(), length, additive, strength);
    if (status != FyStatus.FY_STATUS_OK && mask != null && _animator.gameObject.find(mask) == null) {
      throw ArgumentError.value(mask, 'mask', 'the model has no part by that name');
    }
    check(status);
    _mask = mask;
    _additive = additive;
    _strength = strength;
  }

  @override
  String toString() => 'AnimatorLayer($index${_mask == null ? '' : ', from $_mask'}${_additive ? ', additive' : ''})';
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
/// The animator is its base layer: what it plays moves the whole model.
/// [layer] gives layers over it (a wave on one arm), and [blend] mixes
/// clips by a number (idle, walk and run by speed).
///
/// The game's code says what plays when; there's no state-machine file.
/// Calling [crossFade] with the clip already playing changes nothing, so
/// an update can say what it wants each frame.
final class Animator with AnimatorPlays {
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

  @override
  Animator get _animator => this;
  @override
  int get _layer => 0;

  /// The most layers an animator has, the base included.
  static const maxLayers = 8;

  final _index = <String, int>{};
  late final List<GameObject> _parts;
  // The layers that may be moving the parts, a bit each: from a play
  // until a stop at once.
  var _moving = 0;
  final _layers = List<AnimatorLayer?>.filled(maxLayers, null);
  // The blends made, by name, with their clips and thresholds, and their
  // names by number.
  final _blends = <String, (int, List<String>, List<double>?)>{};
  final _blendNames = <String>[];
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

  void _moves(int layer) => _moving |= 1 << layer;
  void _stops(int layer) => _moving &= ~(1 << layer);

  int _blend(String name, List<String> clips, List<double>? thresholds) {
    if (_blends[name] case (final number, final made, final at)) {
      if (!identical(made, clips) || !identical(at, thresholds)) {
        if (!_same(made, clips) || !_sameNumbers(at, thresholds)) {
          throw ArgumentError.value(name, 'name', 'the blend was made with other clips or thresholds: $made at $at');
        }
      }
      return number;
    }
    if (_index.containsKey(name)) throw ArgumentError.value(name, 'name', 'is one of the model\'s clips');
    if (clips.isEmpty) throw ArgumentError.value(clips, 'clips', 'a blend needs a clip');
    if (thresholds != null && thresholds.length != clips.length) {
      throw ArgumentError.value(thresholds, 'thresholds', 'one for each clip (${clips.length})');
    }
    final count = clips.length;
    final indices = malloc<Uint32>(count);
    final points = malloc<Float>(count);
    try {
      for (var i = 0; i < count; i++) {
        indices[i] = _clip(clips[i]);
        final t = thresholds?[i] ?? i.toDouble();
        if (!t.isFinite) throw ArgumentError.value(thresholds, 'thresholds', 'must be finite');
        points[i] = t;
      }
      final out = malloc<Uint32>();
      try {
        check(fy_animator_blend_define(_world, _root, indices, points, count, out));
        _blends[name] = (
          out.value,
          List.unmodifiable(clips),
          thresholds == null ? null : List.unmodifiable(thresholds),
        );
        _blendNames.add(name);
        return out.value;
      } finally {
        malloc.free(out);
      }
    } finally {
      malloc
        ..free(indices)
        ..free(points);
    }
  }

  /// Layer [index] over the base, 1 to 7, made the first time it's asked
  /// for. [mask], [additive] and [strength] set it as [AnimatorLayer]
  /// says; leaving them out leaves it as it is.
  ///
  /// ```dart
  /// animator.layer(1, mask: 'spine').crossFade('wave');
  /// ```
  AnimatorLayer layer(int index, {String? mask, bool? additive, double? strength}) {
    if (index < 1 || index >= maxLayers) {
      throw RangeError.range(index, 1, maxLayers - 1, 'index', 'the base is the animator itself; layers above it are');
    }
    final layer = _layers[index] ??= AnimatorLayer._(this, index);
    if (mask != null || additive != null || strength != null) {
      layer._set(mask ?? layer._mask, additive ?? layer._additive, strength ?? layer._strength);
    }
    return layer;
  }

  /// Stops every clip on the base layer: at once, the parts keeping the
  /// pose they're in, or fading out over [duration] seconds. The layers
  /// over it play on.
  @override
  void stop({double duration = 0}) => super.stop(duration: duration);

  /// How fast the clips play, on every layer: 1 as made, 2 twice as fast,
  /// 0 paused, negative backwards. Crossfades go at the same pace (forwards
  /// either way), so a paused animator holds its fade. Game time scales it
  /// too ([Time.scale], pausing).
  double get speed => _state().speed;

  set speed(double value) {
    if (!value.isFinite) throw ArgumentError.value(value, 'speed', 'must be finite');
    scratch.number.value = value;
    check(fy_animator_speed_set(_world, gameObject.id, scratch.number));
  }

  /// Called with the clip's name when a clip that doesn't loop reaches its
  /// end, on any layer: a jump that landed, an attack that's done.
  void Function(String clip)? get onEnd => _onEnd;

  set onEnd(void Function(String clip)? value) {
    _onEnd = value;
    _report();
  }

  /// Calls [callback] each time clip [name] passes [time] seconds while
  /// it plays, forwards or backwards, on any layer or in a blend's mix: a
  /// footstep, the moment a sword hits. A time of 0 comes each time a
  /// looping clip comes round, not as it starts. Each call adds a mark, the
  /// same one again too: take it away with [removeMark] (a pooled object
  /// adding it in its `onSpawn` takes it away in its `onRelease`), or all
  /// with [removeMarks].
  void at(String name, double time, void Function() callback) {
    final clip = _clip(name);
    if (!time.isFinite || time < 0 || time > clips[clip].length) {
      throw ArgumentError.value(time, 'time', 'must be within the clip, 0 to ${clips[clip].length}');
    }
    ((_marks ??= {})[clip] ??= []).add((time, callback));
    _report();
  }

  /// Takes away what [at] added with the same clip [name], [time] and
  /// [callback] (a method torn off again is the same callback; a closure
  /// made again isn't), as Unity's `RemoveListener`. Nothing there, it
  /// does nothing.
  void removeMark(String name, double time, void Function() callback) {
    final clip = _clip(name);
    final marks = _marks?[clip];
    if (marks == null) return;
    marks.removeWhere((m) => m.$1 == time && m.$2 == callback);
    if (marks.isEmpty) _marks!.remove(clip);
    _report();
  }

  /// Takes away every mark [at] added to clip [name], or to every clip
  /// when null, as Unity's `RemoveAllListeners`.
  void removeMarks([String? name]) {
    if (name == null) {
      _marks?.clear();
    } else {
      final clip = _clip(name);
      _marks?.remove(clip);
    }
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
    _animatorList.add(animator);
    return animator;
  }

  /// Plays the clips: the engine poses every animator's parts, then the
  /// parts' kept transforms are forgotten, so reading one reads the pose,
  /// and the clips' events go to the game.
  void _animate() {
    if (_animators.isEmpty) return;
    _sendTransforms();
    // The field, through native memory: the getter's double and one passed
    // to a call are each boxed, every frame, in the JIT.
    scratch.number.value = time._delta;
    check(fy_world_animate(pointer, scratch.number, scratch.count));
    final events = scratch.count.value;
    final version = ++_transformVersion;
    for (var i = 0; i < _animatorList.length; i++) {
      final animator = _animatorList[i];
      if (animator._moving == 0) continue;
      final parts = animator._parts;
      for (var j = 0; j < parts.length; j++) {
        final part = parts[j];
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

bool _same(List<String> a, List<String> b) {
  if (a.length != b.length) return false;
  for (var i = 0; i < a.length; i++) {
    if (a[i] != b[i]) return false;
  }
  return true;
}

bool _sameNumbers(List<double>? a, List<double>? b) {
  if (a == null || b == null) return a == b;
  if (a.length != b.length) return false;
  for (var i = 0; i < a.length; i++) {
    if (a[i] != b[i]) return false;
  }
  return true;
}
