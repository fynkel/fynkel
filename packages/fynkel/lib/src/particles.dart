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
/// [rateOverTime] a second while its timeline runs, [rateOverDistance] a
/// metre its object moves, and its [bursts], at most 8.
final class Emission {
  /// Unity's default: ten a second, no bursts.
  const Emission({this.rateOverTime = 10, this.rateOverDistance = 0, this.bursts = const []});

  /// Particles a second while the timeline runs.
  final double rateOverTime;

  /// Particles a metre its object moves while the timeline runs, spread
  /// along the way it went: dust behind a rolling ball, footprints. Best
  /// in [ParticleSpace.world], where they stay where they were made.
  final double rateOverDistance;

  /// At most 8.
  final List<Burst> bursts;

  @override
  String toString() => 'Emission(rateOverTime: $rateOverTime, rateOverDistance: $rateOverDistance, bursts: $bursts)';
}

/// When a particle takes its object's velocity ([InheritVelocity.mode]).
enum InheritVelocityMode {
  /// Once, as it's born: sparks thrown from a moving cart keep going.
  initial,

  /// Each frame, carrying it along as the object moves: a flame on a
  /// torch that's waved about, in world space.
  current,
}

/// A particle takes [curve] of its object's velocity (Unity's *Inherit
/// Velocity* module): 1 for all of it.
final class InheritVelocity {
  /// All of it, as it's born, unless given.
  const InheritVelocity({this.mode = InheritVelocityMode.initial, this.curve = const ParticleCurve(1)});

  /// As it's born, or all its life.
  final InheritVelocityMode mode;

  /// How much of the object's velocity, over each particle's life.
  final ParticleCurve curve;

  @override
  String toString() => 'InheritVelocity(${mode.name}, $curve)';
}

/// Particles that bounce off [planes] (Unity's *Collision* module, with
/// planes): each plane an object's, through its position and facing its
/// up (+Y), without end, a floor or a wall. Its objects can be empty ones
/// (`GameObject(name: 'floor')`), and move. At most 6.
///
/// A particle that meets one from the front, within its radius (half its
/// size, times [radiusScale]), bounces: [bounce] of its speed into the
/// plane comes back out, then it loses [dampen] of its speed and
/// [lifetimeLoss] of its lifetime; one slower than [minKillSpeed]
/// afterwards dies. With [sendCollisionMessages], the system's
/// [ParticleSystem.onParticleCollision] is told how many hit each plane.
final class ParticleCollision {
  /// A full bounce off [planes], nothing lost, unless given (Unity's).
  const ParticleCollision({
    required this.planes,
    this.dampen = const ParticleCurve(0),
    this.bounce = const ParticleCurve(1),
    this.lifetimeLoss = const ParticleCurve(0),
    this.minKillSpeed = 0,
    this.radiusScale = 1,
    this.sendCollisionMessages = false,
  });

  /// At most 6, each added to the world before the system's object.
  final List<GameObject> planes;

  /// How much of its speed it loses as it hits, 0 to 1.
  final ParticleCurve dampen;

  /// How much of its speed into the plane comes back out: 1 for a full
  /// bounce, 0 to stop on the plane.
  final ParticleCurve bounce;

  /// How much of its lifetime it loses as it hits, 0 to 1: 1 dies there.
  final ParticleCurve lifetimeLoss;

  /// Slower than this after a hit, in metres a second, it dies.
  final double minKillSpeed;

  /// Its radius against a plane, times half its size.
  final double radiusScale;

  /// Calls the system's [ParticleSystem.onParticleCollision] after each
  /// frame with hits.
  final bool sendCollisionMessages;

  bool get _finite =>
      dampen._finite && bounce._finite && lifetimeLoss._finite && minKillSpeed.isFinite && radiusScale.isFinite;

  @override
  String toString() => 'ParticleCollision(${planes.length} planes, bounce: $bounce, dampen: $dampen)';
}

/// When a [SubEmitter] emits (Unity's `ParticleSystemSubEmitterType`).
enum SubEmitterType {
  /// From each particle all its life, at the sub system's
  /// [Emission.rateOverTime] for each: a firework's trail.
  birth,

  /// Where one hits a plane, its sub system's bursts' counts: a splash.
  collision,

