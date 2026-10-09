// Signals, singletons and destroyed objects (docs/stage-4.md §3.5, step 4).

import 'dart:async';
import 'dart:developer' show reachabilityBarrier;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

/// Everything that happened, in order.
final log = <String>[];

/// Lets the garbage collector run until what was unreachable before is
/// gone.
Future<void> collectGarbage() async {
  final barrier = reachabilityBarrier;
  // Many small objects that live a while, so they're promoted and the old
  // space fills: what makes a full collection come (leak_tracker's way).
  final kept = <List<DateTime>>[];
  for (var i = 0; reachabilityBarrier < barrier + 2; i++) {
    if (i > 10000) fail('the garbage collector never ran');
    kept.add(List.generate(30000, (_) => DateTime.now()));
    if (kept.length > 100) kept.removeAt(0);
    await Future<void>.delayed(Duration.zero);
  }
}

/// Logs what [signal] tells it, with `on`, and what it's told on the way
/// out.
class Ear extends GameObject {
  Ear(this.label, this.signal, {this.onDisabled, this.onDestroyed});
  final String label;
  final Signal<int> signal;
  final void Function()? onDisabled;
  final void Function()? onDestroyed;

  @override
  void onLoad() {
    super.onLoad();
    on(signal, (value) => log.add('$label heard $value'));
  }

  @override
  void onDisable() {
    super.onDisable();
    onDisabled?.call();
  }

  @override
  void onDestroy() {
    super.onDestroy();
    onDestroyed?.call();
  }
}

/// The same, as a behaviour.
class EarBehaviour extends Behaviour {
  EarBehaviour(this.label, this.signal);
  final String label;
  final Signal<int> signal;

  @override
  void onLoad() {
    super.onLoad();
    on(signal, (value) => log.add('$label heard $value'));
  }
}

class Score extends GameObject {}

class Bonus extends Score {}

class Health extends Behaviour {
  final died = Signal<void>();
}

