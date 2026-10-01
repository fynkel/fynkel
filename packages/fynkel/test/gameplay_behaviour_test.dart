import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class _Script extends Behaviour {
  _Script(this.log, this.tag, [this.priority = 0]);
  final List<String> log;
  final String tag;
  final int priority;
  bool throwOrder = false;
  int orderReads = 0;
  @override
  int get order {
    orderReads++;
    if (throwOrder) throw StateError('order');
    return priority;
  }

  @override
  void onCreate() => log.add('$tag.create');
  @override
  void onEnable() => log.add('$tag.enable');
  @override
  void start() => log.add('$tag.start');
  @override
  void update(UpdateContext context) => log.add('$tag.update');
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

  test('staged attachments share ordinary lookups and publish before lifecycle runs', () {
    final script = _Script(log, 'new');
    final go = world.edit(() {
      final go = world.spawn(name: 'owner');
      expect(go.addBehaviour(script), same(script));
      expect(script.gameObject, same(go));
      expect(script.world, same(world));
      expect(script.isAlive, isTrue);
      expect(script.isCreated, isFalse);
      expect(go.behaviour<_Script>(), same(script));
      expect(go.behavioursOf<_Script>(), [script]);
      expect(go.getComponent<_Script>(), same(script));
      expect(go.getInChildren<_Script>(), same(script));
      expect(world.behaviours, [script]);
      expect(world.findObjectsOfType<_Script>(), [script]);
      script.set(const Layer(layer: 3));
      expect(go.get<Layer>()!.layer, 3);
      expect(log, isEmpty);
      return go;
    });
    expect(script.gameObject, same(go));
    expect(script.orderReads, 1, reason: 'no order getter runs during publication');
    expect(go.behaviours, [script]);
    expect(world.behaviours, [script]);
    expect(log, isEmpty);
    world.frame(0);
    expect(log, ['new.create', 'new.enable', 'new.start', 'new.update']);
    log.clear();
    world.frame(0);
    expect(log, ['new.update']);
  });

  test('abort and final native validation leave scripts unbound and reusable', () {
    final owner = world.edit(() => world.spawn(name: 'owner'));
    for (final finalFailure in [false, true]) {
      final script = _Script(log, 'retry');
      expect(
        () => world.edit(() {
          owner.addBehaviour(script);
          if (finalFailure) {
            final target = world.spawn();
            owner.set(Follow(target: target));
            target.destroy();
          } else {
            throw StateError('abort');
          }
        }),
        throwsStateError,
      );
      expect(script.isAlive, isFalse);
      expect(() => script.gameObject, throwsStateError);
      expect(owner.behaviours, isEmpty);
      expect(world.behaviours, isEmpty);
      expect(owner.has<Follow>(), isFalse);
      world.edit(() => owner.addBehaviour(script));
      expect(script.gameObject, owner);
      // Remove normally at the existing lifecycle boundary before the next case.
      script.destroy();
      world.frame(0);
      expect(log, isEmpty);
    }
  });

  test('duplicate or foreign attachment caught by user still aborts the whole edit', () {
    final other = World.create();
    addTearDown(other.dispose);
    final foreign = other.edit(() => other.spawn());
    final owned = foreign.addBehaviour(_Script(log, 'foreign'));
    final owner = world.edit(() => world.spawn(name: 'owner'));
    for (final existing in [false, true]) {
      final script = existing ? owned : _Script(log, 'duplicate');
      expect(
        () => world.edit(() {
          owner.name = 'discarded';
          try {
            owner.addBehaviour(script);
            if (!existing) owner.addBehaviour(script);
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(owner.name, 'owner');
      expect(owner.behaviours, isEmpty);
    }
    expect(owned.gameObject, foreign);
  });

  test('destroyed staged owners cancel attachments without creation or destruction callbacks', () {
    final script = _Script(log, 'cancelled');
    world.edit(() {
      final root = world.spawn();
      world.spawn(parent: root).addBehaviour(script);
      root.destroy();
      expect(script.isAlive, isFalse);
      expect(world.behaviours, isEmpty);
      expect(world.findObjectsOfType<_Script>(includeInactive: true), isEmpty);
    });
    expect(world.behaviours, isEmpty);
    expect(() => script.gameObject, throwsStateError);
    world.frame(0);
    expect(log, isEmpty);
    final owner = world.edit(() => world.spawn());
    world.edit(() => owner.addBehaviour(script));
    expect(script.gameObject, owner);
  });

  test('callback ordering combines committed and staged attachments with stable ties', () {
    final go = world.edit(() => world.spawn());
    final old = go.addBehaviour(_Script(log, 'old', 5));
    final late = _Script(log, 'late', 5), early = _Script(log, 'early', -1);
    world.edit(() {
      go.addBehaviour(late);
      go.addBehaviour(early);
      expect(go.behaviours, [old, late, early], reason: 'object lookup keeps attach order');
      expect(world.behaviours, [early, old, late], reason: 'scheduler uses callback order');
    });
    expect(world.behaviours, [early, old, late]);
    world.frame(0);
    expect(log.where((e) => e.endsWith('.create')), ['early.create', 'old.create', 'late.create']);
    expect(log.where((e) => e.endsWith('.update')), ['early.update', 'old.update', 'late.update']);
  });

  test('order getter failures abort before native publication, including during preparation', () {
    final owner = world.edit(() => world.spawn(name: 'owner'));
    final old = owner.addBehaviour(_Script(log, 'old'));
    for (final existingThrows in [false, true]) {
      final added = _Script(log, 'new')..throwOrder = !existingThrows;
      old.throwOrder = existingThrows;
      expect(
        () => world.edit(() {
          owner.name = 'discarded';
          try {
            owner.addBehaviour(added);
          } catch (_) {}
        }),
        throwsStateError,
      );
      old.throwOrder = false;
      expect(owner.name, 'owner');
      expect(owner.behaviours, [old]);
      expect(() => added.gameObject, throwsStateError);
      expect(log, isEmpty);
    }
  });

  test('inactive owners defer creation until activation and subscribe surviving scripted objects', () {
    final script = _Script(log, 'inactive');
    final go = world.edit(() {
      final go = world.spawn();
      go.setActive(false);
      go.addBehaviour(script);
      expect(world.findObjectsOfType<_Script>(), isEmpty);
      expect(world.findObjectsOfType<_Script>(includeInactive: true), [script]);
      return go;
    });
    world.frame(0);
    expect(log, isEmpty);
    world.edit(() => go.setActive(true));
    world.frame(0);
    expect(log.take(3), ['inactive.create', 'inactive.enable', 'inactive.start']);
    world.step();
    world.drainEvents();
    expect(world.native.snapshot.recordCount, 1);
  });

  test('registered factories attach transactionally and caught factory errors are sticky', () {
    world.registry.register(BehaviourType(id: 'test.script', create: () => _Script(log, 'factory')));
    world.registry.register(BehaviourType(id: 'test.fail', create: () => throw StateError('factory')));
    final go = world.edit(() => world.spawn(name: 'owner'));
    final script = world.edit(() => go.addBehaviourFromData('test.script'));
    expect(go.behaviour<_Script>(), same(script));
    expect(script.behaviourType!.id, 'test.script');
    expect(
      () => world.edit(() {
        go.name = 'discarded';
        try {
          go.addBehaviourFromData('test.fail');
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(go.name, 'owner');
    expect(go.behaviours, [script]);
  });

  test('invokes on staged scripts wait until after commit and the next scheduler phase', () {
    final script = _Script(log, 'staged');
    var invoked = false;
    world.edit(() {
      world.spawn().addBehaviour(script);
      script.invoke(() => invoked = true, 0);
      expect(script.isInvoking, isTrue);
      expect(invoked, isFalse);
    });
    expect(invoked, isFalse);
    world.frame(0);
    expect(invoked, isTrue);
    expect(script.isInvoking, isFalse);
  });

  test('rejected async attachment expires its binding without leaking to the scheduler', () async {
    final script = _Script(log, 'async');
    final owner = world.edit(() => world.spawn());
    var resumed = false;
    expect(
      () => world.edit(() async {
        owner.addBehaviour(script);
        await Future<void>.value();
        resumed = true;
        script.set(const Layer(layer: 7));
      }),
      throwsStateError,
    );
    await Future<void>.delayed(Duration.zero);
    expect(resumed, isTrue);
    expect(world.behaviours, isEmpty);
    expect(owner.has<Layer>(), isFalse);
    expect(() => script.gameObject, throwsStateError);
    world.edit(() => owner.addBehaviour(script));
    expect(script.gameObject, owner);
  });

  test('a prior committed destruction does not poison behaviour queries in a later edit', () {
    final go = world.edit(() => world.spawn());
    go.addBehaviour(_Script(log, 'old'));
    world.edit(go.destroy);
    world.edit(() => expect(world.behaviours, isEmpty));
    world.frame(0);
    expect(log, isEmpty);
  });
}
