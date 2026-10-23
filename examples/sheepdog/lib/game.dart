// The game: a dog, a flock and a pen. Move with W, A, S, D (or the arrow
// keys; the settings page changes the letters), or drag anywhere as a
// stick. The flock keeps together and walks away from the dog, faster the
// nearer it is: walk behind it to drive it. Space, a tap or the Bark
// button barks: the sheep near the dog run, and a wolf near it runs off.
// Bring the flock into the pen, through its gate, before the time is up.

import 'dart:math' as math;

import 'package:flutter/foundation.dart' show ValueListenable, ValueNotifier;
import 'package:flutter/material.dart' show Brightness, ThemeData;
import 'package:flutter/widgets.dart' show Alignment;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart'
    show Controls, FlutterColorToFynkel, ListenableSignal, WorldTheme, WorldWidget;

import 'effects.dart';
import 'labels.dart' show HealthBar, NameTag;

/// Half the field's width: it goes from -size to size on x and z.
const size = 12.0;

/// The pen is the field's back middle: x between -penHalf and penHalf,
/// z behind penFront, with its gate the whole front.
const penHalf = 4.5, penFront = -5.0;

/// The layer of what's only for show, the bark's ring and the grass: the
/// minimap leaves it out.
const effects = 1;

/// The pond in the field's front left corner: its middle, and how far
/// its water reaches. Nothing walks into it.
const pond = Vec3(-8, 0, 7.5), pondRadius = 2.2;

/// The flock's names, one per sheep.
const names = ['Dolly', 'Shaun', 'Timmy', 'Flossie', 'Larry', 'Nuts', 'Hazel', 'Pidsley'];

/// The quiet seconds at the start of a round before the first wolf, and
/// how long before each wolf comes its mark shows where.
const calm = 15.0, warning = 3.0;

/// A round at [level]: how many seconds it lasts, and how many wolves come
/// in it, one at a time. Level 1 has none.
({double seconds, int wolves}) levelOf(int level) =>
    (seconds: level == 1 ? 90 : 120, wolves: math.min(2 * (level - 1), 8));

/// The stars for bringing [home] sheep home: one for half the flock, two
/// for six, three for all of it.
int starsFor(int home) => home == names.length
    ? 3
    : home >= 6
    ? 2
    : home >= 4
    ? 1
    : 0;

/// Whether [p] is in the pen.
bool inPen(Vec3 p) => p.x.abs() < penHalf && p.z < penFront;

/// Where something going from [from] to [to] on the floor ends up: inside
/// the field, and across the pen's fence only through its gate (never, for
/// a wolf: [intoPen] false). Against a fence it slides along it.
Vec3 walk(Vec3 from, Vec3 to, {bool intoPen = true}) {
  for (final p in [to, Vec3(to.x, 0, from.z), Vec3(from.x, 0, to.z)]) {
    final at = _dry(Vec3(p.x.clamp(-size + 0.5, size - 0.5), from.y, p.z.clamp(-size + 0.5, size - 0.5)));
    if (_mayStep(from, at, intoPen)) return at;
  }
  return from;
}

/// [p], or the nearest place to it on the pond's bank if it's in the
/// water: what walks into the pond slides round it.
Vec3 _dry(Vec3 p) {
  final d = Vec3(p.x - pond.x, 0, p.z - pond.z);
  final far = d.length;
  if (far >= pondRadius) return p;
  final out = far < 1e-6 ? Vec3(1, 0, 0) : d / far;
  return Vec3(pond.x + out.x * pondRadius, p.y, pond.z + out.z * pondRadius);
}

bool _mayStep(Vec3 from, Vec3 to, bool intoPen) {
  final wasIn = inPen(from), isIn = inPen(to);
  if (wasIn == isIn) return true;
  if (isIn && !intoPen) return false;
  final outside = wasIn ? to : from;
  return outside.z >= penFront && outside.x.abs() < penHalf - 0.3;
}

/// The keys the player can change on the settings page; the arrow keys
/// always move too.
Controls sheepdogControls() =>
    Controls({'up': KeyCode.w, 'down': KeyCode.s, 'left': KeyCode.a, 'right': KeyCode.d, 'bark': KeyCode.space});

/// The root: the lights, the cameras, what the HUD shows, and the round
/// being played. Its colours are the app's theme's.
class SheepdogGame extends GameObject {
  /// [seed] makes every round the same, for tests; the first round is at
  /// [level]. [quality] is the settings page's: the world's quality level,
  /// null for the device's own pick.
  SheepdogGame({this.seed, Controls? controls, this.quality, int level = 1})
    : controls = controls ?? sheepdogControls(),
      level = ValueNotifier(level),
      clock = ValueNotifier(levelOf(level).seconds.ceil());

  final int? seed;
  final Controls controls;
  final ValueListenable<Quality?>? quality;

  /// Sheep in the pen, and taken by wolves.
  final home = ValueNotifier(0), lost = ValueNotifier(0);

  /// The level being played, and the seconds left in its round.
  final ValueNotifier<int> level, clock;

  /// Whether a wolf's mark is out: one comes in a moment.
  final coming = ValueNotifier(false);

  /// How many sheep came home, once the round is over (every sheep home
  /// or taken, or the time up); null while it's played.
  final result = ValueNotifier<int?>(null);

