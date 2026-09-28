// Pooling (docs/foundation.md 3.2): Unity's `UnityEngine.Pool.ObjectPool<T>`
// — create on demand, reuse what was released, a hook for each of get,
// release and destroy, a size cap, a check against releasing twice — and
// `ObjectPool.gameObjects`, the usual setup for objects: switched on when
// taken, off when given back, destroyed past the cap. Over the real
// native library.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class Bullet extends Behaviour {
  int enables = 0;
  int disables = 0;

  @override
  void onEnable() => enables++;

  @override
  void onDisable() => disables++;
}

class Thing {
  Thing(this.n);
  final int n;
}

void main() {
  group('ObjectPool', () {
    test('creates when empty, reuses what was released, last in first out', () {
      var made = 0;
      final pool = ObjectPool<Thing>(create: () => Thing(made++));
      final a = pool.get();
      final b = pool.get();
      expect((made, pool.countAll, pool.countActive, pool.countInactive), (2, 2, 2, 0));
      pool.release(a);
      pool.release(b);
      expect((pool.countActive, pool.countInactive), (0, 2));
      expect(pool.get(), same(b));
      expect(pool.get(), same(a));
      expect(made, 2, reason: 'nothing new while the pool had some');
    });

    test('runs onGet, onRelease and onDestroy; past maxSize a release destroys', () {
      final log = <String>[];
      var made = 0;
      final pool = ObjectPool<Thing>(
        create: () => Thing(made++),
        onGet: (t) => log.add('get ${t.n}'),
        onRelease: (t) => log.add('release ${t.n}'),
        onDestroy: (t) => log.add('destroy ${t.n}'),
        maxSize: 1,
      );
      final a = pool.get();
      final b = pool.get();
      pool.release(a);
      pool.release(b);
      expect(log, ['get 0', 'get 1', 'release 0', 'release 1', 'destroy 1']);
      expect((pool.countAll, pool.countInactive), (1, 1));
      log.clear();
      pool.clear();
      expect(log, ['destroy 0']);
      expect((pool.countAll, pool.countInactive), (0, 0));
    });

    test('prewarm makes objects ahead, released and waiting, up to maxSize', () {
      final log = <String>[];
      var made = 0;
      final pool = ObjectPool<Thing>(
        create: () => Thing(made++),
        onGet: (t) => log.add('get ${t.n}'),
        onRelease: (t) => log.add('release ${t.n}'),
        maxSize: 3,
      );
      pool.prewarm(2);
      expect((made, pool.countAll, pool.countInactive), (2, 2, 2));
      expect(log, ['release 0', 'release 1'], reason: 'made and put away; never handed out');
      pool.prewarm(5);
      expect((made, pool.countInactive), (3, 3), reason: 'no more than maxSize waiting');
      pool.get();
      expect(made, 3);
    });

    test('releasing twice throws (collectionCheck), unless turned off', () {
      final pool = ObjectPool<Thing>(create: () => Thing(0));
      final a = pool.get();
      pool.release(a);
      expect(() => pool.release(a), throwsStateError);
      final loose = ObjectPool<Thing>(create: () => Thing(0), collectionCheck: false);
      final b = loose.get();
      loose.release(b);
      loose.release(b);
      expect(loose.countInactive, 2);
    });
  });

  group('ObjectPool.gameObjects', () {
    late NativeLibrary lib;
    setUpAll(() => lib = NativeLibrary.open());
    tearDownAll(() => lib.dispose());

    World fresh() {
      final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8)..throwOnDiagnostics = true;
      addTearDown(w.dispose);
      return w;
    }

    test('taken objects are on, released ones off, and the same object comes back', () {
      final w = fresh();
      var made = 0;
      final pool = ObjectPool.gameObjects(() {
        made++;
        final o = w.spawn(name: 'Bullet');
        o.addBehaviour(Bullet());
        return o;
      });
      final first = pool.get();
      w.frame(1 / 60);
      final bullet = first.behaviour<Bullet>()!;
      expect(first.activeInHierarchy, isTrue);

      pool.release(first);
      expect(first.activeSelf, isFalse);
      w.frame(1 / 60);
      expect(first.activeInHierarchy, isFalse);
      expect(bullet.disables, 1);

      final again = pool.get();
      expect(again, same(first));
      expect(made, 1);
      w.frame(1 / 60);
      expect(again.activeInHierarchy, isTrue);
      expect(bullet.enables, 2);
    });

    test('an object destroyed while in the pool is dropped, not handed out', () {
      final w = fresh();
      final pool = ObjectPool.gameObjects(() => w.spawn(name: 'Bullet'));
      final a = pool.get();
      w.frame(1 / 60);
      pool.release(a);
      a.destroy(); // a scene load, or the game, took it
      w.frame(1 / 60);
      expect(a.isAlive, isFalse);
      final b = pool.get();
      expect(b, isNot(same(a)));
      expect(b.isAlive, isTrue);
      expect((pool.countAll, pool.countInactive), (1, 0));
    });

    test('prewarmed objects wait switched off', () {
      final w = fresh();
      final pool = ObjectPool.gameObjects(() => w.spawn(name: 'Bullet'))..prewarm(3);
      w.frame(1 / 60);
      final a = pool.get();
      expect(pool.countInactive, 2);
      w.frame(1 / 60);
      expect(a.activeInHierarchy, isTrue);
      expect(w.findAll('Bullet').where((o) => !o.activeInHierarchy), hasLength(2));
    });

    test('past maxSize, and on clear, the objects are destroyed', () {
      final w = fresh();
      final pool = ObjectPool.gameObjects(() => w.spawn(name: 'Bullet'), maxSize: 1);
      final a = pool.get();
      final b = pool.get();
      w.frame(1 / 60);
      pool.release(a);
      pool.release(b);
      w.frame(1 / 60);
      expect(b.isAlive, isFalse);
      expect(a.isAlive, isTrue);
      pool.clear();
      w.frame(1 / 60);
      expect(a.isAlive, isFalse);
    });
  });
}