  /// Where one dies, its sub system's bursts' counts: a firework's burst.
  death,
}

/// What a [SubEmitter]'s particles take from the particle they come from,
/// times their own (Unity's `ParticleSystemSubEmitterProperties`).
enum SubEmitterInherit {
  /// Its colour, times the sub system's start colour.
  color,

  /// Its size, times the sub system's start size.
  size,

  /// Its rotation, added to the sub system's start rotation.
  rotation,
}

/// Another system that emits where this one's particles are born, hit or
/// die (Unity's *Sub Emitters* module): its [system] emits only so,
/// never on its own timeline, from wherever its own object is. Each
/// time with [probability], 0 to 1. Its particles start where the
/// particle was, with the sub system's own settings and its
/// [InheritVelocity] of the particle's velocity, in world space.
final class SubEmitter {
  /// [system] emitting at each [type] of event, inheriting nothing, every
  /// time, unless given.
  const SubEmitter(this.system, this.type, {this.inherit = const {}, this.probability = 1});

  /// On an object of its own, which can be this one's child.
  final ParticleSystem system;

  /// As each particle is born, hits a plane or dies.
  final SubEmitterType type;

  /// What its particles take from the particle they come from.
  final Set<SubEmitterInherit> inherit;

  /// How likely each time is, 0 to 1.
  final double probability;

  @override
  String toString() => 'SubEmitter(${type.name}, $system)';
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

/// A number over each particle's life (Unity's `MinMaxCurve`): a
/// constant, one at random between two, a curve, or one at random between
/// two curves. Each particle keeps its own pick all its life. A curve is
/// any function of 0 (born) to 1 (gone), read at 64 points when it's made,
/// not called each frame.
final class ParticleCurve {
  /// [value] for every particle, all its life.
  const ParticleCurve(double value) : _min = value, _max = value, _tables = null;

  /// One at random from [min] to [max] for each particle, all its life.
  const ParticleCurve.between(double min, double max) : _min = min, _max = max, _tables = null;

  /// [curve] over each particle's life, times [scale].
  ParticleCurve.curve(EaseFunction curve, {double scale = 1}) : this._curves(curve, curve, scale);

  /// One at random between [a] and [b], over each particle's life, times
  /// [scale]: each particle somewhere between the two.
  ParticleCurve.curves(EaseFunction a, EaseFunction b, {double scale = 1}) : this._curves(a, b, scale);

  ParticleCurve._curves(EaseFunction a, EaseFunction b, double scale)
    : _min = 0,
      _max = 0,
      _tables = Float32List(2 * FY_PARTICLE_TABLE) {
    for (var i = 0; i < FY_PARTICLE_TABLE; i++) {
      final t = i / (FY_PARTICLE_TABLE - 1);
      _tables![i] = a(t) * scale;
      _tables[FY_PARTICLE_TABLE + i] = b(t) * scale;
    }
  }

  final double _min, _max;
  final Float32List? _tables;

  /// The least it is, anywhere.
  double get _least => _tables == null ? math.min(_min, _max) : _tables.reduce(math.min);

  bool get _finite => _tables == null ? _min.isFinite && _max.isFinite : _tables.every((v) => v.isFinite);

  void _write(FyCurve to) {
    final tables = _tables;
    if (tables == null) {
      to
        ..mode = FY_CURVE_CONSTANT
        ..min = _min
        ..max = _max;
      return;
    }
    to.mode = FY_CURVE_CURVES;
    for (var i = 0; i < FY_PARTICLE_TABLE; i++) {
      to.tables[0][i] = tables[i];
      to.tables[1][i] = tables[FY_PARTICLE_TABLE + i];
    }
  }

  @override
  String toString() => _tables != null
      ? 'ParticleCurve.curves(…)'
      : _min == _max
      ? 'ParticleCurve($_min)'
      : 'ParticleCurve.between($_min, $_max)';
}

/// A vector over each particle's life, an axis each, in [space]: Unity's
/// *Velocity over Lifetime* ([ParticleSystem.velocityOverLifetime], metres
/// a second added to how it moves) and *Force over Lifetime*
/// ([ParticleSystem.forceOverLifetime], metres a second added to its
/// speed each second). In [ParticleSpace.local], it turns with the
/// object.
final class ParticleVector {
  /// Nothing on each axis, in the object's space, unless given.
  const ParticleVector({
    this.x = const ParticleCurve(0),
    this.y = const ParticleCurve(0),
    this.z = const ParticleCurve(0),
    this.space = ParticleSpace.local,
  });