  /// The floor and the fences, coloured by [wear]. The floor turns to
  /// sand where it dips under the pond (assets/shaders/ground.wgsl).
  final grass = Material(name: 'grass', roughness: 1), fence = Material(name: 'fence', roughness: 0.8);

  /// How far the grass's tips bend in the wind, in metres.
  static const wind = 0.12;

  /// The grass's blades, swaying in the wind (assets/shaders/grass.wgsl),
  /// the colour of the floor.
  late final Material blades;

  /// The pond's water (assets/shaders/pond.wgsl): it shows the sandy bed
  /// under it, bent by its ripples.
  late final Material water;

  /// The pond's water at low quality: see-through, without reading what's
  /// behind it. Reading it costs a slower phone (a Note10+) about six
  /// frames a second (docs/stage-10-report.md).
  final stillWater = Material(
    name: 'still water',
    color: const Color(0xB00C4868),
    roughness: 0.06,
    blending: Blending.transparent,
  );

  /// The pond's water at the world's quality.
  Material get pondWater => (world.quality ?? Quality.device) == Quality.low ? stillWater : water;

  /// The wolves' coat and eyes: each wolf burns away on its own when it's
  /// chased off (assets/shaders/burn.wgsl).
  late final Material wolfCoat, wolfEyes;

  /// The floor, with its dip for the pond, and the grass's patches: made
  /// once, for every round.
  late final Mesh field = _field();
  late final List<(Vec3, Mesh)> patches = _patches();

  /// The pond's water: wider than the pond, its edges under the bank.
  late final Mesh pondMesh = _waterMesh(2 * pondRadius + 1.6, 40);

  late final Shader _flash;
  final _flashing = <Material, Material>{};

  /// A copy of a sheep's [plain] material that flashes when the sheep is
  /// bitten (assets/shaders/flash.wgsl): one for each, shared by the
  /// flock.
  Material flashing(Material plain) => _flashing[plain] ??= plain.copy(name: '${plain.name} flash')..shader = _flash;

  /// The minimap's camera, high above the field and looking straight down:
  /// no glow and no darkened corners, which it's too small to show.
  final map = Camera(orthographic: true, orthographicSize: size + 0.5, cullingMask: LayerMask.except([effects]));

  /// The particles' materials: dust, embers, the puff where a sheep
  /// comes home (effects.dart).
  late final ParticleLooks particles;

  late final Light _sun, _ambient;

