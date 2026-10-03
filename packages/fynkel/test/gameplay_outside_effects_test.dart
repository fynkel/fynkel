import 'package:fynkel/gameplay.dart';
import 'package:test/test.dart';

class _Lifecycle extends Behaviour {
  final calls = <String>[];
  @override
  void onCreate() => calls.add('create');
  @override
  void onDestroy() => calls.add('destroy');
}

void main() {
  late World world;
  setUp(() => world = World.create()..throwOnDiagnostics = true);
  tearDown(() => world.dispose());

  test('standalone effects validate immediately and execute once at the next tick', () {
    final go = world.spawn(name: 'body')
      ..set(const RigidBody(bodyType: BodyType.dynamic, mass: 2, gravityScale: 0, linearDamping: 0))
      ..set(const Collider());
    expect(go.isPending, isFalse);
    expect(() => go.applyForce(const Vec3(double.nan, 0, 0)), throwsStateError);
    expect(() => go.move(Vec3.one), throwsStateError);
    go.setLinearVelocity(const Vec3(3, 0, 0));
    go.applyImpulse(const Vec3(4, 0, 0));
    world.frame(0);
    expect(go.bodyState(), isNull);
    world.frame(1 / 60);
    expect(go.bodyState()!.linearVelocity.x, closeTo(5, 1e-5));
    world.frame(1 / 60);
    expect(go.bodyState()!.linearVelocity.x, closeTo(5, 1e-5));
  });

  test('standalone audio and animation share immediate missing-component validation', () {
    final go = world.spawn();
    expect(() => go.audio.play(), throwsStateError);
    expect(() => go.animation.pause(), throwsStateError);
    expect(() => world.audio.play(AssetRef.none, volume: double.nan), throwsStateError);
    world.frame(1 / 60);
  });

  test('standalone membership commits immediately and lifecycle waits for safe boundary', () {
    final go = world.spawn(name: 'owner');
    final script = go.addBehaviour(_Lifecycle());
    expect(go.behaviour<_Lifecycle>(), same(script));
    expect(script.calls, isEmpty);
    world.frame(0);
    expect(script.calls, ['create']);
    script.destroy();
    expect(go.behaviour<_Lifecycle>(), isNull);
    expect(script.calls, ['create']);
    world.frame(0);
    expect(script.calls, ['create', 'destroy']);
    go.destroy();
    expect(go.isAlive, isFalse);
    expect(world.find('owner'), isNull);
  });

  test('standalone and explicit coroutine starts both wait for the scheduler', () {
    final script = world.spawn().addBehaviour(_Lifecycle());
    world.frame(0);
    final calls = <String>[];
    Iterable<Object?> routine(String name) sync* {
      calls.add(name);
      yield null;
    }

    final outside = script.startCoroutine(routine('outside'));
    world.edit(() => script.startCoroutine(routine('inside')));
    final cancelled = script.startCoroutine(routine('cancelled'));
    cancelled.stop();
    expect(calls, isEmpty);
    world.frame(0);
    expect(calls, ['outside', 'inside']);
    expect(outside.isRunning, isTrue);
    world.frame(0);
    expect(outside.isRunning, isFalse);
  });
}
