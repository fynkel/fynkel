// Step 6: motion in a game. Tweens and their helpers, a sequence that
// loops, awaiting a tween in a routine, a race against a timeout, and a
// follower moved by hand with SmoothDamp.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class MotionScene extends LabScene {
  @override
  String get title => 'Motion: tweens and sequences';
  @override
  String get step => 'step 6';
  @override
  String get about =>
      'The orange guard patrols a square: one sequence of moveTo and rotateBy that loops for ever. The blue ball '
      'follows it by hand with SmoothDamp3. Click the crate: punchScale and a colour flash. The coin spins and bobs '
      '(rotateBy and moveBy, looping). "Open the door" runs a routine that awaits each move. "Race" moves the '
      'runner for 3 s against a 1 s wait: the wait wins, and race stops the move.';

  @override
  Vec3 get eye => Vec3(0, 7, 9);

  late final GameObject guard, follower, coin, door, runner;
  late final Crate crate;
  late final GameObject camera = find('camera')!;
  Motion? _patrol, _race;
  Routine<void>? _doorRoutine;
  var hits = 0;

  @override
  void build() {
    guard = add(shape('guard', Mesh.cube(), const Color(0xFFE0613A), at: Vec3(-2, 0.4, -2), size: Vec3(0.6, 0.8, 0.6)));
    guard.add(shape('nose', Mesh.cube(), const Color(0xFFFFD040), at: Vec3(0, 0.1, -0.55), size: Vec3(0.3, 0.2, 0.3)));
    follower = add(
      shape('follower', Mesh.sphere(), const Color(0xFF4CA6E0), at: Vec3(-3, 0.3, -3), size: Vec3(0.5, 0.5, 0.5)),
    );
    follower.addComponent(Trail(guard));
    crate = add(Crate());
    coin = add(shape('coin', Mesh.sphere(), const Color(0xFFFFC020), at: Vec3(3.6, 1, 0), size: Vec3(0.7, 0.7, 0.12)));
    door = add(shape('door', Mesh.cube(), const Color(0xFF8A6A50), at: Vec3(-3.6, 1, 0), size: Vec3(0.2, 2, 1.4)));
    runner = add(
      shape('runner', Mesh.sphere(), const Color(0xFF7BC86C), at: Vec3(-3.5, 0.3, 3), size: Vec3(0.5, 0.5, 0.5)),
    );
  }

  @override
  void onStart() {
    super.onStart();
    _startPatrol();
    // Spins for ever and bobs: two tweens on one object, each its own.
    coin.rotateBy(Vec3.up, 2 * math.pi, seconds: 1.6, ease: Ease.linear, loops: -1);
    coin.moveBy(Vec3(0, 0.3, 0), seconds: 0.8, ease: Ease.inOutSine, loops: -1, yoyo: true);
  }

  /// Round the square, turning a quarter at each corner, for ever.
  void _startPatrol() {
    _patrol?.stop();
    guard.transform = Transform(position: Vec3(-2, 0.4, -2), rotation: Quat.lookRotation(Vec3.right));
    const corners = [Vec3(2, 0.4, -2), Vec3(2, 0.4, 2), Vec3(-2, 0.4, 2), Vec3(-2, 0.4, -2)];
    _patrol = guard.sequence([
      for (final corner in corners) ...[
        guard.moveTo(corner, speed: 2, ease: Ease.inOutSine),
        guard.rotateBy(Vec3.up, -math.pi / 2, seconds: 0.4, ease: Ease.inOutQuad),
      ],
    ], loops: -1);
    log.add('patrol: a sequence of ${corners.length * 2} motions, loops: -1');
  }

  void _openDoor() {
    if (_doorRoutine?.isRunning ?? false) return log.add('the door is busy');
    _doorRoutine = start(() async {
      log.add('door: opening…');
      await door.moveBy(Vec3(0, 1.9, 0), seconds: 1, ease: Ease.inOutCubic);
      log.add('door: open (the routine went on after the await)');
      await wait(1);
      await door.moveBy(Vec3(0, -1.9, 0), seconds: 0.7, ease: Ease.outBounce);
      log.add('door: closed');
    });
  }

  void _runRace() {
    if (_race?.isRunning ?? false) return;
    start(() async {
      runner.position = Vec3(-3.5, 0.3, 3);
      final move = _race = runner.moveTo(Vec3(3.5, 0.3, 3), seconds: 3, ease: Ease.linear);
      await race([move, wait(1)]);
      log.add(
        'race: ${move.isStopped ? 'the 1 s wait won, so the move was stopped' : 'the move won'} '
        'at x = ${runner.position.x.toStringAsFixed(2)}',
      );
      await wait(0.5);
      await runner.moveTo(Vec3(-3.5, 0.3, 3), seconds: 0.6, ease: Ease.outBack);
    });
  }

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value = [
      'patrol: ${_patrol == null
              ? '–'
              : _patrol!.isRunning
              ? 'running'
              : _patrol!.isStopped
              ? 'stopped'
              : 'completed'}'
          '   guard at ${_vec(guard.position)}, facing ${_vec(guard.forward)}',
      'follower: ${follower.get<Trail>()!.speed.toStringAsFixed(2)} a second',
      'crate hits: ${crate.hits}   time scale: ${time.scale.toStringAsFixed(2)}',
    ].join('\n');
  }

  static String _vec(Vec3 v) => '(${v.x.toStringAsFixed(1)}, ${v.y.toStringAsFixed(1)}, ${v.z.toStringAsFixed(1)})';

  @override
  List<Control> get controls => [
    Button('stop the patrol', () => _patrol?.stop()),
    Button('patrol again', _startPatrol),
    Button('open the door', _openDoor),
    Button('race: 3 s move against a 1 s wait', _runRace),
    Button('shake the camera', () => camera.shake(0.15, seconds: 0.5)),
    Slider(
      'follower lag (smoothTime)',
      () => follower.get<Trail>()!.damp.smoothTime,
      (v) {
        follower.get<Trail>()!.damp.smoothTime = v;
      },
      min: 0.05,
      max: 1.5,
    ),
    Slider('time scale', () => time.scale, (v) => time.scale = v, max: 2),
  ];
}

/// Follows [target] by hand, a little behind it, with SmoothDamp3: no
/// tween, just a step each frame.
class Trail extends Behaviour {
  Trail(this.target);

  final GameObject target;
  final damp = SmoothDamp3(smoothTime: 0.5);
  double speed = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    // Behind the guard: the opposite way to where it looks.
    final goal = target.position - target.forward * 1.2;
    position = damp.step(position, Vec3(goal.x, 0.3, goal.z), time.delta);
    speed = damp.velocity.length;
  }
}

/// Click it: it swells and flashes, a hit.
class Crate extends GameObject {
  Crate()
    : super(
        name: 'crate',
        transform: Transform(position: Vec3(0, 0.5, 0)),
      );

  static const _color = Color(0xFFC08A50);
  var hits = 0;

  @override
  void onLoad() {
    super.onLoad();
    addComponent(MeshRenderer(Mesh.cube(), Material(color: _color)));
  }

  @override
  bool onClick(Finger finger) {
    hits++;
    scale = Vec3.one;
    punchScale(Vec3.one * 0.35, seconds: 0.45);
    sequence([colorTo(Color.white, seconds: 0.05), colorTo(_color, seconds: 0.3)]);
    ancestor<LabScene>()?.log.add('crate: hit ${hits}x (punchScale, and a sequence of two colorTo)');
    return true;
  }
}
