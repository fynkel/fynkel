// Stage 6's stability check (docs/stage-6.md, step 5): errors thrown from
// every place a game's code runs, every frame, for many frames; the game
// goes on and each error is reported; a system throwing every frame is
// turned off.

import 'dart:async';

import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

/// Throws from everything it can be called in.
class Thrower extends GameObject {
  Thrower()
    : super(
        transform: Transform(position: Vec3(0, 0, -5)),
        components: [MeshRenderer(Mesh.cube(), Material())],
      );

  final pulse = Signal<int>();

  Never _throw(String where) => throw StateError(where);

  @override
  void onEnable() {
    super.onEnable();
    _throw('onEnable');
  }

  @override
  void onStart() {
    super.onStart();
    on(pulse, (_) => _throw('signal'));
    startRoutine(() async {
      while (true) {
        await nextFrame;
        // A routine that throws ends; start another each time.
        startRoutine(() async {
          await nextFrame;
          _throw('routine');
        });
        if (time.frame % 30 == 0) moveTo(Vec3(0, 0, -5), seconds: 0.01, onComplete: () => _throw('tween'));
      }
    });
    _throw('onStart');
  }

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    _throw('onFixedUpdate');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    pulse.emit(time.frame);
    _throw('onUpdate');
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    _throw('onLateUpdate');
  }

  @override
  void onDisable() {
    super.onDisable();
    _throw('onDisable');
  }

  @override
  bool onClick(Finger finger) => _throw('onClick');

  @override
  bool onDragStart(Finger finger) => _throw('onDragStart');

  @override
  // A throw here counts as not taken, so the tap's click, drag and up
  // don't come: every other tap only.
  bool onPointerDown(Finger finger) => finger.id.isEven ? _throw('onPointerDown') : true;

  @override
  void onPointerUp(Finger finger) => _throw('onPointerUp');
}

/// A behaviour that throws from all its calls.
class ThrowingBehaviour extends Behaviour {
  Never _throw(String where) => throw StateError('behaviour $where');

  @override
  void onEnable() {
    super.onEnable();
    _throw('onEnable');
  }

  @override
  void onStart() {
    super.onStart();
    _throw('onStart');
  }

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    _throw('onFixedUpdate');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _throw('onUpdate');
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    _throw('onLateUpdate');
  }

  @override
  void onDisable() {
    super.onDisable();
    _throw('onDisable');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    _throw('onDestroy');
  }
}

/// Fails to load, and so is destroyed (and throws there too).
class FailsToLoad extends GameObject {
  @override
  Future<void> onLoad() async {
    await super.onLoad();
    throw StateError('onLoad');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    throw StateError('onDestroy');
  }
}

/// A system that throws from every hook.
class ThrowingSystem extends WorldSystem {
  ThrowingSystem(super.world);

  @override
  void beforeFrame() {
    super.beforeFrame();
    throw StateError('system beforeFrame');
  }

  @override
  void beforeUpdate() {
    super.beforeUpdate();
    throw StateError('system beforeUpdate');
  }

  @override
  void afterLateUpdate() {
    super.afterLateUpdate();
    throw StateError('system afterLateUpdate');
  }

  @override
  void afterFrame() {
    super.afterFrame();
    throw StateError('system afterFrame');
  }
}

/// Counts its afterFrame calls: the frame got to its end.
class FrameEnds extends WorldSystem {
  FrameEnds(super.world);

  var ends = 0;

  @override
  void afterFrame() {
    super.afterFrame();
    ends++;
  }
}

/// Counts what it gets: the game going on.
class Witness extends GameObject {
  var updates = 0, lates = 0, fixed = 0, waits = 0;

  @override
  void onStart() {
    super.onStart();
    startRoutine(() async {
      while (true) {
        await wait(0.1);
        waits++;
      }
    });
  }

  @override
  void onUpdate() {
    super.onUpdate();
    updates++;
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    lates++;
  }

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    fixed++;
  }
}

void main() {
  test('errors from every place game code runs, every frame for 600 frames: the game goes on', () async {
    final world = World();
    addTearDown(world.dispose);
    final errors = <String>{};
    var count = 0;
    world.errors.listen((e) {
      count++;
      final message = e.error is StateError ? (e.error as StateError).message : '${e.error}';
      errors.add(message.startsWith('ThrowingSystem threw in') ? 'system turned off' : message);
    });
    world.system(ThrowingSystem.new);
    world.add(GameObject(components: [Camera()]));
    final witness = world.add(Witness());
    final thrower = world.add(Thrower());
    thrower.addComponent(ThrowingBehaviour());

    var finger = 1;
    for (var frame = 1; frame <= 600; frame++) {
      // Every 10 frames a tap or (every other time) a drag of the thrower,
      // over two frames; between them it's turned off and on again, and one that
      // fails to load is added.
      world.input.simulate.resize(100, 100);
      if (frame % 10 == 0) world.input.simulate.fingerDown(finger, 50, 50);
      if (frame % 10 == 1) {
        final to = frame % 20 == 1 ? 50.0 : 80.0;
        world.input.simulate
          ..fingerMove(finger, to, 50)
          ..fingerUp(finger, to, 50);
        finger++;
      }
      if (frame % 10 == 5) {
        thrower
          ..active = false
          ..active = true;
        world.add(FailsToLoad());
      }
      world.update(1 / 60);
      await pumpEventQueue(times: 2);
    }

    expect(witness.updates, 600);
    expect(witness.lates, 600);
    expect(witness.fixed, inInclusiveRange(495, 505)); // 50 a second
    expect(witness.waits, inInclusiveRange(80, 100)); // a 0.1 s wait takes 6 or 7 frames
    expect(errors, {
      'onEnable',
      'onStart',
      'onFixedUpdate',
      'onUpdate',
      'onLateUpdate',
      'onDisable',
      'signal',
      'routine',
      'tween',
      'onClick',
      'onDragStart',
      'onPointerDown',
      'onPointerUp',
      'behaviour onEnable',
      'behaviour onStart',
      'behaviour onFixedUpdate',
      'behaviour onUpdate',
      'behaviour onLateUpdate',
      'behaviour onDisable',
      'onLoad',
      'onDestroy',
      'system beforeFrame',
      'system beforeUpdate',
      'system afterLateUpdate',
      'system afterFrame',
      'system turned off',
    });
    expect(world.maybeSystem<ThrowingSystem>()!.isTurnedOff, isTrue, reason: 'after five frames of errors');
    // ignore: avoid_print
    print('$count errors reported over 600 frames');
    thrower.destroy();
    world.update(1 / 60);
    await pumpEventQueue();
    expect(errors, contains('behaviour onDestroy'));
  });

  test('a throwing listener of the live camera or of the pause goes to errors, and the frame ends', () async {
    final world = World();
    addTearDown(world.dispose);
    final errors = <Object>[];
    world.errors.listen((e) => errors.add(e.error));
    final ends = world.system(FrameEnds.new);
    world.add(GameObject(components: [Camera()]));
    world.cameras.onLiveChanged.listen((_) => throw StateError('live'));
    world.onPausedChanged.listen((_) => throw StateError('paused'));
    world.add(GameObject(components: [VirtualCamera()]));
    expect(() => world.update(1 / 60), returnsNormally);
    expect(ends.ends, 1, reason: 'afterFrame still ran');
    expect(() => world.paused = true, returnsNormally);
    expect(world.paused, isTrue);
    await pumpEventQueue();
    expect(errors.map((e) => (e as StateError).message), ['live', 'paused']);
  });
}