  /// The lamp posts' lights and their bulbs' glass: lit at night.
  final _lamps = <Light>[];
  final _bulb = Material(name: 'bulb', color: Color.black, emissive: const Color(0xFFFFC880));
  late final DogCamera camera;
  late Round round;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    // The farm animals' models, held here for the whole game: each round's
    // sheep and dog take them from here, not from the files again. (Each
    // asks for its own, and hears of a failure there.)
    assets.preload(['sheep.glb', 'dog.glb']).ignore();
    final (ground, grassShader, pondShader, burn, flash, looks) = await (
      assets.shader('shaders/ground.wgsl'),
      assets.shader('shaders/grass.wgsl'),
      assets.shader('shaders/pond.wgsl'),
      assets.shader('shaders/burn.wgsl'),
      assets.shader('shaders/flash.wgsl'),
      ParticleLooks.load(assets),
    ).wait;
    particles = looks;
    grass
      ..shader = ground
      ..set('sand', const Color(0xFFC8B088));
    // One material for every blade, both sides drawn.
    blades = Material(
      name: 'blades',
      shader: grassShader,
      roughness: 0.9,
      doubleSided: true,
      values: {'wind': wind},
      reach: wind,
    );
    water = Material(
      name: 'water',
      shader: pondShader,
      roughness: 0.06,
      values: {'deep': const Color(0xFF0C4868), 'foam': 0.04, 'bend': 0.03},
      reach: 0.03,
    );
    // Cut out where they've burnt, their shadows too.
    final embers = {'edge': const Color(0xFFFF7A20)};
    wolfCoat = Material(
      name: 'wolf coat',
      shader: burn,
      color: Color(0xFF5A5F6B),
      roughness: 0.9,
      alphaCutoff: 0.5,
      values: embers,
    );
    wolfEyes = Material(
      name: 'wolf eyes',
      shader: burn,
      color: Color(0xFFFFD040),
      emissive: Color(0xFFFFB000),
      emissiveIntensity: 2,
      alphaCutoff: 0.5,
      values: embers,
    );
    _flash = flash;
    // The sun's (and the moon's) shadows: the sheep and the dog on the
    // grass, the fences across it.
    _sun = addWith(Light.sun(direction: Vec3(-0.4, -1, -0.6), shadows: true));
    _ambient = addWith(Light.ambient());
    // Lamp posts along the fences: at the pen's gate, two down each side,
    // and two along the front.
    for (final at in [
      Vec3(-penHalf, 0, penFront),
      Vec3(penHalf, 0, penFront),
      for (final x in [-size, size]) ...[Vec3(x, 0, -3), Vec3(x, 0, 6)],
      Vec3(-6, 0, size),
      Vec3(6, 0, size),
    ]) {
      add(
        GameObject(
          name: 'lamp post',
          transform: Transform(position: at + Vec3(0, 1.1, 0), scale: Vec3(0.12, 2.2, 0.12)),
          components: [MeshRenderer(Mesh.cube(), fence)],
        ),
      );
      final lamp = add(
        GameObject(
          name: 'lamp',
          transform: Transform(position: at + Vec3(0, 2.35, 0), scale: Vec3.all(0.3)),
          components: [
            MeshRenderer(Mesh.sphere(), _bulb),
            Light.point(color: const Color(0xFFFFC880), intensity: 0, range: 10),
          ],
        ),
      );
      _lamps.add(lamp.get<Light>()!);
    }
    // Before the round, which picks its pond's water by it.
    if (quality case final quality?) {
      world.quality = quality.value;
      on(quality.changed, (_) {
        world.quality = quality.value;
        for (final r in world.all<MeshRenderer>().where((r) => r.material == water || r.material == stillWater)) {
          r.material = pondWater;
        }
      });
    }
    round = add(Round(this, math.Random(seed)));
    // The main camera is the first added; the minimap's comes after.
    camera = add(DogCamera(round.dog));
    add(
      GameObject(
        name: 'minimap camera',
        transform: Transform(
          position: Vec3(0, 30, 0),
          rotation: Quat.lookRotation(Vec3(0, -1, 0), up: Vec3(0, 0, -1)),
        ),
        components: [map],
      ),
    );
    controls.bindTo(input);
    on(controls.changed, (_) => controls.bindTo(input));
    wear(world.theme);
    on(world.onThemeChanged, wear);
  }

  /// Takes the game's colours from the app's [theme]: a day in the light
  /// theme, a night in the dark one.
  void wear(ThemeData theme) {
    final scheme = theme.colorScheme;
    final dark = theme.brightness == Brightness.dark;
    // The sky, from the theme: it lights the field too, so the ambient
    // light below is a little extra.
    final sky = GradientSky(
      zenith: (dark ? scheme.surfaceContainerLowest : scheme.tertiary).toFynkel(),
      horizon: (dark ? scheme.surfaceContainerHigh : scheme.tertiaryContainer).toFynkel(),
      ground: (dark ? scheme.surfaceContainerLowest : scheme.primaryContainer).toFynkel(),
      lighting: dark ? 0.5 : 0.8,
    );
    world.sky = sky;
    // Far off, the field melts into the horizon.
    world.fog = const Fog(start: 18, end: 60);
    map
      ..showSky = false
      ..clearColor = sky.ground;
    grass.color = (dark ? scheme.onPrimaryFixedVariant : scheme.primary).toFynkel();
    blades.color = grass.color;
    fence.color = (dark ? scheme.outline : scheme.secondaryContainer).toFynkel();
    // The moon is pale blue.
    final light = dark ? Color.rgb(0.65, 0.75, 1) : Color.white;
    _sun
      ..intensity = dark ? 1.1 : 2.6
      ..color = light;
    _ambient
      ..intensity = dark ? 0.15 : 0.25
      ..color = light;
    // The lamps come on at night.
    for (final lamp in _lamps) {
      lamp.intensity = dark ? 12 : 0;
    }
    _bulb.emissiveIntensity = dark ? 4 : 0;
  }

  /// A new round, at [level] (this one's if not given): the flock back in
  /// the field, no wolves, the clock full.
  Future<void> restart({int? level}) async {
    this.level.value = level ?? this.level.value;
    round.destroy();
    round = add(Round(this, math.Random(seed)));
    camera.follow(round.dog);
    home.value = 0;
    lost.value = 0;
    clock.value = round.rules.seconds.ceil();
    coming.value = false;
    result.value = null;
    await round.loaded;
  }

  /// How far the ground dips under the pond's middle, and how far round
  /// it it starts to.
  static const _dip = 0.8, _dipRadius = pondRadius + 0.4;

  /// How high the ground is at [x], [z]: flat, but for the dip of the
  /// pond, steepest at its bank.
  static double groundAt(double x, double z) {
    final d = Vec3(x - pond.x, 0, z - pond.z).length / _dipRadius;
    if (d >= 1) return 0;
    final t = 1 - d * d;
    return -_dip * t * t;
  }

  /// The pond's water level: where it meets the dip is its bank, about
  /// [pondRadius] from its middle.
  static const waterLevel = -0.06;

  /// The floor, the field's whole size, in quarter-metre squares, dipping
  /// for the pond.
  static Mesh _field() {
    const across = 96;
    final positions = <Vec3>[];
    for (var j = 0; j <= across; j++) {
      for (var i = 0; i <= across; i++) {
        final x = (i / across * 2 - 1) * size, z = (j / across * 2 - 1) * size;
        positions.add(Vec3(x, groundAt(x, z), z));
      }
    }
    return Mesh.custom(positions: positions, indices: _squares(across, across));
  }

  /// The grass: a patch a mesh, each of many blades, 4 m across, with none
  /// in the pond. The blades' places are the same every game.
  static List<(Vec3, Mesh)> _patches() {
    final random = math.Random(7);
    const across = 6, side = 2 * size / across, perPatch = 200;
    final patches = <(Vec3, Mesh)>[];
    for (var pj = 0; pj < across; pj++) {
      for (var pi = 0; pi < across; pi++) {
        final middle = Vec3(-size + (pi + 0.5) * side, 0, -size + (pj + 0.5) * side);
        final positions = <Vec3>[], uvs = <Vec2>[], indices = <int>[];
        for (var b = 0; b < perPatch; b++) {
          final at = Vec3((random.nextDouble() - 0.5) * side, 0, (random.nextDouble() - 0.5) * side);
          final world = middle + at;
          final angle = random.nextDouble() * math.pi, tall = 0.25 + random.nextDouble() * 0.25;
          if ((world - pond).flat.length < pondRadius + 0.15 ||
              world.x.abs() > size - 0.2 ||
              world.z.abs() > size - 0.2) {
            continue;
          }
          _blade(positions, uvs, indices, at, angle, tall);
        }
        if (indices.isEmpty) continue;
        patches.add((
          middle,
          Mesh.custom(
            positions: positions,
            normals: List.filled(positions.length, Vec3.up),
            uvs: uvs,
            indices: indices,
          ),
        ));
      }
    }
    return patches;
  }

  /// A blade [tall] metres high at [at], turned [angle] about up: 6 cm
  /// wide at its foot, to a point, in three steps so it bends smoothly.
  /// v is 1 at the foot.
  static void _blade(List<Vec3> positions, List<Vec2> uvs, List<int> indices, Vec3 at, double angle, double tall) {
    final first = positions.length;
    final across = Vec3(math.cos(angle), 0, math.sin(angle));
    for (var i = 0; i <= 3; i++) {
      final y = i / 3, half = 0.03 * (1 - y);
      positions
        ..add(at + across * -half + Vec3(0, y * tall, 0))
        ..add(at + across * half + Vec3(0, y * tall, 0));
      uvs
        ..add(Vec2(0, 1 - y))
        ..add(Vec2(1, 1 - y));
    }
    for (var i = 0; i < 3; i++) {
      final a = first + i * 2;
      indices.addAll([a, a + 1, a + 3, a, a + 3, a + 2]);
    }
  }
}

