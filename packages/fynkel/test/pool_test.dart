// Pools and handles (docs/stage-6-fixes.md, PR 9; findings 8 and 30).

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

/// Logs its onEnable and onDisable, with its hit points, so the order of
/// a get's reset and its onEnable shows.
class Shot extends GameObject {
  Shot() : super(name: 'shot');

  static final log = <String>[];
  var hits = 0;

  @override
  void onEnable() {
    super.onEnable();
    log.add('enable $hits');
  }

  @override
  void onDisable() {
    super.onDisable();
    log.add('disable');
  }
}

/// Listens to [Listener.ping] once on load and again each time it's
/// shown, as the guide's pooling section suggests.
class Listener extends GameObject {
  Listener() : super(name: 'listener');

  static final ping = Signal<int>();
  var heardLoaded = 0;
  var heardShown = 0;

  @override
  void onLoad() {
    super.onLoad();
    on(ping, (_) => heardLoaded++);
  }

  @override
  void onEnable() {
    super.onEnable();
    on(ping, (_) => heardShown++);
  }
}

class Ticker extends Behaviour {
  var ticks = 0;
}

void main() {
  late World world;
  late GameObject root;
  late Pool<Shot> shots;
  setUp(() {
    world = World();
    root = world.add(GameObject(name: 'root'));
    shots = Pool(root, Shot.new);
    Shot.log.clear();
  });
  tearDown(() => world.dispose());

  test('get makes an object under the parent, and release hides it for the next get', () {
    final a = shots.get();
    world.update(0);
    expect(a.parent, same(root));
    expect(a.active, isTrue);
    expect((shots.countActive, shots.countInactive, shots.countAll), (1, 0, 1));
    shots.release(a);
    expect(a.active, isFalse);
    expect((shots.countActive, shots.countInactive, shots.countAll), (0, 1, 1));
    expect(world.all<Shot>(), isEmpty, reason: 'world.all skips hidden objects');
    final b = shots.get();
    expect(b, same(a));
    expect((shots.countActive, shots.countInactive, shots.countAll), (1, 0, 1));
  });

  test('the reset runs before onEnable, which sees what it did', () {
    final a = shots.get((s) => s.hits = 1);
    world.update(0);
    shots.release(a);
    shots.get((s) => s.hits = 2);
    expect(Shot.log, ['enable 1', 'disable', 'enable 2']);
  });

  test('a released object\'s routines and motions stop, below it and on its behaviours too', () {
    final a = shots.get();
    final child = a.add(GameObject());
    final ticker = a.addComponent(Ticker());
    world.update(0);
    var ticks = 0;
    a.startRoutine(() async {
      while (true) {
        await wait(0.1);
        ticks++;
      }
    });
    child.startRoutine(() async {
      await wait(0.1);
      ticks += 100;
    });
    ticker.startRoutine(() async {
      await wait(0.1);
      ticks += 1000;
    });
    final move = a.moveTo(const Vec3(10, 0, 0), seconds: 1);
    shots.release(a);
    expect(move.isRunning, isFalse);
    final again = shots.get();
    expect(again, same(a));
    for (var i = 0; i < 5; i++) {
      world.update(0.25);
    }
    expect(ticks, 0, reason: 'nothing resumes stale');
    expect(a.position, Vec3.zero);
  });

  test('a released object goes back under the parent', () {
    final a = shots.get();
    final elsewhere = world.add(GameObject());
    a.parent = elsewhere;
    shots.release(a);
    expect(a.parent, same(root));
    elsewhere.destroy();
    expect(a.isDestroyed, isFalse);
  });

  test('a handle goes null once its object is released, even when it\'s handed out again', () {
    final a = shots.get();
    final handle = ObjectHandle(a);
    expect(handle.object, same(a));
    expect(handle.isAlive, isTrue);
    shots.release(a);
    expect(handle.object, isNull);
    final b = shots.get();
    expect(b, same(a));
    expect(handle.object, isNull, reason: 'it was handed out as something new');
    final newer = ObjectHandle(b);
    expect(newer.object, same(b));
    expect(newer == handle, isFalse);
    expect(newer == ObjectHandle(b), isTrue);
    b.destroy();
    expect(newer.object, isNull);
  });

  test('a handle to an object that isn\'t pooled goes null when it\'s destroyed', () {
    final plain = world.add(GameObject());
    final handle = ObjectHandle(plain);
    expect(handle.object, same(plain));
    plain.destroy();
    expect(handle.object, isNull);
  });

  test('the object\'s own release gives it back, or destroys one that isn\'t pooled', () {
    final a = shots.get();
    a.release();
    expect(shots.countInactive, 1);
    expect(a.isDestroyed, isFalse);
    final plain = world.add(GameObject());
    plain.release();
    expect(plain.isDestroyed, isTrue);
  });

  test('releasing twice, or into another pool, throws; a destroyed one is left alone', () {
    final a = shots.get();
    shots.release(a);
    expect(() => shots.release(a), throwsStateError);
    expect(() => Pool(root, Shot.new).release(shots.get()), throwsArgumentError);
    final b = shots.get();
    b.destroy();
    shots.release(b);
    expect(shots.countAll, 1);
  });

  test('make must make a new object each time', () {
    final one = Shot();
    final pool = Pool(root, () => one);
    pool.get();
    expect(pool.get, throwsStateError);
  });

  test('destroying a pooled object takes it out of the counts', () {
    final a = shots.get(), b = shots.get(), c = shots.get();
    shots.release(c);
    a.destroy();
    c.destroy();
    expect((shots.countActive, shots.countInactive, shots.countAll), (1, 0, 1));
    expect(shots.get(), isNot(anyOf(same(a), same(b), same(c))));
  });

  test('destroying the parent destroys the pool\'s objects', () {
    final a = shots.get();
    shots.release(shots.get());
    root.destroy();
    expect(a.isDestroyed, isTrue);
    expect(shots.countAll, 0);
  });

  test('beyond maxSize a release destroys', () {
    final pool = Pool(root, Shot.new, maxSize: 1);
    final a = pool.get(), b = pool.get();
    pool.release(a);
    pool.release(b);
    expect(a.isDestroyed, isFalse);
    expect(b.isDestroyed, isTrue);
    expect((pool.countActive, pool.countInactive), (0, 1));
    expect(() => Pool(root, Shot.new, maxSize: -1), throwsArgumentError);
  });

  test('prewarm makes hidden objects; clear destroys them', () {
    shots.prewarm(3);
    world.update(0);
    expect((shots.countActive, shots.countInactive), (0, 3));
    expect(Shot.log, isEmpty, reason: 'they never show');
    expect(world.all<Shot>(), isEmpty);
    final a = shots.get();
    expect(shots.countInactive, 2);
    shots.clear();
    expect((shots.countActive, shots.countInactive), (1, 0));
    expect(a.isDestroyed, isFalse);
  });

  test('a pool made before its parent is added works once it is', () {
    final later = GameObject();
    final pool = Pool(later, Shot.new);
    final a = pool.get();
    pool.release(a);
    world.add(later);
    expect(a.parent, same(later));
    expect(a.active, isFalse);
    expect(identical(pool.get(), a), isTrue);
  });

  test('the objects out can be walked, and released as they are, from the end (finding 39)', () {
    final made = [for (var i = 0; i < 5; i++) shots.get((shot) => shot.hits = i)];
    world.update(0);
    expect(shots.active, made);
    shots.release(made[1]);
    expect(shots.active, unorderedEquals([made[0], made[2], made[3], made[4]]));
    for (var i = shots.countActive - 1; i >= 0; i--) {
      final shot = shots.activeAt(i);
      if (shot.hits.isEven) shots.release(shot);
    }
    expect(shots.active, unorderedEquals([made[3]]));
    final copy = shots.active;
    made[3].destroy();
    expect((copy.length, shots.countActive), (1, 0), reason: 'a destroyed one leaves; the copy stays');
    expect(() => shots.activeAt(0), throwsRangeError);
  });

  test('a release cancels what was listened to while shown, and keeps what was listened to on load', () {
    final pool = Pool(root, Listener.new);
    final a = pool.get();
    world.update(0);
    for (var i = 0; i < 5; i++) {
      pool.release(a);
      expect(pool.get(), same(a));
    }
    Listener.ping.emit(1);
    expect((a.heardShown, a.heardLoaded), (1, 1), reason: 'one handler from the last onEnable, one from the load');
    pool.release(a);
    Listener.ping.emit(2);
    expect((a.heardShown, a.heardLoaded), (1, 2), reason: 'released: only the load\'s handler still hears');
  });
}
