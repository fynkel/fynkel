import 'package:fynkel/src/native/bridge.dart' show NativeGameplayRef;
import 'package:fynkel/gameplay.dart';
import 'package:test/test.dart';

class _Created extends Behaviour {
  int creates = 0;
  @override
  void onCreate() => creates++;
}

class _Activity extends Behaviour {
  final calls = <String>[];
  @override
  void onEnable() => calls.add('enable');
  @override
  void onDisable() => calls.add('disable');
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

  test('unchanged lifecycle has bounded bridge work with hundreds of disabled scripts', () {
    int calls(int count) {
      final world = World.create();
      try {
        world.edit(() {
          for (var i = 0; i < count; i++) {
            world.spawn(name: 'idle$i').addBehaviour(_Retired()).enabled = false;
          }
        });
        world.frame(0);
        world.frame(0);
        final before = world.native.ffiCalls;
        world.frame(0);
        return world.native.ffiCalls - before;
      } finally {
        world.dispose();
      }
    }

    final small = calls(30);
    final large = calls(300);
    expect(large, lessThanOrEqualTo(small + 10), reason: 'unchanged activity must not query every script');
  });

  test('unchanged names skip snapshots while native activity still settles', () {
    final go = w.spawn(name: 'stable');
    final behaviour = go.addBehaviour(_Retired());
    w.frame(0);
    w.edit(() {}); // Catch up the SDK cursor after the spawn.
    final snapshots = w.native.gameplayStateSnapshots;
    final bytes = w.native.gameplayStateBytes;
    for (var i = 0; i < 32; i++) {
      w.edit(() => go.transform.position = Vec3(i.toDouble(), 0, 0));
    }
    expect(w.native.gameplayStateSnapshots, snapshots);
    expect(w.native.gameplayStateBytes - bytes, 32 * 12);
    w.frame(1 / 60);
    expect(w.native.gameplayStateSnapshots, snapshots);
    w.native.editGameplay((edit) => edit.setActive(NativeGameplayRef.live(go.entity), false));
    w.frame(0);
    expect(behaviour.calls, ['disable']);
    expect(w.native.gameplayStateSnapshots, snapshots);
  });

  test('native reparent and parent activity settle child callbacks exactly once', () {
    final parent = w.spawn(name: 'parent');
    final child = w.spawn(name: 'child');
    final behaviour = child.addBehaviour(_Activity());
    w.frame(0);
    expect(behaviour.calls, ['enable']);
    w.native.editGameplay((edit) {
      edit.setActive(NativeGameplayRef.live(parent.entity), false);
      edit.setParent(NativeGameplayRef.live(child.entity), NativeGameplayRef.live(parent.entity));
    });
    w.frame(0);
    w.frame(0);
    expect(behaviour.calls, ['enable', 'disable']);
    w.native.editGameplay((edit) => edit.setActive(NativeGameplayRef.live(parent.entity), true));
    w.frame(0);
    w.frame(0);
    expect(behaviour.calls, ['enable', 'disable', 'enable']);
  });

  test('independent state readers cannot consume SDK rename or destruction changes', () {
    final go = w.spawn(name: 'before');
    w.edit(() {});
    final observer = w.native.gameplayStateSince(0);
    expect(observer.live![go.entity], 'before');
    expect(w.native.gameplayStateSince(observer.revision).live, isNull);
    w.native.editGameplay((edit) => edit.setName(NativeGameplayRef.live(go.entity), 'after'));
    final renamed = w.native.gameplayStateSince(observer.revision);
    expect(renamed.live![go.entity], 'after');
    expect(w.native.gameplayState()[go.entity], 'after');
    expect(
      () => w.edit(() {
        expect(go.name, 'after');
        throw StateError('callback abort');
      }),
      throwsStateError,
    );
    final snapshots = w.native.gameplayStateSnapshots;
    w.edit(() => expect(go.name, 'after'));
    expect(w.native.gameplayStateSnapshots, snapshots);
    w.native.editGameplay((edit) => edit.destroy(NativeGameplayRef.live(go.entity)));
    expect(w.native.gameplayStateSince(renamed.revision).live, isEmpty);
    w.edit(() => expect(go.isAlive, isFalse));
    expect(w.objects, isEmpty);
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
