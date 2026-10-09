// Step 2: the lifecycle. Every call an object and its behaviours get, as
// they're turned on and off, destroyed, and rebuilt; an error that doesn't
// stop the game; and why onLateUpdate exists.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class LifecycleScene extends LabScene {
  @override
  String get title => 'Lifecycle';
  @override
  String get step => 'step 2';
  @override
  String get about =>
      'A tower (the grey base) holds A (orange) and B (blue, which spins). Turn them on and off, destroy A, '
      'rebuild: the log shows each call, parents first. "Add a buggy cube" throws every second; the error is '
      'logged and the game goes on. On the right, the red ball follows the moving cube in onUpdate (a frame '
      'behind), the green one in onLateUpdate (exactly on it).';

  GameObject? _tower;
  GameObject? _a;
  GameObject? _b;
  var _buggies = 0;

  @override
  void build() {
    _build();
    // The followers first, so they update before the mover moves.
    final mover = Mover();
    add(Follower(mover, late: false));
    add(Follower(mover, late: true));
    add(mover);
  }

  void _build() {
    log.add('— building the tower —');
    _tower = add(
      Logged(
        'tower',
        shape('base', Mesh.cube(), const Color(0xFF8A8F99), size: Vec3(2, 0.3, 2)),
        children: [
          _a = Logged('A', shape('A body', Mesh.cube(), const Color(0xFFE0613A)), at: Vec3(-0.6, 0.8, 0)),
          _b = Logged(
            'B',
            shape('B body', Mesh.cube(), const Color(0xFF4CA6E0)),
            at: Vec3(0.6, 0.8, 0),
            components: [LoggedSpin()],
          ),
        ],
      ),
    );
  }

  @override
  List<Control> get controls => [
    Toggle('tower active', () => _tower?.isAlive == true && _tower!.active, (on) => _tower?.active = on),
    Toggle('A active', () => _a?.isAlive == true && _a!.active, (on) {
      if (_a?.isAlive == true) _a!.active = on;
    }),
    Toggle('B\'s spin enabled', () => _b?.get<LoggedSpin>()?.enabled ?? false, (on) {
      _b?.get<LoggedSpin>()?.enabled = on;
    }),
    Button('destroy A', () {
      if (_a?.isAlive == true) _a!.destroy();
    }),
    Button('rebuild', () {
      _tower?.destroy();
      _build();
    }),
    Button('add a buggy cube', () {
      _buggies++;
      add(
        shape(
          'buggy $_buggies',
          Mesh.cube(),
          const Color(0xFFD04060),
          at: Vec3(-4 + _buggies * 0.7, 0.3, 2),
          size: Vec3(0.5, 0.5, 0.5),
          components: [Buggy()],
        ),
      );
    }),
    Button('clear log', log.clear),
  ];
}

/// An object that logs each lifecycle call it and its behaviours get, with
/// [body] as its look.
class Logged extends GameObject {
  Logged(
    String name,
    GameObject body, {
    Vec3 at = Vec3.zero,
    super.components,
    Iterable<GameObject> children = const [],
  }) : super(
         name: name,
         transform: Transform(position: at),
         children: [body, ...children],
       );

  void say(String what) => ancestor<LabScene>()?.log.add('$name: $what');

  @override
  void onLoad() {
    super.onLoad();
    // The scene isn't its ancestor until it's added, which it is by now.
    say('onLoad');
  }

  @override
  void onEnable() {
    super.onEnable();
    say('onEnable');
  }

  @override
  void onStart() {
    super.onStart();
    say('onStart');
  }

  @override
  void onDisable() {
    super.onDisable();
    say('onDisable');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    say('onDestroy');
  }
}

/// Spins its object, and logs its own calls.
class LoggedSpin extends Behaviour {
  void say(String what) => gameObject.ancestor<LabScene>()?.log.add('  ${gameObject.name}\'s spin: $what');

  @override
  void onEnable() {
    super.onEnable();
    say('onEnable');
  }

  @override
  void onStart() {
    super.onStart();
    say('onStart');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    gameObject.rotate(Vec3.up, 2 * time.delta);
  }

  @override
  void onDisable() {
    super.onDisable();
    say('onDisable');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    say('onDestroy');
  }
}

/// Throws once a second from its update. The error goes to world.errors,
/// which the scene logs; everything else keeps going.
class Buggy extends Behaviour {
  double _since = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    gameObject.rotate(Vec3(1, 1, 0).normalized, 3 * time.delta);
    _since += time.delta;
    if (_since >= 1) {
      _since = 0;
      throw StateError('a bug in ${gameObject.name}');
    }
  }
}

/// Goes back and forth fast, on the right.
class Mover extends GameObject {
  Mover() : super(name: 'mover');

  @override
  void onLoad() {
    super.onLoad();
    addComponent(MeshRenderer(Mesh.cube(), own(Material(color: const Color(0xFFE0E0E0)))));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    transform = Transform(position: Vec3(4, 0.4, 2.5 * math.sin(time.sinceStart * 4)), scale: Vec3(0.6, 0.6, 0.6));
  }
}

/// Sits on [target], placed in onUpdate or, if [late], in onLateUpdate.
/// Both are added before the mover, so in onUpdate the mover hasn't moved
/// yet this frame: the red one is always a frame behind. The green one
/// is placed after every update, where the mover ended up.
class Follower extends GameObject {
  Follower(this.target, {required this.late}) : super(name: late ? 'late follower' : 'update follower');

  final GameObject target;
  final bool late;

  @override
  void onLoad() {
    super.onLoad();
    final color = late ? const Color(0xFF60D070) : const Color(0xFFE04848);
    addComponent(MeshRenderer(Mesh.sphere(), own(Material(color: color))));
  }

  void _follow() => transform = Transform(
    position: target.transform.position + Vec3(0, late ? 1.1 : 0.75, 0),
    scale: Vec3(0.35, 0.35, 0.35),
  );

  @override
  void onUpdate() {
    super.onUpdate();
    if (!late) _follow();
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    if (late) _follow();
  }
}
