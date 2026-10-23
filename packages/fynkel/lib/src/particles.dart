part of 'world.dart';

/// A number for each particle as it's made (Unity's `MinMaxCurve`, as a
/// constant or random between two): `Range(5)` is 5 for all, `Range(0.6,
/// 1.0)` one at random from 0.6 to 1.0.
final class Range {
  /// [min] to [max], or [min] for all without one.
  const Range(this.min, [double? max]) : max = max ?? min;

  /// The least it picks.
  final double min;

  /// The most it picks: [min] when it's the same for all.
  final double max;

  bool get _finite => min.isFinite && max.isFinite;

  @override
  bool operator ==(Object other) => other is Range && other.min == min && other.max == max;

  @override
  int get hashCode => Object.hash(min, max);

  @override
  String toString() => min == max ? 'Range($min)' : 'Range($min, $max)';
}

/// Where a [ParticleSystem]'s particles live (Unity's
/// `ParticleSystemSimulationSpace`).
enum ParticleSpace {
  /// They move with the object: a torch's flame on a moving cart.
  local,

  /// They stay where they were made as the object moves on: dust, smoke,
  /// a trail.
  world,
}

/// What happens when a [ParticleSystem] is done: stopped (or not looping
/// and its [ParticleSystem.duration] past), its last particle gone.
/// Unity's `ParticleSystemStopAction`.
enum StopAction {
  /// Nothing.
  none,

  /// Its object is turned off ([GameObject.active] false), as a pool
  /// wants: turned on again, it plays from the start.
  disable,

  /// Its object is destroyed: a burst that cleans up after itself.
  destroy,

  /// Its [ParticleSystem.onStopped] is called.
  callback,
}

/// So many particles at once, [time] seconds into the timeline (Unity's
/// `ParticleSystem.Burst`): [count] of them, [cycles] times in all (0 for
/// every [interval] while it plays), each time with [probability], 0 to
/// 1.
final class Burst {
  /// Unity's default burst: 30 at the start, once.
  const Burst({
    this.time = 0,
    this.count = const Range(30),
    this.cycles = 1,
    this.interval = 0.01,
    this.probability = 1,
  });

  /// Seconds into the timeline.
  final double time;

  /// How many particles each time.
  final Range count;

  /// How many times in all; 0 for as long as it plays.
  final int cycles;

  /// Seconds between its times, at least 0.01.
  final double interval;

  /// How likely each time is, 0 to 1.
  final double probability;

  @override
  bool operator ==(Object other) =>
      other is Burst &&
      other.time == time &&
      other.count == count &&
      other.cycles == cycles &&
      other.interval == interval &&
      other.probability == probability;

  @override
  int get hashCode => Object.hash(time, count, cycles, interval, probability);

  @override
  String toString() => 'Burst(time: $time, count: $count, cycles: $cycles)';
}

/// How many particles a [ParticleSystem] makes (Unity's emission module):
/// [rateOverTime] a second while its timeline runs, and its [bursts], at
/// most 8.
final class Emission {
  /// Unity's default: ten a second, no bursts.
  const Emission({this.rateOverTime = 10, this.bursts = const []});

  /// Particles a second while the timeline runs.
  final double rateOverTime;

  /// At most 8.
  final List<Burst> bursts;

  @override
  String toString() => 'Emission(rateOverTime: $rateOverTime, bursts: $bursts)';
}

enum _ShapeKind { point, sphere, hemisphere, cone, box, circle, edge }

/// Where a [ParticleSystem]'s particles are made, and which way they go
/// (Unity's shape module). Shapes face the object's up (+Y), so one on an
/// object that isn't turned emits upwards. [randomDirection], 0 to 1,
/// turns each particle's way that far towards one at random.
final class ParticleShape {
  /// From the object's position, every way.
  const ParticleShape.point({this.randomDirection = 0})
    : _kind = _ShapeKind.point,
      radius = 0,
      radiusThickness = 1,
      angle = 0,
      arc = 2 * math.pi,
      size = const Vec3(1, 1, 1);

  /// In a ball of [radius], outwards. [radiusThickness] is how much of it
  /// makes particles, from its surface in: 0 the surface alone, 1 all
  /// through.
  const ParticleShape.sphere({this.radius = 1, this.radiusThickness = 1, this.randomDirection = 0})
    : _kind = _ShapeKind.sphere,
      angle = 0,
      arc = 2 * math.pi,
      size = const Vec3(1, 1, 1);

