// The fixed step and the world's systems (docs/stage-5.md §3.1, §3.2,
// step 2).

import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

/// Every call, in order.
final log = <String>[];

/// Counts the presses and releases its fixed steps see, of space, the
/// left mouse button and the 'jump' action.
class FixedPresses extends GameObject {
  var keys = 0, keyUps = 0, buttons = 0, actions = 0;

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    if (input.wasPressed(KeyCode.space)) keys++;
    if (input.wasReleased(KeyCode.space)) keyUps++;
    if (input.wasPressed(MouseButton.left)) buttons++;
    if (input.action('jump').wasPressed) actions++;
  }
}

class Ticks extends GameObject {
  Ticks(String name, {super.components, super.children}) : super(name: name);

  final deltas = <double>[];
  final times = <double>[];
  var fixed = 0;

  @override
  void onStart() {
    super.onStart();
    log.add('start $name');
  }

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    fixed++;
    deltas.add(time.delta);
    times.add(time.sinceStart);
    expect(time.inFixedStep, isTrue);
    log.add('fixed $name');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    expect(time.inFixedStep, isFalse);
    log.add('update $name');
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    log.add('late $name');
  }
}

class FixedLogs extends Behaviour {
  FixedLogs(this.label);

  final String label;

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    log.add('fixed $label');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    log.add('update $label');
  }
}

class Recorder extends WorldSystem {
  Recorder(super.world);

  var disposed = false;

  @override
  void beforeFrame() {
    super.beforeFrame();
    expect(world.time.inFixedStep, isFalse);
    log.add('system frame');
  }

  @override
  void beforeUpdate() {
    super.beforeUpdate();
    log.add('system updates');
  }

  @override
  void afterFrame() {
    super.afterFrame();
    log.add('system end');
  }

  @override
  void beforeFixedUpdate() {
    super.beforeFixedUpdate();
    expect(world.time.inFixedStep, isTrue);
    log.add('system before');
  }

  @override
  void afterFixedUpdate() {
    super.afterFixedUpdate();
    log.add('system after');
  }

  @override
  void afterLateUpdate() {
    super.afterLateUpdate();
    log.add('system late');
  }

  @override
  void dispose() {
    disposed = true;
    super.dispose();
  }
}

class Throws extends WorldSystem {
  Throws(super.world);

  @override
  void afterFixedUpdate() {
    super.afterFixedUpdate();
    throw StateError('the system broke');
  }
}

World newWorld() {
  final world = World();
  addTearDown(world.dispose);
  log.clear();
  return world;
}