/// A flat grid's triangles, [across] × [down] squares, facing up.
List<int> _squares(int across, int down) {
  final indices = <int>[];
  for (var j = 0; j < down; j++) {
    for (var i = 0; i < across; i++) {
      final a = j * (across + 1) + i;
      final b = a + across + 1;
      indices.addAll([a, b, a + 1, a + 1, b, b + 1]);
    }
  }
  return indices;
}

/// A flat square of water [width] metres across, in [across] × [across]
/// squares: enough vertices for its waves.
Mesh _waterMesh(double width, int across) {
  final positions = <Vec3>[];
  for (var j = 0; j <= across; j++) {
    for (var i = 0; i <= across; i++) {
      positions.add(Vec3((i / across - 0.5) * width, 0, (j / across - 0.5) * width));
    }
  }
  return Mesh.custom(
    positions: positions,
    normals: List.filled(positions.length, Vec3.up),
    indices: _squares(across, across),
  );
}

/// One go: the field, the pen, the dog, the flock, the clock, and the
/// wolves of its level.
class Round extends GameObject {
  Round(this.game, this._random) : rules = levelOf(game.level.value), _left = levelOf(game.level.value).seconds;

  final SheepdogGame game;
  final ({double seconds, int wolves}) rules;
  final math.Random _random;
  final dog = Dog();
  final flock = <Sheep>[];
  double _left;
  Routine<void>? _pack;