  /// In the top half of a ball, outwards: dust kicked up from the ground.
  const ParticleShape.hemisphere({this.radius = 1, this.radiusThickness = 1, this.randomDirection = 0})
    : _kind = _ShapeKind.hemisphere,
      angle = 0,
      arc = 2 * math.pi,
      size = const Vec3(1, 1, 1);

  /// From a disc of [radius], up and out by up to [angle] from up, 0 to
  /// under 90° (`25.deg`, Unity's), round [arc] of it: a fountain, a
  /// flame. Unity's default shape.
  const ParticleShape.cone({
    this.angle = 25 * math.pi / 180,
    this.radius = 1,
    this.radiusThickness = 1,
    this.arc = 2 * math.pi,
    this.randomDirection = 0,
  }) : _kind = _ShapeKind.cone,
       size = const Vec3(1, 1, 1);

  /// In a box of [size], up.
  const ParticleShape.box({this.size = const Vec3(1, 1, 1), this.randomDirection = 0})
    : _kind = _ShapeKind.box,
      radius = 0,
      radiusThickness = 1,
      angle = 0,
      arc = 2 * math.pi;

  /// In a flat ring of [radius] (across X and Z), outwards, round [arc]:
  /// a shockwave.
  const ParticleShape.circle({
    this.radius = 1,
    this.radiusThickness = 1,
    this.arc = 2 * math.pi,
    this.randomDirection = 0,
  }) : _kind = _ShapeKind.circle,
       angle = 0,
       size = const Vec3(1, 1, 1);

  /// Along a line [radius] each way across (X), up: rain from a cloud's
  /// edge, a curtain of sparks.
  const ParticleShape.edge({this.radius = 1, this.randomDirection = 0})
    : _kind = _ShapeKind.edge,
      radiusThickness = 1,
      angle = 0,
      arc = 2 * math.pi,
      size = const Vec3(1, 1, 1);

  final _ShapeKind _kind;

  /// A sphere's, hemisphere's, circle's or cone's base's radius; half an
  /// edge's length; in metres.
  final double radius;

  /// How much of the radius makes particles, from the rim in: 0 the rim
  /// alone, 1 all through.
  final double radiusThickness;

  /// A cone's angle from up to its side, in radians.
  final double angle;

  /// How much of a circle or cone goes round, in radians, up to 2π.
  final double arc;

  /// How far each particle's way is turned towards one at random, 0 to 1.
  final double randomDirection;

  /// A box's size, in metres.
  final Vec3 size;

  @override
  String toString() => 'ParticleShape.${_kind.name}(radius: $radius)';
}

/// Smoke, dust, sparks, fire, rain and bursts of light, as Unity's
/// Particle System: a component that makes particles from its [shape],
/// at its [emission]'s rate and in its bursts, each living a
/// [startLifetime] and moving at a [startSpeed], drawn with its
/// [material] as squares that face the camera.
///
/// The engine moves them, once a frame, after the animators and before
/// [Behaviour.lateUpdate]; Dart only sends the settings when they change.
/// A system pauses with the world and follows [Time.scale], unless
/// [useUnscaledTime]. Its numbers are as Unity's main module's, with its
/// defaults: five seconds looping, ten a second living five seconds each,
/// at 5 m/s, white, from a cone.
///
/// Turned off ([enabled] false) or with its object hidden, it stops and
/// its particles go; on or shown again, it plays from the start if it
/// [playOnAwake].
///
/// Each setter sends the settings at once, and throws an [ArgumentError]
/// for one the engine refuses, keeping the old value.
final class ParticleSystem extends Component {
  /// A system with Unity's defaults, but for what's given. Without a
  /// [material] it moves its particles but draws none.
  ParticleSystem({
    this._duration = 5,
    this._looping = true,
    this._prewarm = false,
    this._startDelay = 0,
    this._startLifetime = const Range(5),
    this._startSpeed = const Range(5),
    this._startSize = const Range(1),
    this._startRotation = const Range(0),
    this._startColor = Color.white,
    this._gravityModifier = 0,
    this._simulationSpace = ParticleSpace.local,
    this._simulationSpeed = 1,
    this._useUnscaledTime = false,
    this._playOnAwake = true,
    this._maxParticles = 1000,
    this._stopAction = StopAction.none,
    this._randomSeed,
    this._emission = const Emission(),
    this._shape = const ParticleShape.cone(),
    EaseFunction? sizeOverLifetime,
    ColorGradient? colorOverLifetime,
    this._material,
    this.onStopped,
  }) : _sizeOverLifetime = sizeOverLifetime,
       _sizeTable = _sizes(sizeOverLifetime),
       _colorOverLifetime = colorOverLifetime,
       _colorTable = _colors(colorOverLifetime),
       super._() {
    if (_refused() case final error?) throw error;
  }

