// The game: a dog, a flock and a pen. Move with W, A, S, D (or the arrow
// keys; the settings page changes the letters), or drag anywhere as a
// stick. The flock keeps together and walks away from the dog, faster the
// nearer it is: walk behind it to drive it. Space, a tap or the Bark
// button barks: the sheep near the dog run, and a wolf near it runs off.
// Bring the flock into the pen, through its gate, before the time is up.

import 'dart:math' as math;

import 'package:flutter/foundation.dart' show ChangeNotifier, ValueNotifier;
import 'package:flutter/material.dart' show Brightness, ThemeData;
import 'package:flutter/widgets.dart' show Alignment;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart'
    show FlutterColorToFynkel, ListenableSignal, WorldTheme, WorldWidget;

import 'labels.dart' show HealthBar, NameTag;

/// Half the field's width: it goes from -size to size on x and z.
const size = 12.0;

/// The pen is the field's back middle: x between -penHalf and penHalf,
/// z behind penFront, with its gate the whole front.
const penHalf = 4.5, penFront = -5.0;

/// The layer of what's only for show, the bark's ring: the minimap leaves
/// it out.
const effects = 1;

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
    final at = Vec3(p.x.clamp(-size + 0.5, size - 0.5), from.y, p.z.clamp(-size + 0.5, size - 0.5));
    if (_mayStep(from, at, intoPen)) return at;
  }
  return from;
}

bool _mayStep(Vec3 from, Vec3 to, bool intoPen) {
  final wasIn = inPen(from), isIn = inPen(to);
  if (wasIn == isIn) return true;
  if (isIn && !intoPen) return false;
  final outside = wasIn ? to : from;
  return outside.z >= penFront && outside.x.abs() < penHalf - 0.3;
}

/// [v] on the floor: its y left out.
Vec3 flat(Vec3 v) => Vec3(v.x, 0, v.z);

/// The keys the player chose on the settings page. The game binds them as
/// it starts, and again each time they change; the arrow keys always move
/// too.
class Controls extends ChangeNotifier {
  static const defaults = {
    'up': KeyCode.w,
    'down': KeyCode.s,
    'left': KeyCode.a,
    'right': KeyCode.d,
    'bark': KeyCode.space,
  };

  final _keys = Map.of(defaults);

  /// The key for [name]: one of [defaults]' names.
  KeyCode operator [](String name) => _keys[name]!;

  /// Gives [name] the [key]. What had that key takes [name]'s old one, so
  /// no two share a key.
  void set(String name, KeyCode key) {
    final old = _keys[name]!;
    if (old == key) return;
    for (final other in _keys.keys) {
      if (_keys[other] == key) _keys[other] = old;
    }
    _keys[name] = key;
    notifyListeners();
  }

  /// Back to [defaults].
  void reset() {
    _keys.addAll(defaults);
    notifyListeners();
  }

  /// Binds 'move' and 'bark' in [input] to these keys.
  void bindTo(Input input) {
    input.bindAxis2(
      'move',
      up: [this['up'], KeyCode.arrowUp],
      down: [this['down'], KeyCode.arrowDown],
      left: [this['left'], KeyCode.arrowLeft],
      right: [this['right'], KeyCode.arrowRight],
    );
    input.bindAction('bark', [this['bark']]);
  }
}

/// The root: the lights, the cameras, what the HUD shows, and the round
/// being played. Its colours are the app's theme's.
class SheepdogGame extends GameObject {
  /// [seed] makes every round the same, for tests; the first round is at
  /// [level].
  SheepdogGame({this.seed, Controls? controls, int level = 1})
    : controls = controls ?? Controls(),
      level = ValueNotifier(level),
      clock = ValueNotifier(levelOf(level).seconds.ceil());

  final int? seed;
  final Controls controls;

  /// Sheep in the pen, and taken by wolves.
  final home = ValueNotifier(0), lost = ValueNotifier(0);

  /// The level being played, and the seconds left in its round.
  final ValueNotifier<int> level, clock;

  /// Whether a wolf's mark is out: one comes in a moment.
  final coming = ValueNotifier(false);

  /// How many sheep came home, once the round is over (every sheep home
  /// or taken, or the time up); null while it's played.
  final result = ValueNotifier<int?>(null);

