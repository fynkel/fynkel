// The game: a top-down arena. Move with W, A, S, D (or the arrow keys;
// the settings page changes the letters), or drag anywhere as a stick.
// Swipe, tap or press space to dash: a dash goes through enemies and hits
// them. The enemies come in waves, and each wave has one more. Some leave
// a heart behind.

import 'dart:math' as math;

import 'package:flutter/foundation.dart' show ValueNotifier;
import 'package:flutter/widgets.dart' show Alignment;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart' show Controls, ListenableSignal, WorldWidget;

import 'hud.dart' show HealthBar;

/// The layer of what's only for show, the sparks: taps go through it.
const effects = 1;

/// Half the floor's width: the arena goes from -size to size on x and z.
const size = 10.0;

/// For measuring: this many enemies come at once in the first wave, and
/// they don't hurt the player (`--dart-define=ENEMIES=100`). 0 to play.
const crowd = int.fromEnvironment('ENEMIES');

/// For measuring: false leaves the health bars off (`--dart-define=BARS=false`).
const bars = bool.fromEnvironment('BARS', defaultValue: true);

/// [point] moved inside the walls, on the floor.
Vec3 inside(Vec3 point) => Vec3(point.x.clamp(-size + 0.5, size - 0.5), 0, point.z.clamp(-size + 0.5, size - 0.5));

/// The keys the player can change on the settings page; the arrow keys
/// always move too.
Controls arenaControls() =>
    Controls({'up': KeyCode.w, 'down': KeyCode.s, 'left': KeyCode.a, 'right': KeyCode.d, 'dash': KeyCode.space});

/// The root: the camera, what the HUD shows, and the round being played.
class ArenaGame extends GameObject {
  /// [seed] makes every round the same, for tests.
  ArenaGame({this.seed, Controls? controls}) : controls = controls ?? arenaControls();

  final int? seed;
  final Controls controls;
  final score = ValueNotifier(0);
  final wave = ValueNotifier(0);
  final health = ValueNotifier(Player.hearts);
  final over = ValueNotifier(false);

  late final ArenaCamera camera;
  late Round round;

  @override
  void onLoad() {
    super.onLoad();
    // Its own look, from before a new world had a sky and a sun shadows:
    // the camera's clear colour behind, an ambient light, no shadows.
    world.sky = null;
    add(GameObject(components: [Light.sun(direction: Vec3(-0.3, -1, -0.5), intensity: 2.5, shadows: false)]));
    add(GameObject(components: [Light.ambient(intensity: 0.5)]));
    round = add(Round(math.Random(seed)));
    camera = add(ArenaCamera(round.player));
    input.layers = LayerMask.except([effects]);
    controls.bindTo(input);
    on(controls.changed, (_) => controls.bindTo(input));
  }

  /// A new round, from wave 1.
  Future<void> restart() async {
    round.destroy();
    round = add(Round(math.Random(seed)));
    camera.follow(round.player);
    score.value = 0;
    health.value = Player.hearts;
    over.value = false;
    await round.loaded;
  }
}

/// One go: the floor, the walls, the player, and the waves.
class Round extends GameObject {
  Round(this._random);

  final math.Random _random;
  final player = Player();

  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        transform: Transform(scale: Vec3(2 * size, 1, 2 * size)),
        components: [MeshRenderer(Mesh.plane(), Material(color: Color.rgb(0.18, 0.2, 0.26), roughness: 1))],
      ),
    );
    final stone = Material(color: Color.rgb(0.35, 0.37, 0.45));
    for (final (at, long) in [
      (Vec3(0, 0.5, -size), true),
      (Vec3(0, 0.5, size), true),
      (Vec3(-size, 0.5, 0), false),
      (Vec3(size, 0.5, 0), false),
    ]) {
      final scale = long ? Vec3(2 * size + 1, 1, 1) : Vec3(1, 1, 2 * size + 1);
      add(
        GameObject(
          transform: Transform(position: at, scale: scale),
          components: [MeshRenderer(Mesh.cube(), stone)],
        ),
      );
    }
    add(player);
  }

  @override
  void onStart() {
    super.onStart();
    final game = ancestor<ArenaGame>()!;
    on(player.health.hurt, (left) {
      game.health.value = left;
      world.cameras.shake(0.3);
    });
    on(player.health.healed, (left) => game.health.value = left);
    final waves = startRoutine(() => _waves(game));
    on(player.health.died, (_) {
      waves.stop();
      player.scaleTo(Vec3.zero, seconds: 0.4, ease: Ease.inBack);
      game.over.value = true;
    });
  }

  /// Wave after wave: a pause, then the enemies one by one, then wait
  /// until they're all gone. It stops when the round is destroyed.
  Future<void> _waves(ArenaGame game) async {
    for (var n = 1; ; n++) {
      game.wave.value = n;
      await wait(1.5);
      for (var i = 0; i < (crowd > 0 && n == 1 ? crowd : n + 2); i++) {
        final enemy = add(Enemy(player, at: _edge(), speed: 2 + 0.25 * n, hits: n >= 3 && i.isEven ? 2 : 1));
        on(enemy.health.died, (_) {
          game.score.value++;
          if (_random.nextDouble() < 0.25) add(Heart(player, enemy.position));
        });
        if (crowd == 0) await wait(0.5);
      }
      await waitUntil(() => world.all<Enemy>().isEmpty);
    }
  }

  /// Somewhere along the walls.
  Vec3 _edge() {
    final along = (_random.nextDouble() * 2 - 1) * (size - 1);
    return switch (_random.nextInt(4)) {
      0 => Vec3(along, 0, -size + 1),
      1 => Vec3(along, 0, size - 1),
      2 => Vec3(-size + 1, 0, along),
      _ => Vec3(size - 1, 0, along),
    };
  }
}