  /// Called when it's done, with [StopAction.callback] (Unity's
  /// `OnParticleSystemStopped`).
  void Function()? onStopped;

  double _duration;
  bool _looping, _prewarm;
  double _startDelay;
  Range _startLifetime, _startSpeed, _startSize, _startRotation;
  ParticleColor _startColor;
  double _gravityModifier;
  ParticleSpace _simulationSpace;
  double _simulationSpeed;
  bool _useUnscaledTime, _playOnAwake;
  int _maxParticles;
  StopAction _stopAction;
  int? _randomSeed;
  Emission _emission;
  ParticleShape _shape;
  EaseFunction? _sizeOverLifetime;
  Float32List? _sizeTable;
  ColorGradient? _colorOverLifetime;
  Float32List? _colorTable;
  Material? _material;

  /// How long its timeline is, in seconds, at least 0.05: one round of a
  /// looping system, the whole of one that isn't.
  double get duration => _duration;
  set duration(double value) => _set(() => _duration, (v) => _duration = v, value);

  /// It plays its timeline again each time it ends.
  bool get looping => _looping;
  set looping(bool value) => _set(() => _looping, (v) => _looping = v, value);

  /// A looping system starts as if it had played a whole [duration], full.
  bool get prewarm => _prewarm;
  set prewarm(bool value) => _set(() => _prewarm, (v) => _prewarm = v, value);

  /// Seconds from play to the first particle.
  double get startDelay => _startDelay;
  set startDelay(double value) => _set(() => _startDelay, (v) => _startDelay = v, value);

  /// How long each particle lives, in seconds.
  Range get startLifetime => _startLifetime;
  set startLifetime(Range value) => _set(() => _startLifetime, (v) => _startLifetime = v, value);

  /// How fast each starts, in metres a second, the way its [shape] sends
  /// it.
  Range get startSpeed => _startSpeed;
  set startSpeed(Range value) => _set(() => _startSpeed, (v) => _startSpeed = v, value);

  /// How big each starts, across, in metres.
  Range get startSize => _startSize;
  set startSize(Range value) => _set(() => _startSize, (v) => _startSize = v, value);

  /// How far each is turned, in radians (`Range(0, 360.deg)` for any
  /// way).
  Range get startRotation => _startRotation;
  set startRotation(Range value) => _set(() => _startRotation, (v) => _startRotation = v, value);

  /// Each particle's colour: a [Color], or a [ColorRange] for one at
  /// random. Times the [material]'s colour and texture.
  ParticleColor get startColor => _startColor;
  set startColor(ParticleColor value) => _set(() => _startColor, (v) => _startColor = v, value);

  /// How much gravity pulls them, times 9.81 m/s² down in the world: 1 for
  /// sparks that fall, a little below 0 for smoke that rises.
  double get gravityModifier => _gravityModifier;
  set gravityModifier(double value) => _set(() => _gravityModifier, (v) => _gravityModifier = v, value);

  /// Where its particles live: moved with its object, or left behind.
  ParticleSpace get simulationSpace => _simulationSpace;
  set simulationSpace(ParticleSpace value) => _set(() => _simulationSpace, (v) => _simulationSpace = v, value);

  /// How fast its time goes, times the game's.
  double get simulationSpeed => _simulationSpeed;
  set simulationSpeed(double value) => _set(() => _simulationSpeed, (v) => _simulationSpeed = v, value);

  /// Its time goes on when the world is paused or [Time.scale] is 0: for
  /// a pause menu's effects.
  bool get useUnscaledTime => _useUnscaledTime;
  set useUnscaledTime(bool value) => _set(() => _useUnscaledTime, (v) => _useUnscaledTime = v, value);

  /// It plays when it comes into the world, and each time it's shown
  /// again; else it waits for [play].
  bool get playOnAwake => _playOnAwake;
  set playOnAwake(bool value) => _set(() => _playOnAwake, (v) => _playOnAwake = v, value);

  /// The most particles it holds at once, 1 to 100,000; 1000 unless set.
  /// Past it, it makes no more till some die.
  int get maxParticles => _maxParticles;
  set maxParticles(int value) => _set(() => _maxParticles, (v) => _maxParticles = v, value);