  /// The floor and the fences, coloured by [wear].
  final grass = Material(name: 'grass', roughness: 1), fence = Material(name: 'fence', roughness: 0.8);

  /// The minimap's camera, high above the field and looking straight down.
  final map = Camera(orthographic: true, orthographicSize: size + 0.5, cullingMask: LayerMask.except([effects]));

  late final Light _sun, _ambient;
  late final DogCamera camera;
  late Round round;

  @override
  void onLoad() {
    super.onLoad();
    _sun = add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(Vec3(-0.4, -1, -0.6))),
        components: [Light.sun()],
      ),
    ).get<Light>()!;
    _ambient = add(GameObject(components: [Light.ambient()])).get<Light>()!;
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
    final sky = (dark ? scheme.surfaceContainerLowest : scheme.tertiaryContainer).toFynkel();
    camera.camera.clearColor = sky;
    map.clearColor = sky;
    grass.color = (dark ? scheme.onPrimaryFixedVariant : scheme.primary).toFynkel();
    fence.color = (dark ? scheme.outline : scheme.secondaryContainer).toFynkel();
    // The moon is pale blue.
    final light = dark ? Color.rgb(0.65, 0.75, 1) : Color.white;
    _sun
      ..intensity = dark ? 1.1 : 2.6
      ..color = light;
    _ambient
      ..intensity = dark ? 0.35 : 0.6
      ..color = light;
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
        transform: Transform(scale: Vec3(2 * size, 1, 2 * size)),
        components: [MeshRenderer(Mesh.plane(), game.grass)],
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

  static final _coat = Material(color: Color(0xFF8A5A3B));
  static final _white = Material(color: Color.white);
  static final _ring = Material(color: Color.white, emissive: Color.white, emissiveIntensity: 1.5);

  Vec3 facing = Vec3.forward;
  double _sinceBark = 1;

  /// Told each bark, for the HUD's button to flash.
  final barked = Signal<void>();

  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        transform: Transform(scale: Vec3(0.5, 0.5, 0.9)),
        components: [MeshRenderer(Mesh.cube(), _coat)],
      ),
    );
    // A white muzzle, to show which way it faces.
    add(
      GameObject(
        transform: Transform(position: Vec3(0, 0.15, -0.5), scale: Vec3(0.3, 0.25, 0.3)),
        components: [MeshRenderer(Mesh.cube(), _white)],
      ),
    );
    on(input.onTap, (_) => bark());
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _sinceBark += time.delta;
    final keys = input.axis2('move');
    final direction = _stick() ?? Vec3(keys.x, 0, -keys.y);
    if (direction.length > 0) {
      facing = direction.normalized;
      rotation = Quat.lookRotation(facing);
      position = walk(position, position + direction * (speed * time.delta));
    }
    if (input.action('bark').wasPressed) bark();
  }

  /// A finger dragging alone, as a stick: from where it went down to where
  /// it is, full speed at 60 pixels.
  Vec3? _stick() {
    final down = input.fingers.where((f) => f.isPressed).toList();
    if (down.length != 1 || !down.single.isDragging) return null;
    final d = down.single.totalDelta;
    final v = Vec3(d.x, 0, d.y) * (1 / 60);
    return v.length > 1 ? v.normalized : v;
  }

  /// Barks, at most twice a second: the wolves within [barkReach] run off,
  /// the sheep within [scareReach] run from it.
  void bark() {
    if (_sinceBark < 0.5 || world.paused) return;
    _sinceBark = 0;
    barked.emit(null);
    final round = parent! as Round;
    for (final wolf in world.all<Wolf>()) {
      if (flat(wolf.position - position).length < barkReach) wolf.scare();
    }
    for (final sheep in round.free) {
      if (flat(sheep.position - position).length < scareReach) sheep.startle(from: position);
    }
    final ring = round.add(
      GameObject(
        transform: Transform(position: Vec3(position.x, 0.05, position.z), scale: Vec3(0.5, 0.02, 0.5)),
        layer: effects,
        components: [MeshRenderer(Mesh.cylinder(), _ring)],
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

  static final _wool = Material(color: Color(0xFFF2EFE6), roughness: 1);
  static final _face = Material(color: Color(0xFF2B2B2E));

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

  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        transform: Transform(scale: Vec3(0.8, 0.7, 1.1)),
        components: [MeshRenderer(Mesh.sphere(), _wool)],
      ),
    );
    add(
      GameObject(
        transform: Transform(position: Vec3(0, 0.15, -0.6), scale: Vec3(0.32, 0.32, 0.32)),
        components: [MeshRenderer(Mesh.cube(), _face)],
      ),
    );
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
      final d = flat(position - at);
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
      final d = flat(position - wolf.position);
      final far = d.length;
      if (far >= wolfReach || far < 1e-6) continue;
      if (_panic <= 0) _veer = _random.nextBool() ? 1 : -1;
      _panic = 1.5;
      away += (d + Vec3(-d.z, 0, d.x) * (0.6 * _veer)) * (2 / far);
      pace = 1;
    }
    var want = away.length > 1e-6 ? away.normalized * (speed * pace) : Vec3.zero;

    // The flock: towards the neighbours, the way they go, not too close.
    // Not in a panic.
    var towards = Vec3.zero, heading = Vec3.zero, apart = Vec3.zero;
    var n = 0;
    for (final other in round.free) {
      final d = flat(other.position - position);
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
    removeComponent(get<WorldWidget>()!);
    cameHome.emit(this);
    // Further in, out of the gate, with a hop.
    final spot = Vec3((_random.nextDouble() * 2 - 1) * (penHalf - 0.8), 0.4, -size + 1.5 + _random.nextDouble() * 3);
    moveTo(spot, seconds: 1.2, ease: Ease.inOutSine);
    punchScale(Vec3.one * 0.25, seconds: 0.4);
  }

  /// Taken by a wolf: gone.
  void take() {
    if (!isFree) return;
    _taken = true;
    velocity = Vec3.zero;
    removeComponent(get<WorldWidget>()!);
    taken.emit(this);
    scaleTo(Vec3.zero, seconds: 0.25, ease: Ease.inBack).then((_) => destroy());
  }
}

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

  static final _coat = Material(color: Color(0xFF5A5F6B), roughness: 0.9);
  static final _eyes = Material(color: Color(0xFFFFD040), emissive: Color(0xFFFFB000), emissiveIntensity: 2);

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
        components: [MeshRenderer(Mesh.cube(), _coat)],
      ),
    );
    for (final x in [-0.12, 0.12]) {
      add(
        GameObject(
          transform: Transform(position: Vec3(x, 0.12, -0.61), scale: Vec3.one * 0.1),
          components: [MeshRenderer(Mesh.cube(), _eyes)],
        ),
      );
    }
  }

  @override
  void onStart() {
    super.onStart();
    scaleTo(Vec3.one, seconds: 0.4, ease: Ease.outBack).then((_) => _ready = true);
    on(health.died, (_) => runOff());
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
      final far = flat(sheep.position - position).length;
      if (far < best) (nearest, best) = (sheep, far);
    }
    if (nearest == null) return runOff();
    if (best < 0.9) {
      nearest.take();
      return runOff();
    }
    final toSheep = flat(nearest.position - position) / best;
    var to = toSheep, pace = speed;

    final fromDog = flat(position - round.dog.position);
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
    punchScale(Vec3.one * 0.3, seconds: 0.25);
    health.damage(health.left);
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
    : _follow = Follow(dog, offset: _offset, damping: 0.15),
      _look = LookAt(dog),
      super(name: 'camera');

  static const _offset = Vec3(0, 13, 9);
  final camera = Camera(fovY: 50.deg);
  final Follow _follow;
  final LookAt _look;

  @override
  void onLoad() {
    super.onLoad();
    addComponent(camera);
    addComponent(_follow);
    addComponent(_look);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    // The lens fits the view's height, so a tall view (a phone held
    // upright) sees less across: go back as far again as it's narrower.
    final (w, h) = (input.width, input.height);
    final fit = w > 0 && h > w ? math.min(2.0, 0.75 * h / w) : 1.0;
    _follow.offset = _offset * fit;
  }

  /// Follows [dog] from now on.
  void follow(Dog dog) {
    _follow.target = dog;
    _look.target = dog;
  }
}