  /// Each axis's.
  final ParticleCurve x, y, z;

  /// The object's space (turning with it) or the world's.
  final ParticleSpace space;

  bool get _finite => x._finite && y._finite && z._finite;

  void _write(FyParticleVector to) {
    x._write(to.x);
    y._write(to.y);
    z._write(to.z);
    to.space = space == ParticleSpace.world ? FY_PARTICLE_SPACE_WORLD : FY_PARTICLE_SPACE_LOCAL;
  }

  @override
  String toString() => 'ParticleVector(x: $x, y: $y, z: $z, space: ${space.name})';
}

/// Unity's *Limit Velocity over Lifetime*: a particle faster than [limit]
/// metres a second is brought down towards it by [dampen], 0 (not at all)
/// to 1 (to the limit at once), the same at any frame rate; and [drag]
/// slows every particle by that much of its speed a second, as air does.
final class LimitVelocity {
  /// A limit of 1 m/s, not dampened, and no drag, unless given.
  const LimitVelocity({this.limit = const ParticleCurve(1), this.dampen = 0, this.drag = const ParticleCurve(0)});

  /// The most speed, in metres a second.
  final ParticleCurve limit;

  /// How much of the speed past the limit goes, 0 to 1.
  final double dampen;

  /// How much of its speed it loses a second.
  final ParticleCurve drag;

  @override
  String toString() => 'LimitVelocity(limit: $limit, dampen: $dampen, drag: $drag)';
}

/// Unity's *Size by Speed*: each particle's size times [size] of how fast
/// it goes, from [minSpeed] (`size(0)`) to [maxSpeed] (`size(1)`) metres
/// a second.
final class SizeBySpeed {
  /// [size] from [minSpeed] to [maxSpeed].
  SizeBySpeed(this.size, {this.minSpeed = 0, this.maxSpeed = 1}) : _table = ParticleSystem._sizes(size)!;

  /// Its size's multiplier, from the slowest (0) to the fastest (1).
  final EaseFunction size;

  /// The speeds, in metres a second, of its start and end.
  final double minSpeed, maxSpeed;
  final Float32List _table;

  @override
  String toString() => 'SizeBySpeed(minSpeed: $minSpeed, maxSpeed: $maxSpeed)';
}

/// Unity's *Color by Speed*: each particle's colour times [gradient] of
/// how fast it goes, from [minSpeed] to [maxSpeed] metres a second.
final class ColorBySpeed {
  /// [gradient] from [minSpeed] to [maxSpeed].
  ColorBySpeed(this.gradient, {this.minSpeed = 0, this.maxSpeed = 1}) : _table = ParticleSystem._colors(gradient)!;

  /// Its colour's multiplier, from the slowest to the fastest.
  final ColorGradient gradient;

  /// The speeds, in metres a second, of its start and end.
  final double minSpeed, maxSpeed;
  final Float32List _table;

  @override
  String toString() => 'ColorBySpeed(minSpeed: $minSpeed, maxSpeed: $maxSpeed)';
}

/// Unity's *Noise*: each particle wanders through a smooth random field,
/// as smoke curls and embers drift. [strength] is how hard it pushes, in
/// metres a second; [frequency] how many swirls a metre; [scrollSpeed] how
/// fast the field itself changes; [octaves], 1 to 4, layers of finer
/// detail.
final class ParticleNoise {
  /// Unity's: a strength of 1, half a swirl a metre, still, one octave.
  const ParticleNoise({
    this.strength = const ParticleCurve(1),
    this.frequency = 0.5,
    this.scrollSpeed = 0,
    this.octaves = 1,
  });

  /// How hard it pushes, in metres a second.
  final ParticleCurve strength;

  /// Swirls a metre, and how fast the field changes, a second.
  final double frequency, scrollSpeed;

  /// Layers of finer detail, 1 to 4.
  final int octaves;

