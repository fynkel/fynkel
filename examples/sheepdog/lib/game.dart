// The game: a dog, a flock and a pen. Move with W, A, S, D (or the arrow
// keys; the settings page changes the letters), or drag anywhere as a
// stick. Space, a tap or the Bark button barks: the sheep near the dog
// run from it, and the wolves near it are hurt and pushed back. Bring the
// sheep into the pen, through its gate, before the wolves take them.

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
const penHalf = 3.0, penFront = -5.0;

/// The layer of what's only for show, the bark's ring: the minimap leaves
/// it out.
const effects = 1;

/// The flock's names, one per sheep.
const names = ['Dolly', 'Shaun', 'Timmy', 'Flossie', 'Larry', 'Nuts', 'Hazel', 'Pidsley'];

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
  /// [seed] makes every round the same, for tests.
  SheepdogGame({this.seed, Controls? controls}) : controls = controls ?? Controls();

  final int? seed;
  final Controls controls;

  /// Sheep in the pen, and taken by wolves.
  final home = ValueNotifier(0), lost = ValueNotifier(0);
  final wave = ValueNotifier(0);

  /// How many sheep came home, once the round is over (every sheep home
  /// or taken); null while it's played.
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

  /// A new round: the flock back in the field, no wolves.
  Future<void> restart() async {
    round.destroy();
    round = add(Round(this, math.Random(seed)));
    camera.follow(round.dog);
    home.value = 0;
    lost.value = 0;
    wave.value = 0;
    result.value = null;
    await round.loaded;
  }
}

/// One go: the field, the pen, the dog, the flock, and the wolves.
class Round extends GameObject {
  Round(this.game, this._random);

  final SheepdogGame game;
  final math.Random _random;
  final dog = Dog();
  final flock = <Sheep>[];
  Routine<void>? _wolves;

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
    for (final name in names) {
      final at = Vec3(_random.nextDouble() * 16 - 8, 0, 1 + _random.nextDouble() * 8);
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
    _wolves = startRoutine(_waves);
  }

  /// The round is over once no sheep is left in the field.
  void _maybeOver() {
    if (game.home.value + game.lost.value < flock.length) return;
    _wolves?.stop();
    for (final wolf in world.all<Wolf>()) {
      wolf.runOff();
    }
    game.result.value = game.home.value;
  }

  /// Wave after wave: a pause, then one wolf more than the last time, one
  /// by one, then wait until they're all gone. It stops when the round is
  /// over, or destroyed.
  Future<void> _waves() async {
    await wait(4);
    for (var n = 1; ; n++) {
      game.wave.value = n;
      for (var i = 0; i < n; i++) {
        add(Wolf(this, at: _edge()));
        await wait(1.2);
      }
      await waitUntil(() => world.all<Wolf>().isEmpty);
      await wait(3);
    }
  }

  /// Somewhere along the field's front or sides, away from the pen.
  Vec3 _edge() {
    final along = _random.nextDouble() * 2 - 1;
    return switch (_random.nextInt(3)) {
      0 => Vec3(along * (size - 1), 0, size - 1),
      1 => Vec3(-size + 1, 0, 2 + along * 8),
      _ => Vec3(size - 1, 0, 2 + along * 8),
    };
  }

  /// The sheep still in the field.
  Iterable<Sheep> get free => flock.where((s) => s.isFree);
}

/// Hit points, for the wolves. After a hit it does nothing for [guard]
/// seconds.
class Health extends Behaviour {
  Health(this.max, {this.guard = 0}) : _left = max;

  final int max;
  final double guard;
  int _left;
  double _guarded = 0;

  /// What's left after each hit.
  final hurt = Signal<int>();

  /// Once, at the hit that leaves none.
  final died = Signal<GameObject>();

  int get left => _left;
  bool get isDead => _left == 0;

  void damage(int amount) {
    if (isDead || _guarded > 0) return;
    _left = math.max(0, _left - amount);
    _guarded = guard;
    hurt.emit(_left);
    if (isDead) died.emit(gameObject);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _guarded -= time.delta;
  }
}

/// The player.
class Dog extends GameObject {
  Dog()
    : super(
        name: 'dog',
        transform: Transform(position: Vec3(0, 0.35, 7)),
      );

