import 'package:fynkel/src/native/bridge.dart' show NativeGameplayRef;
import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class _Created extends Behaviour {
  int creates = 0;
  @override
  void onCreate() => creates++;
}

class _Retired extends Behaviour {
  final calls = <String>[];
  @override
  void onDisable() => calls.add('disable');
  @override
  void onDestroy() => calls.add('destroy');
}

void main() {
  late World w;
  setUp(() => w = World.create());
  tearDown(() => w.dispose());

  test('transactions resolve canonical handles, references and lifecycle without a tick', () {
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

  test('callback abort does not undo the preceding commit', () {
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

  test('native rename and destruction reconcile even when the following callback aborts', () {
    final renamed = w.spawn(name: 'before');
    final destroyed = w.spawn(name: 'removed');
    final retired = destroyed.addBehaviour(_Retired());
    w.frame(0);
    w.native.editGameplay((edit) {
      edit.setName(NativeGameplayRef.live(renamed.entity), 'native');
      edit.destroy(NativeGameplayRef.live(destroyed.entity));
    });
    expect(
      () => w.edit(() {
        expect(renamed.name, 'native');
        expect(destroyed.isAlive, isFalse);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(renamed.name, 'native');
    expect(destroyed.isAlive, isFalse);
    expect(w.tick, 0);
    expect(w.drainEvents().whereType<EntityDestroyed>(), hasLength(1));
    w.frame(0);
    expect(w.objects, [renamed]);
    expect(retired.calls, ['disable', 'destroy']);
    w.frame(0);
    expect(retired.calls, ['disable', 'destroy']);
  });

  test('canceled pending spawns and committed destruction retire handles once', () {
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
