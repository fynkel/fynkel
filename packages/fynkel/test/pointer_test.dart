// The objects' pointer handlers (docs/stage-4.md §3.6, step 5): what's
// under a finger gets onPointerDown, onClick and the rest, bubbling up to
// the parents; a drag's taker keeps it.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

final log = <String>[];

/// Logs each pointer call, and takes those in [takes].
class Pointed extends GameObject {
  Pointed(String name, {this.takes = const {}, super.transform, super.components}) : super(name: name);

  final Set<String> takes;

  bool say(String what) {
    log.add('$what $name');
    return takes.contains(what);
  }

  @override
  bool onPointerDown(Finger finger) => say('down');
  @override
  void onPointerUp(Finger finger) => say('up');
  @override
  void onPointerEnter(Finger finger) => say('enter');
  @override
  void onPointerExit(Finger finger) => say('exit');
  @override
  bool onClick(Finger finger) => say('click');
  @override
  bool onLongPress(Finger finger) => say('long');
  @override
  bool onDragStart(Finger finger) => say('drag start');
  @override
  void onDrag(Finger finger) => say('drag ${finger.delta.x}');
  @override
  void onDragEnd(Finger finger) => say('drag end');
}

/// The same, as a behaviour.
class PointedBehaviour extends Behaviour {
  PointedBehaviour(this.name, {this.takes = const {}});

  final String name;
  final Set<String> takes;

  bool say(String what) {
    log.add('$what $name');
    return takes.contains(what);
  }

  @override
  bool onPointerDown(Finger finger) => say('down');
  @override
  void onPointerUp(Finger finger) => say('up');
  @override
  void onPointerEnter(Finger finger) => say('enter');
  @override
  void onPointerExit(Finger finger) => say('exit');
  @override
  bool onClick(Finger finger) => say('click');
  @override
  bool onDragStart(Finger finger) => say('drag start');
  @override
  void onDrag(Finger finger) => say('drag ${finger.delta.x}');
  @override
  void onDragEnd(Finger finger) => say('drag end');
}

MeshRenderer cube() => MeshRenderer(Mesh.cube(), Material());

// A camera at the origin looking down -Z, 60° high, in a 100 × 100 view:
// a cube 5 ahead is at the middle; one 1.5 to the left or right of it at
// x = 24 or 76.
const middle = (50.0, 50.0);
const left = (24.0, 50.0);
const right = (76.0, 50.0);
const nothing = (5.0, 50.0);