/// Hit points, for the player and the enemies alike. After a hit it does
/// nothing for [guard] seconds.
class Health extends Behaviour {
  Health(this.max, {this.guard = 0}) : _left = max;

  final int max;
  final double guard;
  int _left;
  double _guarded = 0;

  /// What's left after each hit.
  final hurt = Signal<int>();

  /// What's left after each heal.
  final healed = Signal<int>();

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

  void heal(int amount) {
    if (isDead) return;
    _left = math.min(max, _left + amount);
    healed.emit(_left);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _guarded -= time.delta;
  }
}

/// Flashes white and swells a little when its object's [Health] is hurt.
class Flash extends Behaviour {
  Flash(this.color);

  /// The object's own colour, to go back to.
  final Color color;

  @override
  void onStart() {
    super.onStart();
    on(get<Health>()!.hurt, (left) {
      if (left == 0) return;
      sequence([colorTo(Color.white, seconds: 0.05), colorTo(color, seconds: 0.25)]);
      punchScale(Vec3.all(0.3), seconds: 0.3);
    });
  }
}

class Player extends GameObject {
  Player()
    : super(
        name: 'player',
        components: [
          MeshRenderer(Mesh.sphere(), Material(color: blue)),
          Health(hearts, guard: 1),
          Flash(blue),
        ],
      );

  static const hearts = 5;
  static const speed = 6.0;
  static const blue = Color(0xFF4CA6E0);

  Health get health => get<Health>()!;
  Vec3 facing = Vec3.forward;
  bool dashing = false;

  @override
  void onLoad() {
    super.onLoad();
    position = Vec3(0, 0.5, 0);
    // A nose, to show which way it faces.
    add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, -0.5), scale: Vec3.all(0.25)),
        components: [MeshRenderer(Mesh.cube(), Material(color: Color.white))],
      ),
    );
    // A swipe's direction is on the view: +y is down it, which is +z on
    // the floor seen from above.
    on(input.onSwipe, (swipe) => dash(Vec3(swipe.direction.x, 0, swipe.direction.y)));
    on(input.onTap, (finger) {
      if (finger.over case final hit?) dash(hit.point - position);
    });
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (health.isDead) return;
    if (dashing) {
      _hitWhatsNear();
      return;
    }
    final move = input.stick() ?? input.axis2('move');
    final direction = Vec3(move.x, 0, -move.y);
    if (direction.length > 0) {
      facing = direction.normalized;
      rotation = Quat.lookRotation(facing);
      position = inside(position + direction * (speed * time.delta)) + Vec3(0, 0.5, 0);
    }
    if (input.action('dash').wasPressed) dash(facing);
  }

  /// Goes 5 metres along [direction] in a blink, hitting every enemy it
  /// passes.
  void dash(Vec3 direction) {
    final flat = direction.flat;
    if (dashing || health.isDead || world.paused || flat.length < 1e-6) return;
    facing = flat.normalized;
    rotation = Quat.lookRotation(facing);
    dashing = true;
    startRoutine(() async {
      await moveTo(inside(position + facing * 5) + Vec3(0, 0.5, 0), seconds: 0.18);
      dashing = false;
    });
  }

  void _hitWhatsNear() {
    for (final enemy in world.all<Enemy>()) {
      if ((enemy.position - position).length < 1.2) enemy.health.damage(1);
    }
  }
}

class Enemy extends GameObject {
  Enemy(this.player, {required Vec3 at, required this.speed, int hits = 1})
    : _size = hits > 1 ? 1.3 : 0.9,
      super(
        transform: Transform(position: at + Vec3(0, 0.5, 0), scale: Vec3.zero),
        components: [
          // Shared: Flash changes the renderer's own colour, not these.
          MeshRenderer(Mesh.cube(), hits > 1 ? _purple : _red),
          Health(hits, guard: 0.3),
          Flash(hits > 1 ? purple : red),
        ],
      );