  @override
  String toString() => 'ParticleNoise(strength: $strength, frequency: $frequency, octaves: $octaves)';
}

/// Unity's *Texture Sheet Animation*: the [ParticleSystem.material]'s
/// texture is a grid of [columns] × [rows] frames, read left to right and
/// top to bottom, and each particle shows one: a flame's flicker, a puff
/// of smoke growing. [frameOverTime] says which through its life, 0 the
/// first frame and 1 past the last (every frame, once, unless given),
/// [cycles] times a life, from [startFrame] frames on. With [randomRow],
/// each particle plays one row, chosen at random: rows of different
/// puffs.
final class TextureSheet {
  /// One frame, the whole texture, unless given.
  TextureSheet({
    this.columns = 1,
    this.rows = 1,
    ParticleCurve? frameOverTime,
    this.startFrame = const Range(0),
    this.cycles = 1,
    this.randomRow = false,
  }) : frameOverTime = frameOverTime ?? ParticleCurve.curve((t) => t);

  /// The grid's frames across and down.
  final int columns, rows;

  /// Which frame, over a life: 0 the first, 1 past the last.
  final ParticleCurve frameOverTime;

  /// How many frames on it starts, a random pick for each particle.
  final Range startFrame;

  /// How many times it goes through [frameOverTime] in a life.
  final double cycles;

  /// Each particle plays one row, chosen at random.
  final bool randomRow;

  @override
  String toString() => 'TextureSheet($columns × $rows, cycles: $cycles)';
}

enum _RenderKind { billboard, stretched, horizontal, vertical, mesh }

/// How a [ParticleSystem]'s particles are drawn (Unity's renderer
/// module's render mode).
final class ParticleRenderMode {
  /// A square facing the camera. The default.
  static const billboard = ParticleRenderMode._(_RenderKind.billboard);

  /// Lying flat, facing up: a ripple, a ring on the ground.
  static const horizontal = ParticleRenderMode._(_RenderKind.horizontal);

  /// Standing up, turned round up to face the camera: a flame that
  /// doesn't lean back as the camera looks down.
  static const vertical = ParticleRenderMode._(_RenderKind.vertical);

  /// Facing the camera, but long the way it moves: [lengthScale] times
  /// its size, plus [speedScale] times its speed. Sparks, rain.
  const ParticleRenderMode.stretched({this.lengthScale = 2, this.speedScale = 0})
    : _kind = _RenderKind.stretched,
      mesh = null;

  /// [mesh], its size across a metre of the mesh, turned round up by its
  /// rotation: debris, coins, leaves.
  const ParticleRenderMode.mesh(Mesh this.mesh) : _kind = _RenderKind.mesh, lengthScale = 0, speedScale = 0;

  const ParticleRenderMode._(this._kind) : lengthScale = 0, speedScale = 0, mesh = null;

  final _RenderKind _kind;

  /// A stretched particle's length: its size times [lengthScale], plus
  /// its speed times [speedScale].
  final double lengthScale, speedScale;

  /// A mesh particle's mesh.
  final Mesh? mesh;

  @override
  bool operator ==(Object other) =>
      other is ParticleRenderMode &&
      other._kind == _kind &&
      other.lengthScale == lengthScale &&
      other.speedScale == speedScale &&
      identical(other.mesh, mesh);

  @override
  int get hashCode => Object.hash(_kind, lengthScale, speedScale, mesh);