  static const speed = 6.5;

  /// How far a bark reaches: the wolves it hurts, and the sheep it moves.
  static const barkReach = 4.5, scareReach = 6.0;

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

  /// Barks, at most twice a second: the wolves within [barkReach] are hurt
  /// and pushed back, the sheep within [scareReach] run from it.
  void bark() {
    if (_sinceBark < 0.5 || world.paused) return;
    _sinceBark = 0;
    barked.emit(null);
    final round = parent! as Round;
    for (final wolf in world.all<Wolf>()) {
      if (flat(wolf.position - position).length < barkReach) wolf.hit(from: position);
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

/// A sheep: it grazes, runs from the dog and the wolves, and once in the
/// pen it's home for good.
class Sheep extends GameObject {
  Sheep(String name, {required Vec3 at, required this._random})
    : super(
        name: name,
        transform: Transform(position: Vec3(at.x, 0.4, at.z)),
      );

  static const speed = 3.4, grazing = 0.6;
  static final _wool = Material(color: Color(0xFFF2EFE6), roughness: 1);
  static final _face = Material(color: Color(0xFF2B2B2E));

  final math.Random _random;

  /// Once, when it walks into the pen.
  final cameHome = Signal<Sheep>();

  /// Once, when a wolf takes it.
  final taken = Signal<Sheep>();

  bool _home = false, _taken = false;
  Vec3 _wander = Vec3.zero;
  double _nextTurn = 0;
  Vec3? _startledFrom;
  double _startled = 0;

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
    var away = Vec3.zero;
    void runFrom(Vec3 from, double reach, double weight) {
      final d = flat(position - from);
      final far = d.length;
      if (far < reach && far > 1e-6) away += d * (weight * (1 - far / reach) / far);
    }

    runFrom(round.dog.position, 3.5, 1);
    for (final wolf in world.all<Wolf>()) {
      if (wolf.isHunting) runFrom(wolf.position, 4, 1.5);
    }
    if (_startledFrom case final from? when _startled > 0) {
      _startled -= time.delta;
      runFrom(from, Dog.scareReach + 1, 2);
    }

    final Vec3 step;
    if (away.length > 0.02) {
      step = away.normalized * (speed * math.min(1, 0.4 + away.length));
    } else {
      _graze();
      step = _wander * grazing;
    }
    if (step.length > 0) {
      rotation = Quat.lookRotation(step.normalized);
      position = walk(position, position + step * time.delta);
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
    _startled = 0.8;
  }

  void _comeHome() {
    _home = true;
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
    removeComponent(get<WorldWidget>()!);
    taken.emit(this);
    scaleTo(Vec3.zero, seconds: 0.25, ease: Ease.inBack).then((_) => destroy());
  }
}

/// A wolf: it comes in from the edge, goes for the nearest sheep in the
/// field and takes it, then runs off with it. Three barks send it off
/// with nothing.
class Wolf extends GameObject {
  Wolf(this.round, {required Vec3 at})
    : super(
        name: 'wolf',
        transform: Transform(position: Vec3(at.x, 0.45, at.z), scale: Vec3.zero),
        components: [Health(3, guard: 0.25)],
      );

  /// A little faster than a sheep runs: it catches one unless the dog
  /// comes between.
  static const speed = 3.8, fleeing = 6.0;
  static final _coat = Material(color: Color(0xFF5A5F6B), roughness: 0.9);
  static final _eyes = Material(color: Color(0xFFFFD040), emissive: Color(0xFFFFB000), emissiveIntensity: 2);

  final Round round;
  bool _ready = false, _leaving = false;

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
    // Its health over it (labels.dart).
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
    final to = flat(nearest.position - position).normalized;
    rotation = Quat.lookRotation(to);
    position = walk(position, position + to * (speed * time.delta), intoPen: false);
  }

  /// Hurt by a bark from [from], and pushed away from it.
  void hit({required Vec3 from}) {
    if (_leaving) return;
    health.damage(1);
    punchScale(Vec3.one * 0.3, seconds: 0.25);
    final away = flat(position - from);
    if (away.length > 1e-6) position = walk(position, position + away.normalized * 1.5, intoPen: false);
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