  /// Over: every sheep home or taken, or the time up.
  bool get isOver => game.result.value != null;

  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        name: 'field',
        // On the ground: it shadows nothing, so it isn't drawn into the
        // shadow maps.
        components: [MeshRenderer(game.field, game.grass)..castShadows = false],
      ),
    );
    // The grass, only for show: the minimap leaves it out, and it casts no
    // shadow (thousands of blades, for little).
    for (final (i, (at, patch)) in game.patches.indexed) {
      add(
        GameObject(
          name: 'grass $i',
          transform: Transform(position: at),
          layer: effects,
          components: [MeshRenderer(patch, game.blades)..castShadows = false],
        ),
      );
    }
    add(
      GameObject(
        name: 'pond',
        transform: Transform(position: Vec3(pond.x, SheepdogGame.waterLevel, pond.z)),
        components: [MeshRenderer(game.pondMesh, game.pondWater)..castShadows = false],
      ),
    );
    // A low fence around the field, and the pen's three sides.
    for (final (at, across) in [
      (Vec3(0, 0, -size), Vec3(2 * size, 0, 0)),
      (Vec3(0, 0, size), Vec3(2 * size, 0, 0)),
      (Vec3(-size, 0, 0), Vec3(0, 0, 2 * size)),
      (Vec3(size, 0, 0), Vec3(0, 0, 2 * size)),
      (Vec3(-penHalf, 0, (penFront - size) / 2), Vec3(0, 0, size + penFront)),
      (Vec3(penHalf, 0, (penFront - size) / 2), Vec3(0, 0, size + penFront)),
    ]) {
      add(
        GameObject(
          transform: Transform(position: at + Vec3(0, 0.3, 0), scale: across + Vec3(0.2, 0.6, 0.2)),
          components: [MeshRenderer(Mesh.cube(), game.fence)],
        ),
      );
    }
    add(dog);
    // The flock together, in the middle of the field, the dog behind it.
    final centre = Vec3(_random.nextDouble() * 8 - 4, 0, 3);
    for (final name in names) {
      final angle = _random.nextDouble() * 2 * math.pi, far = 0.8 + _random.nextDouble() * 1.7;
      final at = centre + Vec3(math.cos(angle), 0, math.sin(angle)) * far;
      flock.add(add(Sheep(name, at: at, random: _random)));
    }
  }

  @override
  void onStart() {
    super.onStart();
    for (final sheep in flock) {
      on(sheep.cameHome, (_) {
        game.home.value++;
        _maybeOver();
      });
      on(sheep.taken, (_) {
        game.lost.value++;
        world.cameras.shake(0.25, seconds: 0.4);
        _maybeOver();
      });
    }
    _pack = startRoutine(_wolves);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (isOver) return;
    _left -= time.delta;
    game.clock.value = math.max(0, _left.ceil());
    if (_left <= 0) _over();
  }

  /// The round is over once no sheep is left in the field.
  void _maybeOver() {
    if (game.home.value + game.lost.value == flock.length) _over();
  }

  void _over() {
    if (isOver) return;
    _pack?.stop();
    for (final mark in world.all<WolfMark>()) {
      mark.destroy();
    }
    game.coming.value = false;
    for (final wolf in world.all<Wolf>()) {
      wolf.runOff();
    }
    game.result.value = game.home.value;
  }

  /// The level's wolves, one at a time: the first after the calm, the
  /// rest spread over the round. Each one's mark shows where it comes in
  /// for a few seconds first. It stops when the round is over, or
  /// destroyed.
  Future<void> _wolves() async {
    final apart = math.min(18.0, 85 / math.max(1, rules.wolves));
    for (var i = 0; i < rules.wolves; i++) {
      await wait((i == 0 ? calm : apart) - warning);
      final at = _edge();
      final mark = add(WolfMark(at));
      game.coming.value = true;
      await wait(warning);
      mark.destroy();
      game.coming.value = false;
      add(Wolf(this, at: at));
    }
  }

  /// Somewhere along the field's edge, but the pen's: its front, its
  /// sides, or the back beside the pen.
  Vec3 _edge() {
    final along = _random.nextDouble() * 2 - 1;
    return switch (_random.nextInt(4)) {
      0 => Vec3(along * (size - 1), 0, size - 1),
      1 => Vec3(-size + 1, 0, along * (size - 1)),
      2 => Vec3(size - 1, 0, along * (size - 1)),
      _ => Vec3(along.sign * (penHalf + 1.5 + along.abs() * (size - penHalf - 2.5)), 0, -size + 1),
    };
  }

  /// The sheep still in the field.
  Iterable<Sheep> get free => flock.where((s) => s.isFree);
}

/// Hit points: a wolf's nerve.
class Health extends Behaviour {
  Health(this.max) : _left = max;

  final int max;
  int _left;

  /// What's left after each hit.
  final hurt = Signal<int>();

  /// Once, at the hit that leaves none.
  final died = Signal<GameObject>();

  int get left => _left;
  bool get isDead => _left == 0;

  void damage(int amount) {
    if (isDead) return;
    _left = math.max(0, _left - amount);
    hurt.emit(_left);
    if (isDead) died.emit(gameObject);
  }
}

/// The player.
class Dog extends GameObject {
  Dog()
    : super(
        name: 'dog',
        transform: Transform(position: Vec3(0, 0.35, 9.5)),
      );

  static const speed = 6.5;

  /// How far a bark reaches: the wolves it sends off, and the sheep it
  /// moves.
  static const barkReach = 5.0, scareReach = 6.0;

  static final _ring = Material(color: Color.white, emissive: Color.white, emissiveIntensity: 1.5);

  Vec3 facing = Vec3.forward;
  double _sinceBark = 1, _speed = 0;
  late final Animator _moves;
  late final Dust _dust;

  /// Told each bark, for the HUD's button to flash.
  final barked = Signal<void>();

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    // A pug (assets/CREDITS.md), a game's size, its feet on the grass.
    final model = add(await assets.instantiate('dog.glb'));
    model.transform = _farmAnimal(lift: 0.35, scale: 0.28);
    _moves = model.animator!..blend('move', _clips, thresholds: _dogPaces, at: 0, duration: 0);
    // Dust at its feet when it runs.
    _dust = add(Dust((parent! as Round).game.particles, lift: 0.35, from: 3.5, size: 0.4));
    on(input.onTap, (_) => bark());
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _sinceBark += time.delta;
    final move = input.stick() ?? input.axis2('move');
    final direction = Vec3(move.x, 0, -move.y);
    if (direction.length > 0) {
      facing = direction.normalized;
      rotation = Quat.lookRotation(facing);
      final was = position;
      position = walk(position, position + direction * (speed * time.delta));
      if (time.delta > 0) _speed = (position - was).flat.length / time.delta;
    } else {
      _speed = 0;
    }
    // Its legs go at its pace, from a stand to a run.
    if (time.delta > 0) _moves.blend('move', _clips, thresholds: _dogPaces, at: _speed);
    _dust.pace(_speed);
    if (input.action('bark').wasPressed) bark();
  }

  /// Barks, at most twice a second: the wolves within [barkReach] run off,
  /// the sheep within [scareReach] run from it.
  void bark() {
    if (_sinceBark < 0.5 || world.paused) return;
    _sinceBark = 0;
    barked.emit();
    final round = parent! as Round;
    for (final wolf in world.all<Wolf>()) {
      if ((wolf.position - position).flat.length < barkReach) wolf.scare();
    }
    for (final sheep in round.free) {
      if ((sheep.position - position).flat.length < scareReach) sheep.startle(from: position);
    }
    final ring = round.add(
      GameObject(
        transform: Transform(position: Vec3(position.x, 0.05, position.z), scale: Vec3(0.5, 0.02, 0.5)),
        layer: effects,
        // A flash on the grass, not a thing: no shadows from it or on it.
        components: [
          MeshRenderer(Mesh.cylinder(), _ring)
            ..castShadows = false
            ..receiveShadows = false,
        ],
      ),
    );
    ring
        .scaleTo(Vec3(2 * barkReach, 0.02, 2 * barkReach), seconds: 0.3, ease: Ease.outQuad)
        .then((_) => ring.destroy());
  }
}

