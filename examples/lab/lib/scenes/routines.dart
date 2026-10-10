// Step 3: routines. Three of them, each async code that waits in game
// time: a spawner whose drops grow, wait and shrink; a traffic light with
// a finally; and a race between a runner and a timeout.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class RoutinesScene extends LabScene {
  @override
  String get title => 'Routines';
  @override
  String get step => 'step 3';
  @override
  String get about =>
      'Left: a spawner adds a drop every 0.8 s; each drop is its own routine (grow, wait 1.5 s, shrink, go). '
      'Turn the spawner off: its routines pause, and go on where they were when it\'s back. '
      'Middle: a traffic light, a loop of waits; stopping it runs its finally, which turns it off. '
      'Right: each round races the runner against a 3 s timeout; the loser is stopped. '
      'Slide the time scale: every wait follows it.';

  late final Spawner _spawner;
  late final TrafficLight _light;
  late final Race _race;

  @override
  Vec3 get eye => Vec3(0, 5, 10);

  @override
  void build() {
    _spawner = add(Spawner());
    _light = add(TrafficLight());
    _race = add(Race());
  }

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value = [
      'spawner: ${_spawner.active ? 'on' : 'off (paused)'}, ${_spawner.drops} drops, ${_spawner.made} made',
      'traffic light: ${_light.running ? 'running' : 'stopped'}',
      'race: round ${_race.round}, ${_race.wins} won, ${_race.losses} too slow',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Toggle('spawner active', () => _spawner.active, (on) => _spawner.active = on),
    Button('stop light', _light.stop),
    Button('start light', _light.go),
    Toggle('race running', () => _race.running, (on) => on ? _race.go() : _race.stop()),
    Slider('time scale', () => time.scale, (v) => time.scale = v, max: 3),
  ];
}

/// Runs [apply] with 0 to 1 over [seconds] of game time, a frame at a
/// time: the shape every animation in a routine has. (Step 6's tweens will
/// do this for you.)
Future<void> over(Time time, double seconds, void Function(double t) apply) async {
  for (var t = 0.0; t < seconds; t += time.delta) {
    apply(t / seconds);
    await nextFrame;
  }
  apply(1);
}

/// Adds a [Drop] every 0.8 s, forever: while it's off, it waits.
class Spawner extends GameObject {
  Spawner()
    : super(
        name: 'spawner',
        transform: Transform(position: Vec3(-3.5, 0, 0)),
      );

  final _random = math.Random(1);
  var made = 0;
  int get drops => children.whereType<Drop>().length;

  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        name: 'pad',
        transform: Transform(position: Vec3(0, 0.05, 0), scale: Vec3(2.4, 0.1, 2.4)),
        components: [MeshRenderer(Mesh.cube(), Material(color: const Color(0xFF8A8F99)))],
      ),
    );
  }

  @override
  void onStart() {
    super.onStart();
    start(() async {
      while (true) {
        await wait(0.8);
        made++;
        // Its children, so they hide (and pause) with it.
        add(Drop(Vec3(2 * _random.nextDouble() - 1, 0.5 + 2 * _random.nextDouble(), 2 * _random.nextDouble() - 1)));
      }
    });
  }
}

/// Grows, waits, shrinks and goes, in one routine.
class Drop extends GameObject {
  Drop(Vec3 at)
    : super(
        name: 'drop',
        transform: Transform(position: at, scale: Vec3.zero),
      );

  static final _colors = [const Color(0xFF4CA6E0), const Color(0xFF7BC86C), const Color(0xFFE0613A)];

  @override
  void onLoad() {
    super.onLoad();
    addComponent(MeshRenderer(Mesh.sphere(), Material(color: _colors[id % 3])));
  }

  @override
  void onStart() {
    super.onStart();
    start(() async {
      final at = transform.position;
      Vec3 size(double s) => Vec3(0.5 * s, 0.5 * s, 0.5 * s);
      await over(time, 0.4, (t) => transform = Transform(position: at, scale: size(t)));
      await wait(1.5);
      await over(time, 0.4, (t) => transform = Transform(position: at, scale: size(1 - t)));
      destroy();
    });
  }
}

/// Green, yellow, red, round and round, until it's stopped.
class TrafficLight extends GameObject {
  TrafficLight()
    : super(
        name: 'traffic light',
        transform: Transform(position: Vec3(0, 0, -0.5)),
      );