  /// What happens when it's done.
  StopAction get stopAction => _stopAction;
  set stopAction(StopAction value) => _set(() => _stopAction, (v) => _stopAction = v, value);

  /// The same particles each play, for a test or a replay; null, new ones
  /// each play (Unity's `useAutoRandomSeed`).
  int? get randomSeed => _randomSeed;
  set randomSeed(int? value) => _set(() => _randomSeed, (v) => _randomSeed = v, value);

  /// How many particles it makes: a rate and bursts.
  Emission get emission => _emission;
  set emission(Emission value) => _set(() => _emission, (v) => _emission = v, value);

  /// Where its particles are made, and the way they go.
  ParticleShape get shape => _shape;
  set shape(ParticleShape value) => _set(() => _shape, (v) => _shape = v, value);

  /// Each particle's size over its life, times its [startSize]: any
  /// function of 0 (born) to 1 (gone), `(t) => 1 - t` or `Ease.outQuad`.
  /// It's read at 64 points when it's set, not called each frame.
  EaseFunction? get sizeOverLifetime => _sizeOverLifetime;
  set sizeOverLifetime(EaseFunction? value) {
    final (oldF, oldT) = (_sizeOverLifetime, _sizeTable);
    _set(() => (oldF, oldT), (v) {
      _sizeOverLifetime = v.$1;
      _sizeTable = v.$2;
    }, (value, _sizes(value)));
  }

  /// Each particle's colour over its life, times its [startColor]:
  /// [ColorGradient.fadeOut] to fade them away.
  ColorGradient? get colorOverLifetime => _colorOverLifetime;
  set colorOverLifetime(ColorGradient? value) {
    final (oldG, oldT) = (_colorOverLifetime, _colorTable);
    _set(() => (oldG, oldT), (v) {
      _colorOverLifetime = v.$1;
      _colorTable = v.$2;
    }, (value, _colors(value)));
  }

  /// What its particles are drawn with: its colour and base colour
  /// texture, unlit, and its emission; [Blending.additive] for fire and
  /// sparks, else see-through by alpha. Without one, nothing is drawn.
  Material? get material => _material;
  set material(Material? value) => _set(() => _material, (v) => _material = v, value);

  /// Plays from the start, keeping the particles it has; a paused system
  /// goes on from where it was.
  void play() => _command(FY_PARTICLES_PLAY, 0);

  /// Stops making particles; those it has live out their lives, or with
  /// [clear], go at once.
  void stop({bool clear = false}) => _command(clear ? FY_PARTICLES_STOP_AND_CLEAR : FY_PARTICLES_STOP, 0);

  /// Stops time for it: nothing moves till [play].
  void pause() => _command(FY_PARTICLES_PAUSE, 0);

  /// Takes its particles away; it plays on.
  void clear() => _command(FY_PARTICLES_CLEAR, 0);

  /// Makes [count] particles now, as its shape would, playing or not.
  void emit(int count) {
    if (count < 0) throw ArgumentError.value(count, 'count', 'must be 0 or more');
    _command(FY_PARTICLES_EMIT, count);
  }

  /// Moves it [seconds] on at once, as if that much time had passed: an
  /// effect seen half-way through.
  void simulate(double seconds) {
    final object = _command(-1, 0);
    if (object == null) return;
    scratch.number.value = seconds;
    check(fy_particles_simulate(object.world.pointer, object.id, scratch.number));
  }

  /// From [play] till it's stopped and its last particle is gone; not
  /// while it's paused. False until it's on a live object.
  bool get isPlaying => _state()?.playing ?? false;

  /// Paused with [pause], till [play].
  bool get isPaused => _state()?.paused ?? false;

  /// Making particles: playing, and not stopped.
  bool get isEmitting => _state()?.emitting ?? false;

  /// Neither playing nor paused.
  bool get isStopped => !isPlaying && !isPaused;

  /// How many particles it has now.
  int get particleCount => _state()?.count ?? 0;

  FyParticleState? _state() {
    final object = _live;
    if (object == null || !_enabledFlag) return null;
    check(fy_particles_get(object.world.pointer, object.id, scratch.particleState));
    return scratch.particleState.ref;
  }