/// A sheep: it keeps to the flock, walks away from the dog (faster the
/// nearer it comes) and runs from the wolves, and once in the pen it's
/// home for good.
class Sheep extends GameObject {
  Sheep(String name, {required Vec3 at, required this._random})
    : super(
        name: name,
        transform: Transform(position: Vec3(at.x, 0.4, at.z)),
      );

  /// Its top speed, running; and grazing, alone.
  static const speed = 3.4, grazing = 0.6;

  /// How near the dog and a wolf move it, and how near its neighbours are.
  static const dogReach = 5.0, wolfReach = 4.0, neighbours = 4.0;

  final math.Random _random;

  /// Once, when it walks into the pen.
  final cameHome = Signal<Sheep>();

  /// Once, when a wolf takes it.
  final taken = Signal<Sheep>();

  /// How it's moving, in metres a second.
  Vec3 velocity = Vec3.zero;

  bool _home = false, _taken = false;
  Vec3 _wander = Vec3.zero;
  double _nextTurn = 0;
  Vec3? _startledFrom;
  double _startled = 0, _panic = 0, _veer = 1;

  bool get isHome => _home;
  bool get isTaken => _taken;

  /// In the field still: neither home nor taken.
  bool get isFree => !_home && !_taken;

  late final Animator _moves;
  late final Dust _dust;
  List<MeshRenderer> _parts = const [];

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    // A sheep (assets/CREDITS.md), a game's size, its feet on the grass.
    final model = add(await assets.instantiate('sheep.glb'));
    model.transform = _farmAnimal(lift: 0.4, scale: 0.2);
    // Its materials, ready to flash when a wolf bites: made at the start,
    // not at the bite, so nothing waits for its shader then.
    final game = (parent! as Round).game;
    _parts = _renderers(model).toList();
    for (final part in _parts) {
      part.material = game.flashing(part.material!);
    }
    _moves = model.animator!..blend('move', _clips, thresholds: _sheepPaces, at: 0, duration: 0);
    // Dust at its feet when it runs, not as it grazes or walks.
    _dust = add(Dust(game.particles, lift: 0.4, from: 2));
    // Its name over it: a Flutter widget, placed where the camera sees it
    // (labels.dart).
    addComponent(WorldWidget((context) => NameTag(name!), offset: Vec3(0, 0.9, 0), alignment: Alignment.bottomCenter));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (!isFree) return;
    final round = parent! as Round;

    // Away from the dog and a bark, at a pace by how near: a walk at the
    // edge of its reach, a run close.
    var away = Vec3.zero;
    var pace = 0.0;
    void from(Vec3 at, double reach) {
      final d = (position - at).flat;
      final far = d.length;
      if (far >= reach || far < 1e-6) return;
      final near = 1 - far / reach;
      away += d * (near / far);
      pace = math.max(pace, math.min(1, 0.2 + near));
    }

    from(round.dog.position, dogReach);
    if (_startledFrom case final at? when _startled > 0) {
      _startled -= time.delta;
      from(at, Dog.scareReach + 1);
      pace = 1;
    }
    // From a hunting wolf it panics: the flock scatters, each sheep its own
    // way, at a run.
    _panic -= time.delta;
    for (final wolf in world.all<Wolf>()) {
      if (!wolf.isHunting) continue;
      final d = (position - wolf.position).flat;
      final far = d.length;
      if (far >= wolfReach || far < 1e-6) continue;
      if (_panic <= 0) _veer = _random.nextBool() ? 1 : -1;
      _panic = 1.5;
      away += (d + Vec3(-d.z, 0, d.x) * (0.6 * _veer)) * (2 / far);
      pace = 1;
    }
    var want = away.normalizedOrZero * (speed * pace);

    // The flock: towards the neighbours, the way they go, not too close.
    // Not in a panic.
    var towards = Vec3.zero, heading = Vec3.zero, apart = Vec3.zero;
    var n = 0;
    for (final other in round.free) {
      final d = (other.position - position).flat;
      final far = d.length;
      if (other == this || far > neighbours) continue;
      n++;
      towards += d;
      heading += other.velocity;
      if (far < 1 && far > 1e-6) apart -= d * ((1 - far) / far);
    }
    if (n > 0 && _panic <= 0) want += towards * (0.3 / n) + heading * (0.6 / n);
    want += apart * 2;

    // At the gate, drawn gently in.
    if (_panic <= 0 && position.x.abs() < penHalf - 0.5 && position.z > penFront && position.z < penFront + 2.5) {
      want += Vec3(0, 0, -1.5);
    }

    if (away.length < 1e-6) {
      _graze();
      want += _wander * grazing;
    }
    if (want.length > speed) want = want.normalized * speed;