  final _lamps = <Material>[];
  static const _on = [Color(0xFF40E060), Color(0xFFFFD040), Color(0xFFFF4040)];
  static const _off = Color(0xFF30333A);
  Routine<void>? _routine;

  bool get running => _routine?.isRunning ?? false;

  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        name: 'pole',
        transform: Transform(position: Vec3(0, 1.2, 0), scale: Vec3(0.7, 2.4, 0.5)),
        components: [MeshRenderer(Mesh.cube(), Material(color: const Color(0xFF22252B)))],
      ),
    );
    for (var i = 0; i < 3; i++) {
      final lamp = Material(color: _off);
      _lamps.add(lamp);
      add(
        GameObject(
          name: 'lamp $i',
          transform: Transform(position: Vec3(0, 0.6 + i * 0.6, 0.3), scale: Vec3(0.45, 0.45, 0.45)),
          components: [MeshRenderer(Mesh.sphere(), lamp)],
        ),
      );
    }
  }

  @override
  void onStart() {
    super.onStart();
    go();
  }

  void _show(int? lit) {
    for (final (i, lamp) in _lamps.indexed) {
      lamp
        ..color = i == lit ? _on[i] : _off
        ..emissive = i == lit ? _on[i] : Color.black;
    }
  }

  void go() {
    if (running) return;
    final log = ancestor<LabScene>()!.log..add('traffic light: started');
    _routine = start(() async {
      try {
        while (true) {
          _show(0);
          await wait(2);
          _show(1);
          await wait(0.7);
          _show(2);
          await wait(2);
        }
      } finally {
        _show(null);
        log.add('traffic light: stopped; its finally turned the lamps off');
      }
    });
  }

  void stop() => _routine?.stop();
}

/// Each round, a runner with a random speed races a 3 s timeout to the
/// flag. `race` gives the first to end and stops the other.
class Race extends GameObject {
  Race()
    : super(
        name: 'race',
        transform: Transform(position: Vec3(3.5, 0, 0)),
      );

  final _random = math.Random(2);
  late final GameObject _runner;
  late final Material _paint;
  Routine<void>? _routine;
  var round = 0, wins = 0, losses = 0;
  static const _from = 2.5, _to = -2.5;

  bool get running => _routine?.isRunning ?? false;

  @override
  void onLoad() {
    super.onLoad();
    _paint = Material(color: Color.white);
    _runner = add(
      GameObject(
        name: 'runner',
        transform: Transform(position: Vec3(0, 0.3, _from), scale: Vec3(0.6, 0.6, 0.6)),
        components: [MeshRenderer(Mesh.cube(), _paint)],
      ),
    );
    add(
      GameObject(
        name: 'flag',
        transform: Transform(position: Vec3(0, 0.75, _to - 0.4), scale: Vec3(1.4, 1.5, 0.1)),
        components: [MeshRenderer(Mesh.cube(), Material(color: const Color(0xFFE0E0E0)))],
      ),
    );
  }

  @override
  void onStart() {
    super.onStart();
    go();
  }

  /// Runs to the flag at [speed], a frame at a time, and says how long it
  /// took.
  Future<String> _run(double speed) async {
    final started = time.sinceStart;
    var z = _from;
    while (z > _to) {
      z = math.max(_to, z - speed * time.delta);
      _runner.transform = Transform(position: Vec3(0, 0.3, z), scale: Vec3(0.6, 0.6, 0.6));
      await nextFrame;
    }
    return 'made it in ${secs(time.sinceStart - started)}';
  }

  void go() {
    if (running) return;
    final log = ancestor<LabScene>()!.log;
    _routine = start(() async {
      while (true) {
        round++;
        final speed = 1.2 + 2 * _random.nextDouble();
        _paint.color = Color.white;
        _runner.transform = Transform(position: Vec3(0, 0.3, _from), scale: Vec3(0.6, 0.6, 0.6));
        final result = await race([
          start(() => _run(speed)),
          start(() async {
            await wait(3);
            return 'too slow';
          }),
        ]);
        final won = result != 'too slow';
        won ? wins++ : losses++;
        _paint.color = won ? const Color(0xFF40E060) : const Color(0xFFFF4040);
        log.add('race $round at ${speed.toStringAsFixed(2)} m/s: $result');
        await wait(1.2);
      }
    });
  }

  void stop() {
    _routine?.stop();
    ancestor<LabScene>()!.log.add('race: stopped (and the runner and timer of this round with it)');
  }
}