  /// Sends [command] (one of `FY_PARTICLES_*`, or -1 for none) to the
  /// engine; returns the object it's on, or null when it's turned off.
  GameObject? _command(int command, int count) {
    _checkChangeable();
    final object = _live;
    if (object == null) {
      throw StateError('this ParticleSystem is on no object in a world yet: add it before playing it');
    }
    if (!_enabledFlag) return null;
    if (command >= 0) check(fy_particles_command(object.world.pointer, object.id, command, count));
    return object;
  }

  void _set<T>(T Function() read, void Function(T) write, T value) {
    _checkChangeable();
    final old = read();
    write(value);
    if (_write() case final failed?) {
      write(old);
      failed.raise();
    }
  }

  @override
  void _enabledChanged(GameObject object) => object._enabledChanged(this);

  _Failure? _write() {
    if (_refused() case final error?) return _Failure(error, StackTrace.current);
    if (!_enabledFlag) return null;
    try {
      _live?._writeParticles(this);
      return null;
    } catch (error, stack) {
      return _Failure(error, stack);
    }
  }

  /// Why it can't be as it is, if it can't: what only Dart knows. The
  /// engine checks the numbers.
  ArgumentError? _refused() {
    if (_emission.bursts.length > 8) return ArgumentError('a ParticleSystem has at most 8 bursts');
    if (_maxParticles < 1 || _maxParticles > 100000) {
      return ArgumentError.value(_maxParticles, 'maxParticles', 'must be 1 to 100,000');
    }
    if (_randomSeed case final seed? when seed < 0 || seed > 0xFFFFFFFF) {
      return ArgumentError.value(seed, 'randomSeed', 'must be 0 to 4294967295');
    }
    for (final r in [_startLifetime, _startSpeed, _startSize, _startRotation]) {
      if (!r._finite) return ArgumentError('a ParticleSystem\'s ranges must be finite; got $r');
    }
    if (_sizeTable case final t? when t.any((v) => !v.isFinite)) {
      return ArgumentError('sizeOverLifetime must give finite numbers from 0 to 1');
    }
    return null;
  }

  /// [f] at 64 points from 0 to 1.
  static Float32List? _sizes(EaseFunction? f) {
    if (f == null) return null;
    return Float32List(FY_PARTICLE_TABLE)
      ..setAll(0, [for (var i = 0; i < FY_PARTICLE_TABLE; i++) f(i / (FY_PARTICLE_TABLE - 1))]);
  }

  /// [g] at 64 points from 0 to 1, as sRGBA.
  static Float32List? _colors(ColorGradient? g) {
    if (g == null) return null;
    final table = Float32List(FY_PARTICLE_TABLE * 4);
    for (var i = 0; i < FY_PARTICLE_TABLE; i++) {
      final c = g.at(i / (FY_PARTICLE_TABLE - 1));
      table
        ..[i * 4] = c.r
        ..[i * 4 + 1] = c.g
        ..[i * 4 + 2] = c.b
        ..[i * 4 + 3] = c.a;
    }
    return table;
  }

  @override
  String toString() =>
      'ParticleSystem(startLifetime: $startLifetime, startSpeed: $startSpeed, emission: $emission, shape: $shape)';
}

extension _Particles on World {
  /// Moves every particle system on, after the animators: the game's time
  /// and the unscaled, through native memory, as [_animate]'s. Does the
  /// stop actions of those that were done.
  void _stepParticles() {
    _particleWatch.reset();
    if (_particleSystems.isEmpty) return;
    _sendTransforms();
    scratch.numbers[0] = time._delta;
    scratch.numbers[1] = time._unscaledDelta;
    _particleWatch.start();
    check(fy_world_particles_step(pointer, scratch.numbers, scratch.secondNumber, scratch.count));
    _particleWatch.stop();
    final count = scratch.count.value;
    if (count > 0) _particleEvents(count);
  }

  void _particleEvents(int count) {
    final buf = scratch.particleEvents(count);
    check(fy_particle_events_get(pointer, buf, count, scratch.count));
    // Copied first: a stop action may destroy or add objects.
    final events = [for (var i = 0; i < count; i++) (buf[i].object, buf[i].action)];
    for (final (id, action) in events) {
      final system = _particleSystems[id];
      if (system == null || system.isRemoved) continue;
      final object = system.gameObject;
      switch (action) {
        case FY_PARTICLE_STOP_DISABLE:
          object.active = false;
        case FY_PARTICLE_STOP_DESTROY:
          object.destroy();
        case FY_PARTICLE_STOP_CALLBACK:
          if (system.onStopped case final onStopped?) _safely(object, onStopped);
      }
      if (_disposeAsked) return;
    }
  }
}