  @override
  String toString() => switch (_kind) {
    _RenderKind.stretched => 'ParticleRenderMode.stretched(lengthScale: $lengthScale, speedScale: $speedScale)',
    _RenderKind.mesh => 'ParticleRenderMode.mesh($mesh)',
    _ => 'ParticleRenderMode.${_kind.name}',
  };
}

/// Smoke, dust, sparks, fire, rain and bursts of light, as Unity's
/// Particle System: a component that makes particles from its [shape],
/// at its [emission]'s rate and in its bursts, each living a
/// [startLifetime] and moving at a [startSpeed], drawn with its
/// [material] as squares that face the camera (or as its [renderMode]
/// says). Its modules change them over their lives, as Unity's do:
/// [velocityOverLifetime], [limitVelocity], [forceOverLifetime],
/// [rotationOverLifetime], [sizeBySpeed], [colorBySpeed], [noise], a
/// [textureSheet], [inheritVelocity], [collision] with planes and
/// [subEmitters]; null (or empty) is a module turned off.
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
    this._velocityOverLifetime,
    this._limitVelocity,
    this._forceOverLifetime,
    this._rotationOverLifetime,
    this._sizeBySpeed,
    this._colorBySpeed,
    this._noise,
    this._textureSheet,
    this._material,
    this._renderMode = ParticleRenderMode.billboard,
    this._lit = false,
    this._softness = 0,
    this._inheritVelocity,
    this._collision,
    this._subEmitters = const [],
    this.onStopped,
    this.onParticleCollision,
  }) : _sizeOverLifetime = sizeOverLifetime,
       _sizeTable = _sizes(sizeOverLifetime),
       _colorOverLifetime = colorOverLifetime,
       _colorTable = _colors(colorOverLifetime),
       super._() {
    if (_refused() ?? _destroyedPlane(_collision) case final error?) throw error;
  }

  /// Called when it's done, with [StopAction.callback] (Unity's
  /// `OnParticleSystemStopped`).
  void Function()? onStopped;

  /// Called after each frame in which its particles hit a plane, with
  /// [ParticleCollision.sendCollisionMessages] on: the plane's object and
  /// how many hit it (Unity's `OnParticleCollision`).
  void Function(GameObject plane, int count)? onParticleCollision;

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
  ParticleVector? _velocityOverLifetime;
  LimitVelocity? _limitVelocity;
  ParticleVector? _forceOverLifetime;
  ParticleCurve? _rotationOverLifetime;
  SizeBySpeed? _sizeBySpeed;
  ColorBySpeed? _colorBySpeed;
  ParticleNoise? _noise;
  TextureSheet? _textureSheet;
  Material? _material;
  ParticleRenderMode _renderMode;
  bool _lit;
  double _softness;
  InheritVelocity? _inheritVelocity;
  ParticleCollision? _collision;
  List<SubEmitter> _subEmitters;

  /// How long its timeline is, in seconds, from 0.05 to 100,000 (as
  /// Unity's): one round of a looping system, the whole of one that isn't.
  double get duration => _duration;
  set duration(double value) => _set(() => _duration, (v) => _duration = v, value);

  /// It plays its timeline again each time it ends.
  bool get looping => _looping;
  set looping(bool value) => _set(() => _looping, (v) => _looping = v, value);

  /// A looping system starts as if it had played a whole [duration], full:
  /// its particles have met its collision planes, but fired no sub emitters
  /// and told of no hits.
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

  /// Added to how each particle moves, over its life, in metres a
  /// second, without changing its velocity: a swirl, a drift.
  ParticleVector? get velocityOverLifetime => _velocityOverLifetime;
  set velocityOverLifetime(ParticleVector? value) =>
      _set(() => _velocityOverLifetime, (v) => _velocityOverLifetime = v, value);

  /// A speed limit and drag.
  LimitVelocity? get limitVelocity => _limitVelocity;
  set limitVelocity(LimitVelocity? value) => _set(() => _limitVelocity, (v) => _limitVelocity = v, value);

  /// Added to each particle's velocity each second, over its life: wind,
  /// an updraft.
  ParticleVector? get forceOverLifetime => _forceOverLifetime;
  set forceOverLifetime(ParticleVector? value) => _set(() => _forceOverLifetime, (v) => _forceOverLifetime = v, value);

  /// How fast each particle turns, in radians a second, over its life.
  ParticleCurve? get rotationOverLifetime => _rotationOverLifetime;
  set rotationOverLifetime(ParticleCurve? value) =>
      _set(() => _rotationOverLifetime, (v) => _rotationOverLifetime = v, value);

  /// Each particle's size by how fast it goes.
  SizeBySpeed? get sizeBySpeed => _sizeBySpeed;
  set sizeBySpeed(SizeBySpeed? value) => _set(() => _sizeBySpeed, (v) => _sizeBySpeed = v, value);

  /// Each particle's colour by how fast it goes: sparks that cool as they
  /// slow.
  ColorBySpeed? get colorBySpeed => _colorBySpeed;
  set colorBySpeed(ColorBySpeed? value) => _set(() => _colorBySpeed, (v) => _colorBySpeed = v, value);

  /// A smooth random wander.
  ParticleNoise? get noise => _noise;
  set noise(ParticleNoise? value) => _set(() => _noise, (v) => _noise = v, value);

  /// Frames of the material's texture, over each particle's life.
  TextureSheet? get textureSheet => _textureSheet;
  set textureSheet(TextureSheet? value) => _set(() => _textureSheet, (v) => _textureSheet = v, value);

  /// What its particles are drawn with: its colour and base colour
  /// texture, unlit, and its emission; [Blending.additive] for fire and
  /// sparks, else see-through by alpha. A material with its own
  /// [Material.shader] draws them with it, each particle's colour its
  /// `in.color`, and `particle_age()` and `particle_random()` for each.
  /// Without one, nothing is drawn.
  Material? get material => _material;
  set material(Material? value) => _set(() => _material, (v) => _material = v, value);

  /// How each particle is drawn: [ParticleRenderMode.billboard] unless
  /// set.
  ParticleRenderMode get renderMode => _renderMode;
  set renderMode(ParticleRenderMode value) => _set(() => _renderMode, (v) => _renderMode = v, value);

  /// Lit by the scene's lights as a surface facing the camera, with the
  /// material's metal and roughness, rather than shown as it is: smoke and
  /// dust in the sun.
  bool get lit => _lit;
  set lit(bool value) => _set(() => _lit, (v) => _lit = v, value);

  /// How far, in metres, each particle fades out in front of what's
  /// behind it (Unity's soft particles), so smoke doesn't cut a hard line
  /// into the ground; 0 for a hard edge. A frame with soft particles keeps
  /// the solid scene's depth to read, which costs a little.
  double get softness => _softness;
  set softness(double value) => _set(() => _softness, (v) => _softness = v, value);

  /// A particle takes its object's velocity, as it's born or all its
  /// life.
  InheritVelocity? get inheritVelocity => _inheritVelocity;
  set inheritVelocity(InheritVelocity? value) => _set(() => _inheritVelocity, (v) => _inheritVelocity = v, value);

  /// Its particles bounce off planes. A plane whose object is destroyed
  /// later is left out; one destroyed already is refused.
  ParticleCollision? get collision => _collision;
  set collision(ParticleCollision? value) {
    if (_destroyedPlane(value) case final error?) throw error;
    _set(() => _collision, (v) => _collision = v, value);
  }

  static ArgumentError? _destroyedPlane(ParticleCollision? collision) =>
      collision != null && collision.planes.any((p) => p.isDestroyed)
      ? ArgumentError('collision: a plane\'s object is destroyed')
      : null;

  /// Other systems that emit where its particles are born, hit a plane or
  /// die: at most 4.
  List<SubEmitter> get subEmitters => _subEmitters;
  set subEmitters(List<SubEmitter> value) => _set(() => _subEmitters, (v) => _subEmitters = v, value);

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

  /// Makes [count] particles now, as its shape would, playing or not: as
  /// many of them as fit in [maxParticles].
  void emit(int count) {
    if (count < 0) throw ArgumentError.value(count, 'count', 'must be 0 or more');
    _command(FY_PARTICLES_EMIT, count);
  }

  /// Moves it [seconds] on at once, as if that much time had passed: an
  /// effect seen half-way through. In steps of 1/30 s; past a minute, in
  /// longer ones, so a long span costs no more than a minute's.
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
    if (_sizeBySpeed?._table case final t? when t.any((v) => !v.isFinite)) {
      return ArgumentError('sizeBySpeed must give finite numbers from 0 to 1');
    }
    final curves = [
      _velocityOverLifetime?._finite,
      _forceOverLifetime?._finite,
      _rotationOverLifetime?._finite,
      _limitVelocity?.limit._finite,
      _limitVelocity?.drag._finite,
      _noise?.strength._finite,
      _textureSheet?.frameOverTime._finite,
    ];
    if (curves.contains(false)) return ArgumentError('a ParticleSystem\'s curves must be finite');
    if (_limitVelocity case final l? when l.limit._least < 0 || l.drag._least < 0 || l.dampen < 0 || l.dampen > 1) {
      return ArgumentError('limitVelocity: limit and drag can\'t be below 0, and dampen is 0 to 1; got $l');
    }
    if (_noise case final n? when n.octaves < 1 || n.octaves > 4) {
      return ArgumentError.value(n.octaves, 'noise.octaves', 'must be 1 to 4');
    }
    if (_textureSheet case final t? when t.columns < 1 || t.columns > 64 || t.rows < 1 || t.rows > 64) {
      return ArgumentError('textureSheet: 1 to 64 columns and rows; got $t');
    }
    if (_renderMode.mesh case final mesh? when mesh.isDisposed) {
      return ArgumentError('renderMode: its mesh is disposed');
    }
    if (!_emission.rateOverDistance.isFinite || _emission.rateOverDistance < 0) {
      return ArgumentError.value(_emission.rateOverDistance, 'rateOverDistance', 'must be 0 or more');
    }
    if (_inheritVelocity case final i? when !i.curve._finite) {
      return ArgumentError('inheritVelocity\'s curve must be finite');
    }
    if (_collision case final c?) {
      if (c.planes.length > 6) return ArgumentError('a ParticleSystem collides with at most 6 planes');
      if (!c._finite) return ArgumentError('collision\'s numbers must be finite');
    }
    if (_subEmitters.length > 4) return ArgumentError('a ParticleSystem has at most 4 sub emitters');
    for (final sub in _subEmitters) {
      if (identical(sub.system, this)) return ArgumentError('a ParticleSystem can\'t be its own sub emitter');
      if (!sub.probability.isFinite || sub.probability < 0 || sub.probability > 1) {
        return ArgumentError.value(sub.probability, 'probability', 'must be 0 to 1');
      }
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
    if (_waitingParticles.isNotEmpty && _waitingShape != _shape) _linkWaitingParticles();
    if (_particleSystems.isEmpty && _trails.isEmpty) return;
    _sendTransforms();
    scratch.numbers[0] = time._delta;
    scratch.numbers[1] = time._unscaledDelta;
    _particleWatch.start();
    check(fy_world_particles_step(pointer, scratch.numbers, scratch.secondNumber, scratch.count));
    _particleWatch.stop();
    final count = scratch.count.value;
    if (count > 0) _particleEvents(count);
  }

  /// Writes again the systems whose sub emitters or planes weren't in the
  /// world when they were written, now that they may be.
  void _linkWaitingParticles() {
    final waiting = _waitingParticles.toList();
    _waitingParticles.clear();
    for (final system in waiting) {
      if (system.isRemoved || !system._enabledFlag) continue;
      system._live?._writeParticles(system);
    }
    _waitingShape = _shape;
  }

  void _particleEvents(int count) {
    check(fy_particle_events_get(pointer, scratch.particleEvents(count), count, scratch.count));
    // Read one at a time, a word at a time, without a list or a closure,
    // as collisions come every frame: nothing a game does here writes to
    // the buffer.
    assert(sizeOf<FyParticleEvent>() == 32);
    for (var i = 0; i < count; i++) {
      final words = scratch.particleEventWords;
      final halves = scratch.particleEventHalves;
      final id = words[i * 4];
      final kind = halves[i * 8 + 2];
      final action = halves[i * 8 + 3];
      final plane = words[i * 4 + 2];
      final hits = halves[i * 8 + 6];
      final system = _particleSystems[id];
      if (system == null || system.isRemoved) continue;
      final object = system.gameObject;
      if (kind == FY_PARTICLE_EVENT_COLLISION) {
        final planeObject = _objects[plane];
        final onCollision = system.onParticleCollision;
        if (onCollision == null || planeObject == null) continue;
        try {
          onCollision(planeObject, hits);
        } catch (error, stack) {
          _report(object, error, stack);
        }
      } else {
        switch (action) {
          case FY_PARTICLE_STOP_DISABLE:
            object.active = false;
          case FY_PARTICLE_STOP_DESTROY:
            object.destroy();
          case FY_PARTICLE_STOP_CALLBACK:
            if (system.onStopped case final onStopped?) _safely(object, onStopped);
        }
      }
      if (_disposeAsked) return;
    }
  }
}
