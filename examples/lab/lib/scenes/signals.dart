// Step 4: signals. Enemies that tell whoever listens when they die, a
// scoreboard found with world.only, ghosts that show why `on` exists, a
// door whose routine awaits a plate's next press, and what a destroyed
// object says when it's used.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class SignalsScene extends LabScene {
  @override
  String get title => 'Signals';
  @override
  String get step => 'step 4';
  @override
  String get about =>
      'Tap an enemy (left) to hurt it; at 0 it emits died and goes. The scoreboard (middle, found with '
      'world.only) listens with on() and grows. "Add ghosts": the purple one listens to every death with listen(), '
      'the green one with on(). Destroy them and kill again: the purple one still hears, from beyond; the green one '
      'never does. Right: the door\'s routine awaits the plate\'s next press, racing 5 s; tap the plate in time.';

  /// Every enemy's death, for the ghosts.
  final anyDied = Signal<String>();

  final _random = math.Random(4);
  var _spawned = 0;
  final _ghosts = <GameObject>[];
  GameObject? _lastDead;
  late final Plate _plate;

  @override
  Vec3 get eye => Vec3(0, 5, 10);

  @override
  void build() {
    add(Scoreboard());
    _plate = add(Plate());
    add(Door(_plate));
    _spawn(3);
  }

  void _spawn(int n) {
    for (var i = 0; i < n; i++) {
      // On a grid, a little off, so they never overlap.
      final slot = _spawned++ % 9;
      final at = Vec3(
        -4.6 + (slot % 3) * 1.3 + 0.3 * _random.nextDouble(),
        0.4,
        -1.4 + (slot ~/ 3) * 1.3 + 0.3 * _random.nextDouble(),
      );
      final enemy = add(Enemy(at));
      world.only<Scoreboard>().watch(enemy);
    }
  }

  void died(Enemy enemy) {
    _lastDead = enemy;
    anyDied.emit('${enemy.name}');
  }

  void _addGhosts() {
    _ghosts
      ..add(add(Ghost(viaOn: false, at: Vec3(-0.6 + _ghosts.length * 0.6, 0.3, 3))))
      ..add(add(Ghost(viaOn: true, at: Vec3(-0.3 + _ghosts.length * 0.6, 0.3, 3))));
  }

  void _destroyGhosts() {
    for (final ghost in _ghosts) {
      ghost.destroy();
    }
    log.add('ghosts destroyed: kill an enemy and see who still hears');
    _ghosts.clear();
  }

  void _readDead() {
    final dead = _lastDead;
    if (dead == null) {
      log.add('no enemy has died yet');
      return;
    }
    try {
      log.add('the dead enemy is at ${dead.transform.position}');
    } on StateError catch (e) {
      log.add('reading the last dead enemy: ${e.message}');
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value = [
      'score ${world.only<Scoreboard>().score}   enemies ${world.all<Enemy>().length}',
      'ghosts alive ${_ghosts.length}   anyDied has listeners: ${anyDied.hasListeners}',
      'door: ${world.only<Door>().state}',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Button('spawn 3 enemies', () => _spawn(3)),
    Button('hurt all', () {
      for (final health in world.all<Health>()) {
        health.hurt(1);
      }
    }),
    Button('press the plate', _plate.press),
    Button('add ghosts', _addGhosts),
    Button('destroy ghosts', _destroyGhosts),
    Button('read a dead enemy', _readDead),
    Button('clear log', log.clear),
  ];
}

/// Hit points, and a signal for when they run out.
class Health extends Behaviour {
  Health(this.max) : hp = max;
  final int max;
  int hp;

  /// Emitted when [hp] reaches 0, just before its object is destroyed.
  final died = Signal<void>();

  /// Emitted with what's left, each hit.
  final hit = Signal<int>();

  void hurt(int damage) {
    if (hp <= 0) return;
    hp -= damage;
    hit.emit(hp);
    if (hp <= 0) {
      died.emit(null);
      gameObject.destroy();
    }
  }
}

/// A cube with [Health]: flashes white when hit, and says when it dies.
class Enemy extends GameObject {
  Enemy(Vec3 at)
    : super(
        transform: Transform(position: at, scale: Vec3(0.8, 0.8, 0.8)),
        components: [Health(3)],
      );

  static var _count = 0;
  late final Material _paint;
  static const _color = Color(0xFFD04848);
  static const _shades = [Color(0xFF502828), Color(0xFF803434), Color(0xFFA83E3E), _color];

  @override
  void onLoad() {
    super.onLoad();
    name = 'enemy ${++_count}';
    _paint = own(Material(color: _color));
    addComponent(MeshRenderer(Mesh.cube(), _paint));
    final health = get<Health>()!;
    // Its own signals: they go with it.
    on(health.hit, (hp) {
      start(() async {
        _paint.color = Color.white;
        await wait(0.1);
        // Darker as it weakens.
        _paint.color = _shades[hp.clamp(0, _shades.length - 1)];
      });
    });
    on(health.died, (_) => ancestor<SignalsScene>()?.died(this));
  }

  @override
  bool onTap(Tap tap) {
    get<Health>()!.hurt(1);
    return true;
  }
}

/// Counts deaths, listening to each enemy with `on`. There's one, found
/// with `world.only<Scoreboard>()`, no static instance.
class Scoreboard extends GameObject {
  Scoreboard()
    : super(
        name: 'scoreboard',
        transform: Transform(position: Vec3(0, 0, -2)),
      );

  var score = 0;
  late final GameObject _bar;

  @override
  void onLoad() {
    super.onLoad();
    _bar = add(
      GameObject(
        name: 'bar',
        components: [MeshRenderer(Mesh.cube(), own(Material(color: const Color(0xFFFFD040))))],
      ),
    );
    _show();
  }

  void watch(Enemy enemy) {
    on(enemy.get<Health>()!.died, (_) {
      score++;
      _show();
      ancestor<LabScene>()?.log.add('scoreboard: ${enemy.name} died, score $score');
    });
  }

  void _show() {
    final height = 0.2 + 0.25 * math.min(score, 16);
    _bar.transform = Transform(position: Vec3(0, height / 2, 0), scale: Vec3(0.8, height, 0.8));
  }
}

/// Hears every death: with `on`, until it's destroyed; with `listen`, for
/// as long as the signal lives, destroyed or not.
class Ghost extends GameObject {
  Ghost({required this.viaOn, required Vec3 at})
    : super(
        name: viaOn ? 'on() ghost' : 'listen() ghost',
        transform: Transform(position: at, scale: Vec3(0.4, 0.4, 0.4)),
      );

  final bool viaOn;

  @override
  void onLoad() {
    super.onLoad();
    final color = viaOn ? const Color(0xFF60D070) : const Color(0xFFA060E0);
    addComponent(MeshRenderer(Mesh.sphere(), own(Material(color: color))));
    final scene = ancestor<SignalsScene>()!;
    final log = scene.log;
    final label = '$name #$id';
    void heard(String who) {
      log.add(
        isDestroyed ? '$label heard $who die, though it was destroyed: a leak' : '$label heard $who die',
        error: isDestroyed,
      );
    }

    if (viaOn) {
      on(scene.anyDied, heard);
    } else {
      // Wrong on purpose: nothing cancels this.
      scene.anyDied.listen(heard);
    }
  }
}

/// A plate on the floor: tap it to press it.
class Plate extends GameObject {
  Plate()
    : super(
        name: 'plate',
        transform: Transform(position: Vec3(3, 0.05, 2), scale: Vec3(1.2, 0.1, 1.2)),
      );

  final pressed = Signal<int>();
  var _presses = 0;

  @override
  void onLoad() {
    super.onLoad();
    addComponent(MeshRenderer(Mesh.cube(), own(Material(color: const Color(0xFF4CA6E0)))));
  }

  void press() => pressed.emit(++_presses);

  @override
  bool onTap(Tap tap) {
    press();
    return true;
  }
}

/// Opens when the plate is pressed within 5 s; otherwise locks for 2 s.
/// One routine, waiting on the plate's `next`.
class Door extends GameObject {
  Door(this.plate)
    : super(
        name: 'door',
        transform: Transform(position: Vec3(3.5, 0, -1)),
      );

  final Plate plate;
  var state = 'closed';
  late final GameObject _panel;
  late final Material _paint;

  @override
  void onLoad() {
    super.onLoad();
    _paint = own(Material(color: const Color(0xFF8A8F99)));
    _panel = add(GameObject(name: 'panel', components: [MeshRenderer(Mesh.cube(), _paint)]));
    _place(0);
  }

  void _place(double open) =>
      _panel.transform = Transform(position: Vec3(0, 1 + 1.9 * open, 0), scale: Vec3(1.6, 2, 0.3));

  @override
  void onStart() {
    super.onStart();
    final log = ancestor<LabScene>()!.log;
    start(() async {
      while (true) {
        state = 'waiting for the plate (5 s)';
        _paint.color = const Color(0xFF8A8F99);
        final press = await race<Object?>([plate.pressed.next, wait(5)]);
        if (press == null) {
          // Shown, not logged: it comes every 7 s.
          state = 'locked: too slow';
          _paint.color = const Color(0xFFD04848);
          await wait(2);
          continue;
        }
        state = 'open (press $press)';
        log.add('door: press $press heard, opening');
        _paint.color = const Color(0xFF60D070);
        for (var t = 0.0; t < 0.5; t += time.delta) {
          _place(t / 0.5);
          await nextFrame;
        }
        _place(1);
        await wait(2);
        _place(0);
      }
    });
  }
}