void main() {
  group('fixed steps', () {
    test('run as many as the frame\'s time holds, keeping the rest for the next frame', () {
      final world = newWorld();
      final a = world.add(Ticks('a'));
      world.update(0); // a starts
      a.fixed = 0;
      world.update(0.05);
      expect(a.fixed, 2, reason: '0.05 s holds two steps of 0.02');
      world.update(0.01);
      expect(a.fixed, 3, reason: 'the 0.01 left and this 0.01 make a third');
      world.update(0.019);
      expect(a.fixed, 3);
    });

    test('come 50 times a second at any frame rate', () {
      for (final rate in [30, 60, 120, 144, 240]) {
        final world = newWorld();
        final a = world.add(Ticks('a'));
        for (var i = 0; i < rate; i++) {
          world.update(1 / rate);
        }
        expect(a.fixed, 50, reason: 'at $rate fps');
      }
    });

    test('see time.delta as fixedDelta and sinceStart as the steps\' own time', () {
      final world = newWorld();
      final a = world.add(Ticks('a'));
      world.time.fixedDelta = 0.025;
      world.update(0.06);
      expect(world.time.delta, closeTo(0.06, 1e-12), reason: 'outside the steps, the frame\'s');
      expect(a.deltas, [0.025, 0.025]);
      expect(a.times[0], closeTo(0.025, 1e-12));
      expect(a.times[1], closeTo(0.05, 1e-12));
      expect(world.time.sinceStart, closeTo(0.06, 1e-12));
      expect(world.time.inFixedStep, isFalse);
    });

    test('follow time.scale, and time.maxDelta limits them after a hitch', () {
      final world = newWorld();
      final a = world.add(Ticks('a'));
      world.time.scale = 0.5;
      world.update(0.08);
      expect(a.fixed, 2, reason: '0.08 s at half speed is 0.04 s of game time');
      world.time.scale = 0;
      world.update(1 / 60);
      expect(a.fixed, 2, reason: 'paused, none');
      world.time.scale = 1;
      world.time.maxFixedSteps = 100;
      world.update(5);
      expect(a.fixed, 2 + 16, reason: 'a 5 s hitch counts as maxDelta, 1/3 s: 16 steps');
    });

    test('a frame runs at most maxFixedSteps, and drops the rest of its time', () {
      final world = newWorld();
      final a = world.add(Ticks('a'));
      expect(world.time.maxFixedSteps, 5);
      world.update(5);
      expect(a.fixed, 5, reason: 'a hitch holds 16 steps; 5 run');
      world.update(0.01);
      expect(a.fixed, 5, reason: 'the 11 dropped don\'t come in the next frames');
      world.update(0.02);
      expect(a.fixed, 6);
      expect(() => world.time.maxFixedSteps = 0, throwsArgumentError);
    });

    test('maxDelta limits the game\'s time, after the scale', () {
      final world = newWorld();
      final a = world.add(Ticks('a'));
      world.time.scale = 5;
      world.time.maxFixedSteps = 100;
      world.update(1);
      expect(world.time.delta, closeTo(1 / 3, 1e-12), reason: 'not 5/3');
      expect(world.time.unscaledDelta, closeTo(1 / 3, 1e-12));
      expect(a.fixed, 16);
      world.update(1 / 60);
      expect(world.time.delta, closeTo(5 / 60, 1e-12), reason: 'a normal frame is scaled as before');
    });

    test('fixedAlpha is how far the frame is towards the next step', () {
      final world = newWorld();
      expect(world.time.fixedAlpha, 0);
      world.update(0.03);
      expect(world.time.fixedAlpha, closeTo(0.5, 1e-9));
      world.update(0.005);
      expect(world.time.fixedAlpha, closeTo(0.75, 1e-9));
      world.time.fixedDelta = 0.01;
      expect(world.time.fixedAlpha, 1, reason: 'a step is due: never past 1');
    });

    test('fixedDelta must be more than 0', () {
      final world = newWorld();
      expect(() => world.time.fixedDelta = 0, throwsArgumentError);
      expect(() => world.time.fixedDelta = double.nan, throwsArgumentError);
      expect(() => world.time.fixedDelta = double.infinity, throwsArgumentError);
      expect(world.time.fixedDelta, 0.02);
    });
  });

  group('the order', () {
    test('starts, then each step (systems, objects, behaviours, systems), then updates, then the systems\' late', () {
      final world = newWorld();
      world.system(Recorder.new);
      world.add(Ticks('a', components: [FixedLogs('a.b')], children: [Ticks('c')]));
      world.update(0.04);
      expect(log, [
        'system frame',
        'start a',
        'start c',
        'system before',
        'fixed a',
        'fixed a.b',
        'fixed c',
        'system after',
        'system before',
        'fixed a',
        'fixed a.b',
        'fixed c',
        'system after',
        'system updates',
        'update a',
        'update a.b',
        'update c',
        'late a',
        'late c',
        'system late',
        'system end',
      ]);
    });

    test('a frame with no step runs no fixed calls, and the systems\' late still comes', () {
      final world = newWorld();
      world.system(Recorder.new);
      world.add(Ticks('a'));
      world.update(0.01);
      expect(log, ['system frame', 'start a', 'system updates', 'update a', 'late a', 'system late', 'system end']);
    });

    test('hidden objects and disabled behaviours get no fixed step; new ones start first', () {
      final world = newWorld();
      final b = FixedLogs('b');
      final a = world.add(Ticks('a', components: [b]));
      final hidden = world.add(Ticks('hidden'))..active = false;
      world.update(0.02);
      expect(log, contains('fixed a'));
      expect(log, isNot(contains('fixed hidden')));
      log.clear();
      b.enabled = false;
      world.update(0.02);
      expect(log, ['fixed a', 'update a', 'late a']);
      log.clear();
      a.add(Ticks('late comer'));
      world.update(0.02);
      expect(log, [
        'start late comer',
        'fixed a',
        'fixed late comer',
        'update a',
        'update late comer',
        'late a',
        'late late comer',
      ]);
      expect(hidden.fixed, 0);
      log.clear();
      a.active = false;
      world.update(0.02);
      expect(log, isEmpty, reason: 'started, then hidden');
    });

    test('an object destroyed in a step gets no more calls, and the others go on', () {
      final world = newWorld();
      final a = world.add(Ticks('a'));
      final b = world.add(_DestroysInFixed());
      world.update(0.04);
      expect(a.fixed, 2);
      expect(b.fixed, 1);
      expect(b.isDestroyed, isTrue);
    });

    test('an error in onFixedUpdate reaches world.errors and the step goes on', () async {
      final world = newWorld();
      final errors = <ObjectError>[];
      world.errors.listen(errors.add);
      final bad = world.add(_ThrowsInFixed());
      final a = world.add(Ticks('a'));
      world.update(0.04);
      await pumpEventQueue();
      expect(a.fixed, 2);
      expect(errors, hasLength(2));
      expect(errors.first.gameObject, bad);
      expect(errors.first.error, isA<StateError>());
    });
  });

  group('routines', () {
    test('fixedStep ends after the next fixed step, and waits for a frame that has one', () async {
      final world = newWorld();
      final a = world.add(Ticks('a'));
      world.system(Recorder.new);
      world.update(0); // a starts
      final steps = <int>[];
      a.startRoutine(() async {
        for (var i = 0; i < 3; i++) {
          await fixedStep;
          expect(world.time.inFixedStep, isTrue);
          steps.add(a.fixed);
          log.add('routine');
        }
      });
      log.clear();
      world.update(0.01);
      expect(steps, isEmpty, reason: 'no step this frame');
      world.update(0.03);
      expect(steps, [1, 2], reason: 'a step\'s waits end after it, so each step wakes it once');
      expect(
        log.where(
          (l) => l.startsWith('fixed') || l == 'routine' || l.startsWith('system before') || l == 'system after',
        ),
        ['system before', 'fixed a', 'system after', 'routine', 'system before', 'fixed a', 'system after', 'routine'],
      );
      world.update(0.02);
      expect(steps, [1, 2, 3]);
    });

    test('one started in onFixedUpdate waits for the next step, not the one it was started in', () {
      final world = newWorld();
      final starter = world.add(_StartsInFixed());
      world.update(0.02);
      expect(starter.woke, isEmpty);
      world.update(0.02);
      expect(starter.woke, [1], reason: 'started in step 1, woken after step 2');
    });
  });

  group('systems', () {
    test('one of each type per world, made the first time it\'s asked for', () {
      final world = newWorld();
      final other = newWorld();
      final first = world.system(Recorder.new);
      expect(world.system(Recorder.new), same(first));
      expect(other.system(Recorder.new), isNot(same(first)));
      expect(first.world, world);
      expect(world.maybeSystem<Recorder>(), same(first));
      expect(world.maybeSystem<Throws>(), isNull);
    });

    test('are disposed with their world', () {
      final world = World();
      final recorder = world.system(Recorder.new);
      world.dispose();
      expect(recorder.disposed, isTrue);
      expect(() => world.system(Recorder.new), throwsStateError);
    });

    test('an error in a system reaches world.errors with no object, and the frame goes on', () async {
      final world = newWorld();
      final errors = <ObjectError>[];
      world.errors.listen(errors.add);
      world.system(Throws.new);
      final a = world.add(Ticks('a'));
      world.update(0.04);
      await pumpEventQueue();
      expect(a.fixed, 2);
      expect(log, contains('update a'));
      expect(errors, hasLength(2));
      expect(errors.first.gameObject, isNull);
      expect(errors.first.error, isA<StateError>());
    });

    test('hear of components coming into the world, and read them without a list', () async {
      final world = newWorld();
      final system = world.system(_Components.new);
      final early = _Plain();
      final object = world.add(GameObject(components: [early]));
      expect(system.added, [(object, early)], reason: 'an object\'s own components, as it\'s added');
      final late = object.addComponent(_Plain());
      expect(system.added, [(object, early), (object, late)]);
      expect([for (var i = 0; i < object.componentCount; i++) object.componentAt(i)], [early, late]);
      object.removeComponent(early);
      expect(object.componentAt(0).isRemoved, isTrue);
      final errors = <ObjectError>[];
      world.errors.listen(errors.add);
      world.report(object, StateError('a callback broke'), StackTrace.current);
      await pumpEventQueue();
      expect(errors.single.gameObject, same(object));
    });
  });

  test('a press is seen by exactly one fixed step, however many the frame runs', () {
    final world = World();
    addTearDown(world.dispose);
    world.input.simulate.resize(100, 100);
    final presses = world.add(FixedPresses());
    world.input.bindAction('jump', [KeyCode.j]);
    world.update(0);
    // A frame of two steps.
    world.input.simulate
      ..keyDown(KeyCode.space)
      ..keyDown(KeyCode.j)
      ..fingerDown(1, 50, 50, kind: FingerKind.mouse);
    world.update(2 / 50);
    expect((presses.keys, presses.buttons, presses.actions), (1, 1, 1), reason: 'not twice');
    // Frames of no step: the release waits for the next one.
    world.input.simulate.keyUp(KeyCode.space);
    world.update(0.25 / 50);
    world.input.simulate.keyDown(KeyCode.space);
    world.update(0.25 / 50);
    expect((presses.keys, presses.keyUps), (1, 0));
    world.update(0.5 / 50);
    expect((presses.keys, presses.keyUps), (2, 1), reason: 'not never');
    world.update(1 / 50);
    expect((presses.keys, presses.keyUps), (2, 1));
  });
}

class _Plain extends Behaviour {}

class _Components extends WorldSystem {
  _Components(super.world);

  final added = <(GameObject, Component)>[];

  @override
  void componentAdded(GameObject object, Component component) {
    super.componentAdded(object, component);
    added.add((object, component));
  }
}

class _DestroysInFixed extends GameObject {
  var fixed = 0;

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    fixed++;
    destroy();
  }
}

class _ThrowsInFixed extends GameObject {
  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    throw StateError('broken');
  }
}

class _StartsInFixed extends GameObject {
  final woke = <int>[];
  var _steps = 0;

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    if (_steps++ > 0) return;
    startRoutine(() async {
      await fixedStep;
      woke.add(_steps - 1);
    });
  }
}
