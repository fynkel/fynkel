import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class _Created extends Behaviour {
  int creates = 0;
  @override
  void onCreate() => creates++;
}

void main() {
  late World w;
  setUp(() => w = World.create());
  tearDown(() => w.dispose());

  test('adoption resolves canonical handles, references and lifecycle without a tick', () {
    final root = w.spawn(name: 'root');
    final child = w.spawn(name: 'child', parent: root)..set(Follow(target: root));
    final b = child.addBehaviour(_Created());
    child.localTransform;
    w.edit(() {
      expect(root.isPending, isFalse);
      expect(child.parent, same(root));
      expect(child.get<Follow>()!.target, same(root));
      expect(w.byEntity(child.entity), same(child));
      expect(b.creates, 0);
      child.name = 'edited';
    });
    expect(w.tick, 0);
    expect(w.find('edited'), same(child));
    w.frame(1 / 60);
    expect(b.creates, 1);
    expect(w.objects.length, 2);
    w.frame(1 / 60);
    expect(b.creates, 1);
  });

  test('callback abort does not undo the preceding adoption commit', () {
    final go = w.spawn(name: 'queued');
    expect(
      () => w.edit(() {
        expect(go.isPending, isFalse);
        go.name = 'discard';
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(go.isAlive, isTrue);
    expect(go.name, 'queued');
    w.edit(() => expect(w.find('queued'), same(go)));
    w.frame(1 / 60);
    expect(w.objects.length, 1);
  });

  test('failed adoption retains native queue ownership across retries', () {
    w.throwOnDiagnostics = false;
    final go = w.spawn(name: 'queued');
    w.native.submit(
      CommandLane.simulation,
      (CommandBatch()..setTransformOf(go.entity, const Pose(position: Vec3(double.nan, 0, 0)))).encoded(),
    );
    var entered = false;
    for (var i = 0; i < 2; i++) {
      expect(() => w.edit(() => entered = true), throwsStateError);
      expect(go.isPending, isFalse);
    }
    expect(entered, isFalse);
    w.step();
    w.drainEvents();
    expect(go.isPending, isFalse);
    expect(w.objects.length, 1);
    w.edit(() => go.name = 'recovered');
    expect(go.name, 'recovered');
  });

  test('canceled pending spawns and adopted destruction retire handles once', () {
    final survivor = w.edit(() => w.spawn(name: 'survivor'));
    final canceled = w.spawn(name: 'canceled');
    canceled.destroy();
    survivor.destroy();
    w.edit(() {
      expect(canceled.isAlive, isFalse);
      expect(survivor.isAlive, isFalse);
      expect(w.objects, isEmpty);
    });
    w.frame(1 / 60);
    expect(w.objects, isEmpty);
  });

  test('immediate spawns keep canonical identity across a manual step without a drain', () {
    final go = w.spawn(name: 'stepped');
    w.step();
    var entered = false;
    w.edit(() => entered = true);
    expect(entered, isTrue);
    w.drainEvents();
    w.edit(() => expect(w.find('stepped'), same(go)));
    expect(w.objects.length, 1);
  });

  test('adopted effects precede callback effects and execute only once', () {
    final go = w.spawn()
      ..set(const RigidBody(bodyType: BodyType.dynamic, mass: 2, gravityScale: 0, linearDamping: 0))
      ..set(const Collider());
    go.setLinearVelocity(const Vec3(3, 0, 0));
    w.edit(() {
      expect(go.bodyState(), isNull);
      go.applyImpulse(const Vec3(4, 0, 0));
    });
    w.frame(1 / 60);
    expect(go.bodyState()!.linearVelocity.x, closeTo(5, 1e-5));
    w.frame(1 / 60);
    expect(go.bodyState()!.linearVelocity.x, closeTo(5, 1e-5));
  });
}