    // It turns and speeds up or slows down over a moment, not at once: when
    // the dog backs off, it settles.
    velocity += (want - velocity) * math.min(1, 4 * time.delta);
    if (velocity.length > 0.05) {
      rotation = Quat.lookRotation(velocity.normalized);
      position = walk(position, position + velocity * time.delta);
    }
    _moves.blend('move', _clips, thresholds: _sheepPaces, at: velocity.length);
    _dust.pace(velocity.length);
    if (inPen(position)) _comeHome();
  }

  /// Now and then, a new way to wander, or a rest.
  void _graze() {
    _nextTurn -= time.delta;
    if (_nextTurn > 0) return;
    _nextTurn = 1.5 + _random.nextDouble() * 2;
    final angle = _random.nextDouble() * 2 * math.pi;
    _wander = _random.nextBool() ? Vec3(math.cos(angle), 0, math.sin(angle)) : Vec3.zero;
  }

  /// Runs from [from] for a moment, as from a bark.
  void startle({required Vec3 from}) {
    _startledFrom = from;
    _startled = 0.6;
  }

  void _comeHome() {
    _home = true;
    velocity = Vec3.zero;
    _dust.pace(0);
    removeComponent(get<WorldWidget>()!);
    cameHome.emit(this);
    // A puff where it came in.
    final round = parent! as Round;
    round.add(Puff(Vec3(position.x, 0.4, position.z), round.game.particles));
    // Further in, out of the gate, with a hop.
    final spot = Vec3((_random.nextDouble() * 2 - 1) * (penHalf - 0.8), 0.4, -size + 1.5 + _random.nextDouble() * 3);
    moveTo(spot, seconds: 1.2, ease: Ease.inOutSine);
    punchScale(Vec3.all(0.25), seconds: 0.4);
    _moves
      ..crossFade('Jump', duration: 0.1, loop: false)
      ..onEnd = (_) => _moves.crossFade('Idle');
  }

  /// Taken by a wolf: bitten, it flashes, then it's gone.
  void take() {
    if (!isFree) return;
    _taken = true;
    velocity = Vec3.zero;
    _dust.pace(0);
    removeComponent(get<WorldWidget>()!);
    taken.emit(this);
    _moves.crossFade('Death', duration: 0.05, loop: false);
    tween(
      1.0,
      0.0,
      seconds: biteFlash,
      onUpdate: (v) {
        for (final part in _parts) {
          part.set('flash', v);
        }
      },
    );
    scaleTo(Vec3.zero, seconds: 0.25, delay: biteFlash * 0.6, ease: Ease.inBack).then((_) => destroy());
  }

  /// How long a bitten sheep flashes.
  static const biteFlash = 0.4;
}

/// Every renderer in [o] and under it.
Iterable<MeshRenderer> _renderers(GameObject o) sync* {
  yield* o.getAll<MeshRenderer>();
  for (final child in o.children) {
    yield* _renderers(child);
  }
}

/// The farm animals' clips for a blend by speed: standing, walking slowly,
/// walking and running.
const _clips = ['Idle', 'WalkSlow', 'Walk', 'Run'];

/// The speeds (m/s) the clips' feet go at, for the dog and a sheep.
const _dogPaces = [0.0, 1.0, 2.5, Dog.speed];
const _sheepPaces = [0.0, Sheep.grazing, 1.5, Sheep.speed];

/// Where a farm animal's model sits in its object: [lift] down, so its
/// feet are on the grass, shrunk by [scale] from the pack's few metres,
/// and turned round to face forward (-z), as the pack's face +z.
Transform _farmAnimal({required double lift, required double scale}) =>
    Transform(position: Vec3(0, -lift, 0), rotation: Quat.axisAngle(Vec3.up, math.pi), scale: Vec3.all(scale));

/// Where a wolf is about to come in: a red mark on the ground, pulsing,
/// seen on the minimap too.
class WolfMark extends GameObject {
  WolfMark(Vec3 at)
    : super(
        name: 'wolf mark',
        transform: Transform(position: Vec3(at.x, 0.06, at.z)),
        components: [MeshRenderer(Mesh.cylinder(), _red)],
      );

  static final _red = Material(color: Color(0xFFE53935), emissive: Color(0xFFE53935), emissiveIntensity: 1);
  double _age = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    _age += time.delta;
    final across = 2.5 + 0.6 * math.sin(_age * 8);
    localScale = Vec3(across, 0.02, across);
  }
}

/// A wolf: it comes in at its mark, stalks the nearest sheep in the field
/// (slower than a sheep runs, with a short lunge when close) and takes it,
/// then runs off with it. It keeps away from the dog and goes round it;
/// near the dog its nerve wears down, and when it's gone, or at a bark, it
/// runs off with nothing.
class Wolf extends GameObject {
  Wolf(this.round, {required Vec3 at})
    : super(
        name: 'wolf',
        transform: Transform(position: Vec3(at.x, 0.45, at.z), scale: Vec3.zero),
        components: [Health(3)],
      );

  /// Its stalk (slower than a running sheep), its lunge, and its run off.
  static const speed = 2.6, lunge = 5.0, fleeing = 6.0;

  /// How near the dog it won't come; within a little more, its nerve wears
  /// down a point a second.
  static const fear = 3.5;

  /// How near a sheep it lunges, and for how long; then it rests.
  static const lungeFrom = 2.2, lungeFor = 0.6, rest = 1.5;