  static const red = Color(0xFFE0613A);
  static final _red = Material(color: red);
  static final _purple = Material(color: purple);
  static const purple = Color(0xFF9A5AE0);

  final Player player;
  final double speed;
  final double _size;
  bool _ready = false;

  Health get health => get<Health>()!;

  @override
  void onStart() {
    super.onStart();
    scaleTo(Vec3.all(_size), seconds: 0.4, ease: Ease.outBack).then((_) => _ready = true);
    on(health.died, (_) => _die());
    // Its health over it: a Flutter widget, placed where the camera sees
    // it (hud.dart).
    if (bars) {
      addComponent(
        WorldWidget((context) => HealthBar(health), offset: Vec3(0, 1.1, 0), alignment: Alignment.bottomCenter),
      );
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (!_ready || health.isDead || player.health.isDead) return;
    final to = (player.position - position).flat;
    if (to.length < 0.9) {
      if (!player.dashing && crowd == 0) player.health.damage(1);
      return;
    }
    position += to.normalized * (speed * time.delta);
    rotate(Vec3.up, 2 * time.delta);
  }

  void _die() {
    if (get<WorldWidget>() case final bar?) removeComponent(bar);
    final at = position;
    parent!.add(Sparks(at));
    scaleTo(Vec3.zero, seconds: 0.2, ease: Ease.inBack).then((_) => destroy());
  }
}

/// A burst of small glowing cubes, on the [effects] layer: taps go
/// through them.
class Sparks extends GameObject {
  Sparks(Vec3 at)
    : super(
        transform: Transform(position: at),
        layer: effects,
      );

  static final _glow = Material(color: Color.rgb(1, 0.8, 0.4), emissive: Color.rgb(1, 0.6, 0.2), emissiveIntensity: 3);
  static final _random = math.Random();

  @override
  void onStart() {
    super.onStart();
    for (var i = 0; i < 8; i++) {
      final angle = 2 * math.pi * (i + _random.nextDouble()) / 8;
      final spark = add(
        GameObject(
          transform: Transform(scale: Vec3.all(0.2)),
          layer: effects,
          components: [MeshRenderer(Mesh.cube(), _glow)],
        ),
      );
      spark.moveBy(Vec3(math.cos(angle), 0.5 + _random.nextDouble(), math.sin(angle)) * 1.5, seconds: 0.5);
      spark.scaleTo(Vec3.zero, seconds: 0.5, ease: Ease.inQuad);
    }
    destroy(after: 0.5);
  }
}

/// A heart an enemy left: it spins and bobs until the player takes it.
class Heart extends GameObject {
  Heart(this.player, Vec3 at)
    : super(
        transform: Transform(position: Vec3(at.x, 0.6, at.z), scale: Vec3.zero),
        components: [MeshRenderer(Mesh.sphere(), _pink)],
      );

  static final _pink = Material(color: Color(0xFFFF6FA0), emissive: Color(0xFFFF3070), emissiveIntensity: 0.6);

  final Player player;
  bool _taken = false;

  @override
  void onStart() {
    super.onStart();
    scaleTo(Vec3.all(0.5), seconds: 0.3, ease: Ease.outBack);
    moveBy(Vec3(0, 0.25, 0), seconds: 0.6, ease: Ease.inOutSine, loops: -1, yoyo: true);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final to = player.position - position;
    if (_taken || to.flat.length > 1) return;
    _taken = true;
    player.health.heal(1);
    player.punchScale(Vec3.all(0.4), seconds: 0.3);
    scaleTo(Vec3.zero, seconds: 0.2).then((_) => destroy());
  }
}

/// Above the player and a little behind, looking down at it. A pinch or
/// the mouse wheel zooms.
class ArenaCamera extends GameObject {
  ArenaCamera(Player player)
    : _follow = Follow(player, offset: _offset, damping: 0.15, portraitFit: 0.75),
      _look = LookAt(player),
      super(name: 'camera');

  static const _offset = Vec3(0, 14, 7);
  final Follow _follow;
  final LookAt _look;
  double _zoom = 1;

  @override
  void onLoad() {
    super.onLoad();
    addComponent(Camera(fovY: 50.deg, clearColor: Color.rgb(0.08, 0.09, 0.12), bloom: const Bloom()));
    addComponent(_follow);
    addComponent(_look);
    on(input.onPinch, (pinch) => _zoomBy(1 / pinch.scale));
    on(input.onScroll, (scroll) => _zoomBy(1 - scroll.y * 0.002));
  }

  // The lens fits the view's height, so a tall view (a phone held upright)
  // sees less across: the Follow's portraitFit goes back as far again as
  // it's narrower.
  void _zoomBy(double factor) {
    _zoom = (_zoom * factor).clamp(0.5, 1.6);
    _follow.offset = _offset * _zoom;
  }

  /// Follows [player] from now on.
  void follow(Player player) {
    _follow.target = player;
    _look.target = player;
  }
}
