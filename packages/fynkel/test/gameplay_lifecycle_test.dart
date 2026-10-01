import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class _Script extends Behaviour {
  _Script(this.log, this.tag);
  final List<String> log;
  final String tag;
  bool failDisable = false;
  @override
  void onCreate() => log.add('$tag.create');
  @override
  void onEnable() => log.add('$tag.enable');
  @override
  void start() => log.add('$tag.start');
  @override
  void onDisable() {
    log.add('$tag.disable');
    if (failDisable) throw StateError('disable callback');
  }

  @override
  void onDestroy() => log.add('$tag.destroy');
}

void main() {
  late World world;
  late List<String> log;
  setUp(() {
    world = World.create();
    log = [];
  });
  tearDown(() => world.dispose());

  _Script live() {
    final b = world.edit(() => world.spawn(name: 'owner').addBehaviour(_Script(log, 'old')));
    world.frame(0);
    log.clear();
    return b;
  }

  void absent(GameObject owner) {
    expect(owner.behaviours, isEmpty);
    expect(owner.behaviour<_Script>(), isNull);
    expect(owner.behavioursOf<_Script>(), isEmpty);
    expect(owner.getComponent<_Script>(), isNull);
    expect(owner.getInChildren<_Script>(), isNull);
    expect(owner.getAllInChildren<_Script>(), isEmpty);
    expect(world.behaviours, isEmpty);
    expect(world.findObjectsOfType<_Script>(includeInactive: true), isEmpty);
  }

  test('enabled reads staged intent while callbacks wait, and toggles coalesce', () {
    final b = live();
    expect(
      () => world.edit(() {
        b.enabled = false;
        expect(b.enabled, isFalse);
        expect(b.isActive, isTrue);
        expect(log, isEmpty);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(b.enabled, isTrue);
    world.frame(0);
    expect(log, isEmpty);
    world.edit(() {
      b.enabled = false;
      b.enabled = true;
    });
    world.frame(0);
    expect(log, isEmpty);
    world.edit(() => b.enabled = false);
    expect(b.enabled, isFalse);
    expect(b.isActive, isTrue);
    world.frame(0);
    expect(log, ['old.disable']);
    expect(b.isActive, isFalse);
    world.frame(0);
    expect(log, ['old.disable']);
    world.edit(() => b.enabled = true);
    world.frame(0);
    expect(log, ['old.disable', 'old.enable']);
  });

  test('transactional removal is immediately absent but cleanup runs once after commit', () {
    final b = live(), owner = world.find('owner')!;
    world.edit(() {
      b.destroy();
      b.destroy();
      expect(b.isAlive, isFalse);
      expect(b.isActive, isTrue);
      expect(b.isDestroyed, isFalse);
      expect(b.gameObject, owner, reason: 'cleanup retains the owning object');
      absent(owner);
      expect(log, isEmpty);
    });
    absent(owner);
    expect(b.isAlive, isFalse);
    expect(b.isDestroyed, isFalse);
    expect(owner.isAlive, isTrue);
    world.edit(b.destroy);
    world.frame(0);
    expect(log, ['old.disable', 'old.destroy']);
    expect(b.isDestroyed, isTrue);
    expect(b.isActive, isFalse);
    world.edit(b.destroy);
    world.frame(0);
    expect(log, ['old.disable', 'old.destroy']);
  });

  test('final native validation failure rolls back removal and enabled intent together', () {
    final b = live(), owner = world.find('owner')!;
    final other = owner.addBehaviour(_Script(log, 'other'));
    world.frame(0);
    log.clear();
    expect(
      () => world.edit(() {
        b.destroy();
        other.enabled = false;
        owner.name = 'discarded';
        final target = world.spawn();
        owner.set(Follow(target: target));
        target.destroy();
      }),
      throwsStateError,
    );
    expect(owner.name, 'owner');
    expect(owner.behaviours, [b, other]);
    expect(b.isAlive, isTrue);
    expect(other.enabled, isTrue);
    expect(owner.has<Follow>(), isFalse);
    world.frame(0);
    expect(log, isEmpty);
  });

  test('new disabled scripts create without enabling and can activate in a later edit', () {
    final b = world.edit(() {
      final b = world.spawn().addBehaviour(_Script(log, 'new'));
      b.enabled = false;
      expect(b.enabled, isFalse);
      return b;
    });
    expect(b.enabled, isFalse);
    expect(log, isEmpty);
    world.frame(0);
    expect(log, ['new.create']);
    expect(b.hasStarted, isFalse);
    world.edit(() => b.enabled = true);
    world.frame(0);
    expect(log, ['new.create', 'new.enable', 'new.start']);
  });

  test('attach then remove cancels publication and preserves the reusable instance', () {
    final b = _Script(log, 'cancelled');
    final owner = world.edit(() {
      final owner = world.spawn();
      owner.addBehaviour(b);
      b.enabled = false;
      b.destroy();
      expect(b.isAlive, isFalse);
      absent(owner);
      return owner;
    });
    absent(owner);
    expect(() => b.gameObject, throwsStateError);
    expect(b.enabled, isTrue, reason: 'canceled attachment does not publish its enabled edit');
    world.frame(0);
    expect(log, isEmpty);
    world.edit(() => owner.addBehaviour(b));
    world.frame(0);
    expect(log, ['cancelled.create', 'cancelled.enable', 'cancelled.start']);
  });

  test('removing and replacing a script publishes one new lookup without inline callbacks', () {
    final old = live(), owner = world.find('owner')!;
    final replacement = _Script(log, 'new');
    world.edit(() {
      old.destroy();
      owner.addBehaviour(replacement);
      expect(owner.behaviour<_Script>(), replacement);
      expect(world.findObjectsOfType<_Script>(), [replacement]);
    });
    expect(owner.behaviours, [replacement]);
    expect(log, isEmpty);
    world.frame(0);
    expect(log, ['old.disable', 'old.destroy', 'new.create', 'new.enable', 'new.start']);
  });

  test('writes after staged removal fail stickily instead of resurrecting a script', () {
    final b = live(), owner = world.find('owner')!;
    expect(
      () => world.edit(() {
        owner.name = 'discarded';
        b.destroy();
        try {
          b.enabled = true;
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(b.isAlive, isTrue);
    expect(owner.name, 'owner');
    expect(owner.behaviour<_Script>(), b);
    world.edit(b.destroy);
    expect(() => b.enabled = true, throwsStateError);
    expect(
      () => world.edit(() {
        owner.name = 'also discarded';
        try {
          b.enabled = false;
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(owner.name, 'owner');
  });

  test('removal retains the last enabled request for existing-script cleanup', () {
    final b = live();
    world.edit(() {
      b.enabled = false;
      b.destroy();
      expect(b.enabled, isFalse);
    });
    expect(b.enabled, isFalse);
    expect(b.isAlive, isFalse);
    world.frame(0);
    expect(log, ['old.disable', 'old.destroy']);
  });

  test('cross-world lifecycle edits cannot affect either scheduler', () {
    final b = live();
    final other = World.create();
    addTearDown(other.dispose);
    final foreign = other.edit(() => other.spawn().addBehaviour(_Script(log, 'foreign')));
    for (final operation in <void Function()>[() => foreign.enabled = false, foreign.destroy]) {
      expect(
        () => world.edit(() {
          b.enabled = false;
          try {
            operation();
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(b.enabled, isTrue);
      expect(foreign.enabled, isTrue);
      expect(foreign.isAlive, isTrue);
    }
  });

  test('rejected async lifecycle edits roll back and later continuations stay expired', () async {
    final b = live();
    var resumed = false;
    expect(
      () => world.edit(() async {
        b.enabled = false;
        b.destroy();
        await Future<void>.value();
        resumed = true;
        b.enabled = false;
      }),
      throwsStateError,
    );
    await Future<void>.delayed(Duration.zero);
    expect(resumed, isTrue);
    expect(b.enabled, isTrue);
    expect(b.isAlive, isTrue);
    world.frame(0);
    expect(log, isEmpty);
  });

  test('cleanup callback exceptions do not undo committed data or skip destruction', () {
    final b = live(), owner = world.find('owner')!;
    final errors = <BehaviourError>[];
    world.onBehaviourError = errors.add;
    b.failDisable = true;
    world.edit(() {
      owner.set(const Layer(layer: 7));
      b.destroy();
    });
    world.frame(0);
    expect(errors, hasLength(1));
    expect(log, ['old.disable', 'old.destroy']);
    expect(b.isDestroyed, isTrue);
    expect(owner.get<Layer>()!.layer, 7);
    absent(owner);
    world.frame(0);
    expect(errors, hasLength(1));
  });
}