void main() {
  late World world;
  setUp(() {
    world = World();
    log.clear();
  });
  tearDown(() => world.dispose());

  /// Runs [n] frames of [dt] seconds.
  void frames(int n, [double dt = 0.25]) {
    for (var i = 0; i < n; i++) {
      world.update(dt);
    }
  }

  group('Signal', () {
    test('listeners hear each value at once, in the order they started listening', () {
      final signal = Signal<int>();
      expect(signal.hasListeners, isFalse);
      final a = signal.listen((v) => log.add('a $v'));
      signal.listen((v) => log.add('b $v'));
      expect(signal.hasListeners, isTrue);
      signal.emit(1);
      a.cancel();
      a.cancel(); // again: nothing
      expect(a.isCancelled, isTrue);
      signal.emit(2);
      expect(log, ['a 1', 'b 1', 'b 2']);
    });

    test('one added while it emits hears the next; one cancelled before its turn hears nothing', () {
      final signal = Signal<int>();
      late SignalSubscription<int> c;
      signal.listen((v) {
        log.add('a $v');
        if (v == 1) {
          signal.listen((v) => log.add('new $v'));
          c.cancel();
        }
      });
      signal.listen((v) => log.add('b $v'));
      c = signal.listen((v) => log.add('c $v'));
      signal.emit(1);
      signal.emit(2);
      expect(log, ['a 1', 'b 1', 'a 2', 'b 2', 'new 2']);
    });

    test('a listener that throws doesn\'t stop the rest; emit then throws the first error', () {
      final signal = Signal<int>();
      signal.listen((v) => throw StateError('first'));
      signal.listen((v) => log.add('b $v'));
      signal.listen((v) => throw StateError('second'));
      expect(() => signal.emit(1), throwsA(isA<StateError>().having((e) => e.message, 'message', 'first')));
      expect(log, ['b 1']);
    });

    test('emitting inside a handler reaches every listener, the outer emit goes on after', () {
      final signal = Signal<int>();
      signal.listen((v) {
        log.add('a $v');
        if (v == 1) signal.emit(2);
      });
      signal.listen((v) => log.add('b $v'));
      signal.emit(1);
      expect(log, ['a 1', 'a 2', 'b 2', 'b 1']);
    });

    test('next completes with the next value only, outside a routine', () async {
      final signal = Signal<int>();
      final next = signal.next;
      expect(signal.hasListeners, isTrue);
      signal.emit(7);
      signal.emit(8);
      expect(await next, 7);
      expect(signal.hasListeners, isFalse);
    });
  });

  group('on', () {
    test('a destroyed object never hears again; it stops listening after its onDisable, before its onDestroy', () {
      final signal = Signal<int>();
      final ear = world.add(Ear('ear', signal, onDisabled: () => signal.emit(1), onDestroyed: () => signal.emit(2)));
      final other = world.add(Ear('other', signal));
      signal.emit(0);
      ear.destroy();
      signal.emit(3);
      expect(log, ['ear heard 0', 'other heard 0', 'ear heard 1', 'other heard 1', 'other heard 2', 'other heard 3']);
      expect(other.isAlive, isTrue);
    });

    test('a behaviour stops listening when it\'s removed or its object is destroyed', () {
      final signal = Signal<int>();
      final object = world.add(GameObject(components: [EarBehaviour('one', signal), EarBehaviour('two', signal)]));
      signal.emit(1);
      object.removeComponent(object.getAll<EarBehaviour>().first);
      signal.emit(2);
      object.destroy();
      signal.emit(3);
      expect(log, ['one heard 1', 'two heard 1', 'two heard 2']);
      expect(signal.hasListeners, isFalse);
    });

    test('everything below a destroyed object stops listening too', () {
      final signal = Signal<int>();
      final root = world.add(GameObject(children: [Ear('child', signal)]));
      root.destroy();
      signal.emit(1);
      expect(log, isEmpty);
      expect(signal.hasListeners, isFalse);
    });

    test('a hidden object still hears, as in Unity', () {
      final signal = Signal<int>();
      final ear = world.add(Ear('ear', signal));
      ear.active = false;
      signal.emit(1);
      expect(log, ['ear heard 1']);
    });

    test(
      'a handler that throws goes to world.errors, with its object; the rest still hear, and emit doesn\'t throw',
      () async {
        final errors = <ObjectError>[];
        world.errors.listen(errors.add);
        final signal = Signal<int>();
        final bad = world.add(GameObject());
        bad.on(signal, (_) => throw StateError('bug'));
        world.add(Ear('ear', signal));
        signal.emit(1);
        await pumpEventQueue();
        expect(log, ['ear heard 1']);
        expect(errors.single.object, same(bad));
        expect(errors.single.error, isA<StateError>());
      },
    );

    test('a destroyed object or a removed behaviour can\'t start listening', () {
      final signal = Signal<int>();
      final object = world.add(GameObject(components: [EarBehaviour('b', signal)]));
      final behaviour = object.get<EarBehaviour>()!;
      object.removeComponent(behaviour);
      expect(() => behaviour.on(signal, (_) {}), throwsStateError);
      object.destroy();
      expect(() => object.on(signal, (_) {}), throwsStateError);
    });

    test('disposing the world stops every listener in it', () {
      final other = World();
      final signal = Signal<int>();
      other.add(Ear('ear', signal));
      other.add(GameObject(components: [EarBehaviour('b', signal)]));
      other.dispose();
      signal.emit(1);
      expect(log, isEmpty);
      expect(signal.hasListeners, isFalse);
    });

    test('a destroyed listener is let go: a signal that lives on holds nothing of it', () async {
      final signal = Signal<int>();
      WeakReference<Ear> ear() {
        final ear = world.add(Ear('ear', signal));
        ear.destroy();
        return WeakReference(ear);
      }

      final weak = ear();
      world.update(0.25);
      await collectGarbage();
      expect(weak.target, isNull);
    });

    test('a listener that lives on holds nothing of a destroyed emitter\'s signal', () async {
      final listener = world.add(GameObject());
      WeakReference<Signal<void>> enemy() {
        final enemy = world.add(GameObject(components: [Health()]));
        final died = enemy.get<Health>()!.died;
        listener.on(died, (_) {});
        enemy.destroy();
        return WeakReference(died);
      }

      final weak = enemy();
      world.update(0.25);
      await collectGarbage();
      expect(weak.target, isNull);
      expect(listener.isAlive, isTrue);
    });

    test('many signals listened to and gone leave nothing behind', () async {
      final listener = world.add(GameObject());
      final weak = <WeakReference<Signal<int>>>[];
      void listen() {
        for (var i = 0; i < 100; i++) {
          final signal = Signal<int>();
          listener.on(signal, (_) {});
          weak.add(WeakReference(signal));
        }
      }

      listen();
      await collectGarbage();
      expect(weak.where((w) => w.target != null), isEmpty);
      // And listening goes on working.
      final signal = Signal<int>();
      listener.on(signal, (v) => log.add('heard $v'));
      signal.emit(1);
      expect(log, ['heard 1']);
    });
  });

  group('next in a routine', () {
    test('the code after it runs in the routines step, after the updates, with the value', () {
      final signal = Signal<int>();
      final object = world.add(GameObject());
      object.start(() async {
        final value = await signal.next;
        log.add('got $value in frame ${object.time.frame}');
      });
      frames(1);
      signal.emit(5);
      log.add('emitted');
      frames(1);
      expect(log, ['emitted', 'got 5 in frame 2']);
      expect(signal.hasListeners, isFalse);
    });

    test('a value emitted in the frame it started waiting comes in the next frame', () {
      final signal = Signal<int>();
      final object = world.add(GameObject());
      object.start(() async {
        log.add('waiting in frame ${object.time.frame}');
        log.add('got ${await signal.next} in frame ${object.time.frame}');
      });
      signal.emit(1);
      frames(2);
      expect(log, ['waiting in frame 0', 'got 1 in frame 1']);
    });

    test('it holds while the routine is paused, and goes on when it\'s back', () {
      final signal = Signal<int>();
      final object = world.add(GameObject());
      object.start(() async {
        log.add('got ${await signal.next}');
      });
      frames(1);
      object.active = false;
      signal.emit(3);
      signal.emit(4);
      frames(3);
      expect(log, isEmpty);
      object.active = true;
      frames(1);
      expect(log, ['got 3']);
    });

    test('stopping the routine unwinds it from there, and it stops listening', () {
      final signal = Signal<int>();
      final object = world.add(GameObject());
      final routine = object.start(() async {
        try {
          await signal.next;
          log.add('never');
        } finally {
          log.add('finally');
        }
      });
      frames(1);
      routine.stop();
      expect(log, ['finally']);
      expect(signal.hasListeners, isFalse);
      signal.emit(1);
      frames(1);
      expect(log, ['finally']);
    });

    test('destroying the object stops it, and it stops listening', () {
      final signal = Signal<int>();
      final object = world.add(GameObject());
      object.start(() async {
        await signal.next;
        log.add('never');
      });
      frames(1);
      object.destroy();
      expect(signal.hasListeners, isFalse);
    });

    test('in a race, a timeout that wins drops the wait, and its listener', () {
      final signal = Signal<int>();
      final object = world.add(GameObject());
      object.start(() async {
        final won = await race<Object?>([signal.next, wait(1)]);
        log.add('race: $won');
      });
      frames(1);
      expect(signal.hasListeners, isTrue);
      frames(5);
      expect(log, ['race: null']);
      expect(signal.hasListeners, isFalse);
    });

    test('in a race, the signal that comes first wins', () {
      final signal = Signal<int>();
      final object = world.add(GameObject());
      object.start(() async {
        log.add('race: ${await race<Object?>([signal.next, wait(1)])}');
      });
      frames(1);
      signal.emit(9);
      frames(1);
      expect(log, ['race: 9']);
    });
  });

  group('only', () {
    test('gives the one object or component of a type, and throws for none or more', () {
      expect(
        () => world.only<Score>(),
        throwsA(isA<StateError>().having((e) => e.message, 'message', contains('there is no Score'))),
      );
      final score = world.add(Score());
      expect(world.only<Score>(), same(score));
      world.add(GameObject(components: [Health()]));
      expect(world.only<Health>(), isA<Health>());
      world.add(Bonus());
      expect(
        () => world.only<Score>(),
        throwsA(isA<StateError>().having((e) => e.message, 'message', contains('there are 2 of Score'))),
      );
      expect(world.only<Bonus>(), isA<Bonus>());
      score.destroy();
      expect(world.only<Score>(), isA<Bonus>());
      world.add(Bonus());
      expect(
        () => world.only<Bonus>(),
        throwsA(isA<StateError>().having((e) => e.message, 'message', contains('there are 2 of Bonus'))),
      );
    });
  });

  group('a destroyed object', () {
    test('can still be read in its onDisable and onDestroy, and says it was destroyed after', () {
      final signal = Signal<int>();
      late final Ear ear;
      ear = world.add(
        Ear(
          'ear',
          signal,
          onDisabled: () => log.add('disable: ${ear.name} at ${ear.transform.position}'),
          onDestroyed: () => log.add('destroy: ${ear.name} active ${ear.active}'),
        )..name = 'enemy',
      );
      ear.destroy();
      expect(log, ['disable: enemy at ${Vec3.zero}', 'destroy: enemy active true']);
      expect(ear.isDestroyed, isTrue);
      for (final (use, what) in [
        (() => ear.transform, 'its transform can\'t be read'),
        (() => ear.rotate(Vec3.up, 1), 'its transform can\'t be read'),
        (() => ear.name, 'its name can\'t be read'),
        (() => ear.active = true, 'active can\'t be set'),
        (() => ear.parent, 'its parent can\'t be read'),
        (() => ear.worldTransform, 'its worldTransform can\'t be read'),
      ]) {
        expect(
          use,
          throwsA(isA<StateError>().having((e) => e.message, 'message', 'Ear#${ear.id} was destroyed, so $what')),
        );
      }
    });

    test('its behaviours say so too', () {
      final object = world.add(GameObject(components: [Health()]));
      final health = object.get<Health>()!;
      object.destroy();
      expect(
        () => health.transform,
        throwsA(
          isA<StateError>().having(
            (e) => e.message,
            'message',
            'GameObject#${object.id} was destroyed, so its transform can\'t be read',
          ),
        ),
      );
    });

    test('its components can\'t be changed', () {
      final camera = Camera();
      final object = world.add(GameObject(components: [camera]));
      object.destroy();
      expect(
        () => camera.fovY = 1,
        throwsA(
          isA<StateError>().having(
            (e) => e.message,
            'message',
            'GameObject#${object.id} was destroyed, so its Camera can\'t be changed',
          ),
        ),
      );
      final light = Light.sun();
      final other = world.add(GameObject(components: [light]));
      other.removeComponent(light);
      expect(
        () => light.intensity = 2,
        throwsA(
          isA<StateError>().having(
            (e) => e.message,
            'message',
            'Light was removed from GameObject#${other.id}, so it can\'t be changed',
          ),
        ),
      );
    });
  });
}