void main() {
  late World world;
  late Input input;
  var id = 0;

  setUp(() {
    world = World();
    input = world.input..simulate.resize(100, 100);
    world.add(GameObject(name: 'camera', components: [Camera()]));
    log.clear();
  });

  tearDown(() => world.dispose());

  void frame([double dt = 1 / 60]) => world.update(dt);

  int down((double, double) at, {FingerKind kind = FingerKind.touch}) {
    final finger = kind == FingerKind.mouse ? -1 : ++id;
    input.simulate.fingerDown(finger, at.$1, at.$2, kind: kind);
    return finger;
  }

  void move(int finger, (double, double) at) => input.simulate.fingerMove(finger, at.$1, at.$2);
  void up(int finger, (double, double) at) => input.simulate.fingerUp(finger, at.$1, at.$2);

  /// A robot 5 ahead with an arm, below a level.
  ({Pointed level, Pointed robot, Pointed arm}) robot({
    Set<String> level = const {},
    Set<String> robot = const {},
    Set<String> arm = const {},
  }) {
    final l = world.add(Pointed('level', takes: level));
    final r = l.add(
      Pointed(
        'robot',
        takes: robot,
        transform: Transform(position: Vec3(0, 0, -5)),
      ),
    );
    final a = r.add(Pointed('arm', takes: arm, components: [cube()]));
    return (level: l, robot: r, arm: a);
  }

  test('a down bubbles from the part hit until one takes it; only the taker gets its up, wherever it is', () {
    robot(robot: {'down'});
    final f = down(middle);
    frame();
    expect(log, ['enter level', 'enter robot', 'enter arm', 'down arm', 'down robot']);
    log.clear();
    move(f, nothing);
    frame();
    up(f, nothing);
    frame();
    expect(log.where((l) => !l.startsWith('drag')), [
      'exit arm',
      'exit robot',
      'exit level',
      'up robot',
    ], reason: 'no up for the arm or the level');
  });

  test("an object's behaviours come right after it, and can take what it passes on", () {
    final r = robot();
    r.robot.addComponent(PointedBehaviour('brain', takes: {'down', 'click'}));
    final f = down(middle);
    frame();
    up(f, middle);
    frame();
    expect(log.where((l) => !l.startsWith('enter') && !l.startsWith('exit')), [
      'down arm',
      'down robot',
      'down brain',
      'click arm',
      'click robot',
      'click brain',
      'up brain',
    ]);
  });

  test('a disabled behaviour gets nothing', () {
    final r = robot();
    r.arm.addComponent(PointedBehaviour('off')).enabled = false;
    final f = down(middle);
    frame();
    up(f, middle);
    frame();
    expect(log.where((l) => l.endsWith('off')), isEmpty);
  });

  test('a click bubbles; a double tap clicks twice, the second with tapCount 2', () {
    final counts = <int>[];
    final r = robot(arm: {'click'});
    r.arm.addComponent(_Counts(counts));
    for (var i = 0; i < 2; i++) {
      final f = down(middle);
      frame(0.05);
      up(f, middle);
      frame(0.05);
    }
    expect(log.where((l) => l.startsWith('click')), ['click arm', 'click arm']);
    expect(counts, isEmpty, reason: 'the arm took them before its behaviour');

    r.arm.destroy();
    // Long enough after for the next to be a first tap again.
    frame(0.3);
    frame(0.3);
    final b = robot();
    b.robot.addComponent(_Counts(counts));
    log.clear();
    for (var i = 0; i < 2; i++) {
      final f = down(middle);
      frame(0.05);
      up(f, middle);
      frame(0.05);
    }
    expect(counts, [1, 2]);
  });

  test('a tap over nothing clicks nothing, but the input still tells it', () {
    robot(level: {'click'});
    final taps = <Finger>[];
    input.onTap.listen(taps.add);
    final f = down(nothing);
    frame();
    up(f, nothing);
    frame();
    expect(log, isEmpty);
    expect(taps.single.over, isNull);
  });

  test('a long press bubbles as a click does', () {
    robot(robot: {'long'});
    down(middle);
    frame();
    // A frame counts for at most a third of a second.
    frame(0.3);
    frame(0.3);
    expect(log.where((l) => l.startsWith('long')), ['long arm', 'long robot']);
  });

  test("a drag goes to what it started over; the one that takes it keeps it, wherever the finger goes", () {
    final r = robot(robot: {'drag start'});
    final other = world.add(
      Pointed(
        'other',
        transform: Transform(position: Vec3(1.5, 0, -5)),
        components: [cube()],
      ),
    );
    final f = down(middle);
    frame();
    move(f, (middle.$1 + 20, 50));
    frame();
    move(f, right);
    frame();
    move(f, nothing);
    frame();
    up(f, nothing);
    frame();
    expect(log.where((l) => l.startsWith('drag')), [
      'drag start arm',
      'drag start robot',
      'drag 20.0 robot',
      'drag 6.0 robot',
      'drag -71.0 robot',
      'drag end robot',
    ]);
    expect(log, contains('enter other'), reason: 'it passed over the other, which got no drag');
    expect(r.arm.isAlive && other.isAlive, isTrue);
  });

  test('a drag no one takes goes to no object', () {
    robot();
    final f = down(middle);
    frame();
    move(f, nothing);
    frame();
    up(f, nothing);
    frame();
    expect(log.where((l) => l.startsWith('drag')), ['drag start arm', 'drag start robot', 'drag start level']);
  });

  test("a drag's taker destroyed meanwhile gets no more of it", () {
    final r = robot(arm: {'drag start'});
    final f = down(middle);
    frame();
    move(f, nothing);
    frame();
    r.arm.destroy();
    move(f, middle);
    frame();
    up(f, middle);
    frame();
    expect(log.where((l) => l.startsWith('drag')), ['drag start arm', 'drag -45.0 arm']);
  });

  test("a behaviour that takes a drag keeps it", () {
    final r = robot();
    r.robot.addComponent(PointedBehaviour('handle', takes: {'drag start'}));
    final f = down(middle);
    frame();
    move(f, nothing);
    frame();
    up(f, nothing);
    frame();
    expect(log.where((l) => l.startsWith('drag')), [
      'drag start arm',
      'drag start robot',
      'drag start handle',
      'drag -45.0 handle',
      'drag end handle',
    ]);
  });

  test('the mouse enters and exits as it hovers: from the top down in, from the part up out', () {
    final r = robot();
    r.robot.add(
      Pointed(
        'hand',
        transform: Transform(position: Vec3(-1.5, 0, 0)),
        components: [cube()],
      ),
    );
    input.simulate.hover(-1, nothing.$1, nothing.$2);
    frame();
    expect(log, isEmpty);
    input.simulate.hover(-1, middle.$1, middle.$2);
    frame();
    expect(log, ['enter level', 'enter robot', 'enter arm']);
    log.clear();
    input.simulate.hover(-1, left.$1, left.$2);
    frame();
    expect(log, ['exit arm', 'enter hand'], reason: 'still over the robot and the level');
    log.clear();
    input.simulate.hover(-1, left.$1 + 1, left.$2);
    frame();
    expect(log, isEmpty, reason: 'over the same hand');
    input.simulate.leave(-1);
    frame();
    expect(log, ['exit hand', 'exit robot', 'exit level']);
  });

  test('a finger knows what it started over and what it is over now', () {
    final r = robot();
    final f = down(middle);
    frame();
    final finger = input.fingers.single;
    expect(finger.startedOver!.gameObject, same(r.arm));
    expect(finger.startedOver!.point.z, closeTo(-4.5, 1e-5));
    move(f, nothing);
    frame();
    expect(finger.over, isNull);
    expect(finger.startedOver!.gameObject, same(r.arm));
  });

  test('input comes first in the frame: signals, then handlers, then starts and updates', () {
    robot(arm: {'click'});
    world.add(_Updates());
    input.onTap.listen((_) => log.add('onTap signal'));
    final f = down(middle);
    up(f, middle);
    frame();
    expect(log.where((l) => !l.startsWith('enter') && !l.startsWith('exit') && !l.startsWith('down')), [
      'onTap signal',
      'click arm',
      'start',
      'update',
    ]);
  });

  test("what a handler throws goes to world.errors, and the bubbling goes on", () async {
    final errors = <ObjectError>[];
    world.errors.listen(errors.add);
    final r = robot(level: {'click'});
    r.arm.addComponent(_Throws());
    input.onTap.listen((_) => throw StateError('listen'));
    final f = down(middle);
    up(f, middle);
    frame();
    await pumpEventQueue();
    expect(log, contains('click level'));
    expect(errors.map((e) => '${e.error}'), ['Bad state: listen', 'Bad state: click']);
    expect(errors.last.gameObject, same(r.arm));
  });

  test('without a camera, or a view size, nothing is under a finger', () {
    world.all<GameObject>().firstWhere((o) => o.name == 'camera').destroy();
    robot(level: {'click'});
    var f = down(middle);
    up(f, middle);
    frame();
    expect(log, isEmpty);

    world.add(GameObject(components: [Camera()]));
    input.simulate.resize(0, 0);
    f = down(middle);
    up(f, middle);
    frame();
    expect(log, isEmpty);
  });
}

class _Counts extends Behaviour {
  _Counts(this.counts);
  final List<int> counts;

  @override
  bool onClick(Finger finger) {
    counts.add(finger.tapCount);
    return true;
  }
}

class _Throws extends Behaviour {
  @override
  bool onClick(Finger finger) => throw StateError('click');
}

class _Updates extends GameObject {
  @override
  void onStart() {
    super.onStart();
    log.add('start');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    log.add('update');
  }
}
