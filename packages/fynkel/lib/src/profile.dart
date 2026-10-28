part of 'world.dart';

// Per-system frame timing (stage 14's GT7): what Unity's Profiler shows in
// its CPU view, for the Dart side of a frame. The engine's own side, on
// the view's render thread, is [ViewStats.busy].

/// How long the last frame's [World.update] took, part by part
/// ([World.profile]): the game's code in each of the frame's calls, the
/// world's own work, and each system's (physics, the animators, the
/// particles, sound, the cameras), as Unity's Profiler shows them. Each
/// part's time is its own: a system's hooks aren't counted in the part
/// they run in. The parts and the systems add up to [total].
///
/// It's the frame on the game's thread. What the engine does to draw it
/// runs on the view's render thread ([ViewStats.busy]), and a
/// physics step run between frames on physics' own (what's left of it
/// when the next frame starts is waited for in physics' time).
///
/// Always on: timing a frame allocates nothing, and scale's 10,000
/// objects' frame went from 0.49 to 0.51 ms with it, within its noise
/// (Mac, 2026-10-27). Zero before the first frame.
final class FrameProfile {
  FrameProfile._(this._world);

  final World _world;
  final _clock = Stopwatch()..start();

  // The last finished frame's ticks by part, and the one being timed.
  final _last = List<int>.filled(_Part.values.length, 0);
  final _now = List<int>.filled(_Part.values.length, 0);
  var _lastTotal = 0;

  // The tick the frame started at, the last lap's, and the ticks the
  // systems' hooks took this frame, in all and up to the last lap.
  var _start = 0;
  var _mark = 0;
  var _systemTicks = 0;
  var _systemTicksAtMark = 0;

  /// The whole update.
  Duration get total => _duration(_lastTotal);

  /// The input's events and gestures, and the pointer handlers they call
  /// ([GameObject.onPointerDown] and the rest).
  Duration get input => _part(_Part.input);

  /// The [GameObject.onStart]s and [Behaviour.onStart]s.
  Duration get starts => _part(_Part.starts);

  /// The fixed steps' [GameObject.onFixedUpdate]s, and the routines
  /// waiting for [fixedStep]: every step this frame.
  Duration get fixedUpdates => _part(_Part.fixedUpdates);

  /// The [GameObject.onUpdate]s and [Behaviour.onUpdate]s.
  Duration get updates => _part(_Part.updates);

  /// The motions ([GameObject.tween] and the rest), the routines' waits,
  /// and the objects destroyed after a time.
  Duration get motions => _part(_Part.motions);

  /// The [GameObject.onLateUpdate]s and [Behaviour.onLateUpdate]s.
  Duration get lateUpdates => _part(_Part.lateUpdates);

  /// Sending the meshes changed this frame ([World.meshChangeTime]).
  Duration get meshes => _part(_Part.meshes);

  /// Sending what was drawn with [World.debug].
  Duration get debug => _part(_Part.debug);

  /// The rest of the world's own work: tidying away what was destroyed,
  /// and between the parts.
  Duration get other => _part(_Part.other);

  /// Each system's hooks, every one it has in a frame together, in the
  /// order they run: the animators, the particles, the cameras, and what
  /// packages add (physics, sound). A system turned off for throwing
  /// takes none.
  List<({String name, Duration time})> get systems => [
    for (final system in _world._systems) (name: system.name, time: _duration(system._lastTicks)),
  ];

  Duration _part(_Part part) => _duration(_last[part.index]);

  Duration _duration(int ticks) => Duration(microseconds: ticks * 1000000 ~/ _clock.frequency);

  /// The frame's parts and systems, in milliseconds: for a print.
  @override
  String toString() {
    String ms(Duration d) => (d.inMicroseconds / 1000).toStringAsFixed(2);
    final parts = [
      for (final part in _Part.values) '${part.label} ${ms(_part(part))}',
      for (final s in systems) '${s.name} ${ms(s.time)}',
    ];
    return 'FrameProfile(${ms(total)} ms: ${parts.join(', ')})';
  }

  int get _ticks => _clock.elapsedTicks;

  void _begin() {
    _now.fillRange(0, _now.length, 0);
    final systems = _world._systems;
    for (var i = 0; i < systems.length; i++) {
      systems[i]._ticks = 0;
    }
    _start = _mark = _ticks;
    _systemTicks = _systemTicksAtMark = 0;
  }

  /// Counts the time since the last lap, less the systems' hooks in it,
  /// as [part]'s.
  void _lap(_Part part) {
    final now = _ticks;
    _now[part.index] += (now - _mark) - (_systemTicks - _systemTicksAtMark);
    _mark = now;
    _systemTicksAtMark = _systemTicks;
  }

  /// Counts [ticks] (since [from]) as [system]'s.
  void _system(WorldSystem system, int from) {
    final ticks = _ticks - from;
    system._ticks += ticks;
    _systemTicks += ticks;
  }

  void _end() {
    _lap(_Part.other);
    _lastTotal = _mark - _start;
    _last.setAll(0, _now);
    final systems = _world._systems;
    for (var i = 0; i < systems.length; i++) {
      systems[i]._lastTicks = systems[i]._ticks;
    }
  }
}

/// The parts of a frame [FrameProfile] times.
enum _Part {
  input('input'),
  starts('starts'),
  fixedUpdates('fixed updates'),
  updates('updates'),
  motions('motions'),
  lateUpdates('late updates'),
  meshes('meshes'),
  debug('debug'),
  other('other');

  const _Part(this.label);
  final String label;
}