  /// How long a wolf chased off takes to burn away.
  static const burning = 0.9;

  final Round round;
  bool _ready = false, _leaving = false;
  double _lunging = 0, _resting = 0, _worn = 0;

  /// Its nerve.
  Health get health => get<Health>()!;

  /// Out for a sheep: grown in, and not running off.
  bool get isHunting => _ready && !_leaving;

  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        transform: Transform(scale: Vec3(0.55, 0.55, 1.2)),
        components: [MeshRenderer(Mesh.cube(), round.game.wolfCoat)],
      ),
    );
    for (final x in [-0.12, 0.12]) {
      add(
        GameObject(
          transform: Transform(position: Vec3(x, 0.12, -0.61), scale: Vec3.all(0.1)),
          components: [MeshRenderer(Mesh.cube(), round.game.wolfEyes)],
        ),
      );
    }
  }

  @override
  void onStart() {
    super.onStart();
    scaleTo(Vec3.one, seconds: 0.4, ease: Ease.outBack).then((_) => _ready = true);
    on(health.died, (_) => _burnAway());
    // Its nerve over it (labels.dart).
    addComponent(
      WorldWidget((context) => HealthBar(health), offset: Vec3(0, 0.9, 0), alignment: Alignment.bottomCenter),
    );
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (_leaving) {
      _runToTheEdge();
      return;
    }
    if (!_ready) return;
    Sheep? nearest;
    var best = double.infinity;
    for (final sheep in round.free) {
      final far = (sheep.position - position).flat.length;
      if (far < best) (nearest, best) = (sheep, far);
    }
    if (nearest == null) return runOff();
    if (best < 0.9) {
      nearest.take();
      return runOff();
    }
    final toSheep = (nearest.position - position).flat / best;
    var to = toSheep, pace = speed;

    final fromDog = (position - round.dog.position).flat;
    final dogFar = fromDog.length;
    _resting -= time.delta;
    if (dogFar < fear + 1.5 && dogFar > 1e-6) {
      // Afraid: out from the dog and round it, the sheep's way.
      _lunging = 0;
      final out = fromDog / dogFar;
      var side = Vec3(-out.z, 0, out.x);
      if (side.dot(toSheep) < 0) side = -side;
      final push = (fear + 1.5 - dogFar) / 1.5;
      final steer = toSheep + side * push + out * (2 * push);
      if (steer.length > 1e-6) to = steer.normalized;
      _worn += time.delta;
      if (_worn >= 1) {
        _worn -= 1;
        health.damage(1);
      }
    } else if (_lunging > 0) {
      _lunging -= time.delta;
      pace = lunge;
      if (_lunging <= 0) _resting = rest;
    } else if (_resting > 0) {
      pace = speed / 2;
    } else if (best < lungeFrom) {
      _lunging = lungeFor;
    }
    rotation = Quat.lookRotation(to);
    position = walk(position, position + to * (pace * time.delta), intoPen: false);
  }

  /// Scared off by a bark: its nerve gone at once.
  void scare() {
    if (_leaving) return;
    punchScale(Vec3.all(0.3), seconds: 0.25);
    health.damage(health.left);
  }

  /// Chased off: it runs, burning away as it goes, and is gone.
  void _burnAway() {
    runOff();
    round.add(Embers(this, round.game.particles, seconds: burning));
    final parts = _renderers(this).toList();
    tween(
      0.0,
      1.0,
      seconds: burning,
      ease: Ease.inQuad,
      onUpdate: (v) {
        for (final part in parts) {
          part.set('burnt', v);
        }
      },
    ).then((_) {
      if (!isDestroyed) destroy();
    });
  }

  /// Leaves the field, the nearest way out, and is gone.
  void runOff() {
    if (_leaving) return;
    _leaving = true;
    if (get<WorldWidget>() case final bar?) removeComponent(bar);
  }

  void _runToTheEdge() {
    final p = position;
    final out = [
      (size - p.x, Vec3(1, 0, 0)),
      (size + p.x, Vec3(-1, 0, 0)),
      (size - p.z, Vec3(0, 0, 1)),
    ].reduce((a, b) => a.$1 < b.$1 ? a : b).$2;
    rotation = Quat.lookRotation(out);
    position += out * (fleeing * time.delta);
    if (p.x.abs() > size + 1 || p.z > size + 1) destroy();
  }
}

/// Above the dog and a little behind, looking down at it.
class DogCamera extends GameObject {
  DogCamera(Dog dog)
    // The lens fits the view's height, so a tall view (a phone held
    // upright) sees less across: portraitFit goes back as far again as
    // it's narrower.
    : _follow = Follow(dog, offset: _offset, damping: 0.15, portraitFit: 0.75),
      _look = LookAt(dog),
      super(name: 'camera');

  static const _offset = Vec3(0, 13, 9);
  // Glow for the bulbs and the wolves' eyes at night, and darker
  // corners, where the quality draws them.
  final camera = Camera(fovY: 50.deg, bloom: const Bloom(), ambientOcclusion: const AmbientOcclusion());
  final Follow _follow;
  final LookAt _look;

  @override
  void onLoad() {
    super.onLoad();
    addComponent(camera);
    addComponent(_follow);
    addComponent(_look);
  }

  /// Follows [dog] from now on.
  void follow(Dog dog) {
    _follow.target = dog;
    _look.target = dog;
  }
}
