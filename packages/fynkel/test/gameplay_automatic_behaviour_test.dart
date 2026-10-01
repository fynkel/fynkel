import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class _CallbackScript extends Behaviour {
  void Function()? create, enable, begin, fixed, after, frame, late, disable, cleanup, trigger;
  @override
  void onTriggerEnter(Contact contact) => trigger?.call();
  @override
  void onCreate() => create?.call();
  @override
  void onEnable() => enable?.call();
  @override
  void start() => begin?.call();
  @override
  void fixedUpdate(FixedUpdateContext context) => fixed?.call();
  @override
  bool get wantsAfterPhysics => true;
  @override
  void afterPhysics(AfterPhysicsContext context) => after?.call();
  @override
  void update(UpdateContext context) => frame?.call();
  @override
  void lateUpdate(UpdateContext context) => late?.call();
  @override
  void onDisable() => disable?.call();
  @override
  void onDestroy() => cleanup?.call();
}

void main() {
  late World world;
  late GameObject target;
  late _CallbackScript script;
  late List<BehaviourError> errors;
  setUp(() {
    world = World.create();
    errors = [];
    world.onBehaviourError = errors.add;
    target = world.edit(() => world.spawn(name: 'initial')..set(const Layer()));
    script = world.edit(() => world.spawn().addBehaviour(_CallbackScript()));
  });
  tearDown(() => world.dispose());

  test('each lifecycle and phase callback commits independently and rolls back before reporting', () {
    final phases = <String>[];
    void fail(String phase) {
      phases.add(phase);
      target.name = phase;
      target.set(const Layer(layer: 3));
      throw StateError(phase);
    }

    world.onBehaviourError = (error) {
      expect(target.name, 'initial');
      expect(target.get<Layer>()!.layer, 0);
      errors.add(error);
    };
    script.create = () => fail('create');
    script.enable = () => fail('enable');
    script.begin = () => fail('start');
    script.fixed = () => fail('fixed');
    script.after = () => fail('after');
    script.frame = () => fail('update');
    script.late = () => fail('late');
    script.disable = () => fail('disable');
    script.cleanup = () => fail('destroy');
    world.frame(1 / 60);
    world.frame(1 / 60);
    script.destroy();
    world.frame(0);
    expect(
      phases,
      containsAll(['create', 'enable', 'start', 'fixed', 'after', 'update', 'late', 'disable', 'destroy']),
    );
    expect(errors.length, phases.length);
    expect(target.name, 'initial');
  });

  test('a later callback sees a prior commit while failed writes and attachments disappear', () {
    script.fixed = () {
      target.name = 'fixed';
      expect(world.find('fixed'), same(target));
    };
    script.after = () {
      expect(target.name, 'fixed');
      target.name = 'discard';
      target.addBehaviour(_CallbackScript());
      world.spawn(name: 'discard-spawn');
      throw StateError('after');
    };
    script.frame = () {
      expect(target.name, 'fixed');
      expect(target.behaviours, isEmpty);
      expect(world.find('discard-spawn'), isNull);
      world.edit(() => target.name = 'update');
    };
    script.late = () => expect(target.name, 'update');
    world.frame(1 / 60);
    expect(errors.single.callback, 'afterPhysics');
    expect(target.name, 'update');
  });

  test('coroutine advancement and wait predicates roll back independently and terminate on failure', () {
    world.frame(0);
    Iterable<Object?> advances() sync* {
      target.name = 'first';
      yield null;
      target.name = 'discard';
      throw StateError('advance');
    }

    final routine = script.startCoroutine(advances());
    expect(target.name, 'initial');
    world.frame(0);
    expect(target.name, 'first');
    world.frame(0);
    expect(target.name, 'first');
    expect(routine.isRunning, isFalse);
    expect(errors.single.callback, 'coroutine');
    Iterable<Object?> waits() sync* {
      yield WaitUntil(() {
        target.name = 'predicate';
        throw StateError('predicate');
      });
      target.name = 'unreachable';
    }

    final waiting = script.startCoroutine(waits());
    world.frame(0);
    expect(waiting.isRunning, isTrue);
    world.frame(0);
    expect(waiting.isRunning, isFalse);
    expect(target.name, 'first');
    expect(errors.length, 2);
    world.frame(0);
    expect(errors.length, 2);
  });

  test('contact failure rolls back before the next behaviour receives the same event', () {
    var observed = false;
    world.edit(() {
      final sensor = world.spawn()..set(const Collider(isSensor: true));
      sensor.addBehaviour(
        _CallbackScript()
          ..trigger = () {
            target.name = 'discard';
            throw StateError('trigger');
          },
      );
      sensor.addBehaviour(
        _CallbackScript()
          ..trigger = () {
            expect(target.name, 'initial');
            target.name = 'contact';
            observed = true;
          },
      );
      world.spawn()
        ..set(const Collider())
        ..set(const RigidBody(bodyType: BodyType.dynamic, gravityScale: 0));
    });
    world.frame(1 / 60);
    expect(errors.single.callback, 'onTriggerEnter');
    expect(observed, isTrue);
    expect(target.name, 'contact');
  });

  test('failed queue adoption is one host error and retries lifecycle and routines next frame', () {
    world.throwOnDiagnostics = false;
    final diagnostics = <FynkelDiagnostic>[];
    world.onDiagnostics = diagnostics.addAll;
    final calls = <String>[];
    script.create = () => calls.add('create');
    script.enable = () => calls.add('enable');
    script.begin = () => calls.add('start');
    script.frame = () => calls.add('update');
    Iterable<Object?> work() sync* {
      yield null;
      calls.add('routine');
    }

    final routine = script.startCoroutine(work());
    target.name = 'queued';
    world.native.submit(
      CommandLane.simulation,
      (CommandBatch()..setTransformOf(target.entity, const Pose(position: Vec3(double.nan, 0, 0)))).encoded(),
    );
    expect(() => world.frame(1 / 60, update: () => calls.add('host')), throwsStateError);
    expect(errors, isEmpty, reason: 'a pre-entry failure belongs to the host boundary');
    expect(calls, isEmpty, reason: 'none of the skipped callbacks consumed lifecycle state');
    expect(routine.isRunning, isTrue);
    expect(diagnostics.single.code, DiagnosticCode.invalidPayload);
    expect(target.name, 'queued', reason: 'the rejected adoption left older work for the normal tick');
    world.frame(1 / 60);
    expect(calls, ['create', 'enable', 'start', 'update']);
    expect(routine.isRunning, isTrue);
    world.frame(0);
    expect(calls, ['create', 'enable', 'start', 'update', 'update', 'routine']);
    expect(routine.isRunning, isFalse);
    expect(errors, isEmpty);
    world.frame(1 / 60);
    expect(calls.where((call) => call == 'create'), hasLength(1));
  });

  test('invoke callback failure rolls back without cancelling another invoke', () {
    world.frame(0);
    script.invoke(() {
      target.name = 'discard';
      throw StateError('invoke');
    }, 0);
    script.invoke(() {
      expect(target.name, 'initial');
      target.name = 'survivor';
    }, 0);
    world.frame(0);
    expect(errors.single.callback, 'invoke');
    expect(target.name, 'survivor');
  });
}
