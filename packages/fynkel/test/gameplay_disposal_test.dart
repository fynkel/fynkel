import 'package:fynkel/advanced.dart';
import 'package:test/test.dart';

class DisposeInFixed extends Behaviour {
  int calls = 0;
  @override
  void fixedUpdate(FixedUpdateContext context) {
    calls++;
    world.dispose();
  }
}

class LaterFixed extends Behaviour {
  int calls = 0;
  @override
  void fixedUpdate(FixedUpdateContext context) => calls++;
}

class DisposeInLifecycle extends Behaviour {
  DisposeInLifecycle(this.atCreate);
  final bool atCreate;
  @override
  void onCreate() {
    if (atCreate) world.dispose();
  }

  @override
  void start() {
    if (!atCreate) world.dispose();
  }
}

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  World fresh() {
    final world = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(world.dispose);
    return world;
  }

  for (final manual in [false, true]) {
    test('host fixed disposal stops ${manual ? 'stepFrame' : 'catch-up frame'} before native stepping', () {
      final world = fresh();
      var fixed = 0;
      var updates = 0;
      void callback(int tick) {
        fixed++;
        world.dispose();
        expect(world.isDisposed, isFalse);
      }

      if (manual) {
        world.stepFrame(fixedUpdate: callback);
      } else {
        world.frame(4 / 60, fixedUpdate: callback, update: () => updates++);
      }
      expect(world.isDisposed, isTrue);
      expect(fixed, 1);
      expect(updates, 0);
      expect(world.frameTimes.ticks, 0);
    });

    test(
      'behaviour fixed disposal stops ${manual ? 'stepFrame' : 'catch-up frame'} and later behaviours',
      () {
        final world = fresh();
        final disposer = DisposeInFixed();
        final later = LaterFixed();
        world.edit(() {
          world.spawn(name: 'disposer').addBehaviour(disposer);
          world.spawn(name: 'later').addBehaviour(later);
        });
        if (manual) {
          world.stepFrame();
        } else {
          world.frame(4 / 60);
        }
        expect(world.isDisposed, isTrue);
        expect(disposer.calls, 1);
        expect(later.calls, 0);
        expect(world.frameTimes.ticks, 0);
      },
    );
  }

  test('disposal still reports an earlier deferred callback failure', () {
    final world = fresh();
    world.spawn(name: 'disposer').addBehaviour(DisposeInFixed());
    expect(
      () => world.frame(1 / 60, fixedUpdate: (_) => throw StateError('original failure')),
      throwsA(isA<StateError>().having((error) => error.message, 'message', 'original failure')),
    );
    expect(world.isDisposed, isTrue);
  });
  for (final atCreate in [true, false]) {
    test('disposal in ${atCreate ? 'onCreate' : 'start'} ends the frame safely', () {
      final world = fresh();
      world.edit(() => world.spawn(name: 'lifecycle').addBehaviour(DisposeInLifecycle(atCreate)));
      world.frame(1 / 60);
      expect(world.isDisposed, isTrue);
      expect(world.frameTimes.ticks, 0);
    });
  }
}
