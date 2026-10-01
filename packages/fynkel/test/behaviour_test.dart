// The Behaviour lifecycle (§3.2) as the world drives it at the §5.1 points:
// order of callbacks, start once, disabled = silent, deferred destruction,
// deterministic ordering, services through the current GameContext (with a
// test context installed), the registry building a behaviour from authored
// data, and a behaviour that really moves an entity.

import 'dart:io';
import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:fynkel/generated/schema.g.dart' show SpawnCommand;
import 'package:test/test.dart';

/// Records every callback with the tick it ran at.
class Recorder extends Behaviour {
  Recorder(this.log, {this.tag = '', this.priority = 0, this.afterPhysicsToo = false});

  final List<String> log;
  final String tag;
  final int priority;
  final bool afterPhysicsToo;

  @override
  int get order => priority;

  @override
  bool get wantsAfterPhysics => afterPhysicsToo;

  void _r(String what) => log.add('$tag$what');

  @override
  void onCreate() => _r('onCreate');
  @override
  void onEnable() => _r('onEnable');
  @override
  void start() => _r('start');
  @override
  void fixedUpdate(FixedUpdateContext context) => _r('fixedUpdate@${context.tick}');
  @override
  void afterPhysics(AfterPhysicsContext context) => _r('afterPhysics@${context.tick}');
  @override
  void update(UpdateContext context) => _r('update');
  @override
  void lateUpdate(UpdateContext context) => _r('lateUpdate');
  @override
  void onDisable() => _r('onDisable');
  @override
  void onDestroy() => _r('onDestroy');
}

@FynkelBehaviour(id: 'game.Spinner')
class Spinner extends Behaviour {
  @Expose(min: 0, step: 0.1)
  double speed = 1.0; // radians per second

  double angle = 0;

  @override
  void fixedUpdate(FixedUpdateContext context) {
    angle += speed * Time.fixedDelta;
    gameObject.setLocalRotationAxisAngle(0, 1, 0, angle);
  }
}

/// What the generator will emit for [Spinner] (M2-16); hand-written here.
final spinnerType = BehaviourType(
  id: 'game.Spinner',
  create: Spinner.new,
  fields: [
    BehaviourField(
      name: 'speed',
      type: 'number',
      defaultValue: 1.0,
      min: 0,
      step: 0.1,
      get: (b) => (b as Spinner).speed,
      set: (b, v) => (b as Spinner).speed = (v as num).toDouble(),
    ),
  ],
);

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  World world({BehaviourRegistry? registry}) =>
      World.create(library: lib, registry: registry)..throwOnDiagnostics = true;

  test('lifecycle: pending object → create/enable at the first boundary after it spawns, start once', () {
    final w = world();
    addTearDown(w.dispose);
    final log = <String>[];
    final go = w.spawn(name: 'thing');
    final b = go.addBehaviour(Recorder(log));
    expect(b.gameObject, same(go));
    expect(b.isCreated, isFalse);
    expect(log, isEmpty, reason: 'nothing runs at attach time');

    w.frame(1 / 60);
    // The spawn resolves at the tick; the frame's update phase is the first
    // safe boundary after that.
    expect(log, ['onCreate', 'onEnable', 'start', 'fixedUpdate@0', 'update', 'lateUpdate']);
    expect(b.isCreated && b.isActive && b.hasStarted, isTrue);

    log.clear();
    w.frame(1 / 60);
    expect(log, ['fixedUpdate@1', 'update', 'lateUpdate'], reason: 'start ran once');

    // Attached to a resolved object: the fixed phase is the first boundary.
    log.clear();
    go.addBehaviour(Recorder(log, tag: 'b:'));
    w.frame(1 / 60);
    expect(
      log,
      [
        'b:onCreate',
        'b:onEnable',
        'b:start',
        'fixedUpdate@2',
        'b:fixedUpdate@2',
        'update',
        'b:update',
        'lateUpdate',
        'b:lateUpdate',
      ],
      reason: 'created before the phase; same order (0), later sequence, so it runs after the first one; its start runs before the pass (foundation 1.8)',
    );
  });

  test('disabled behaviours receive no gameplay callbacks; re-enabling does not restart', () {
    final w = world();
    addTearDown(w.dispose);
    final log = <String>[];
    final go = w.spawn();
    final b = go.addBehaviour(Recorder(log));
    w.frame(1 / 60);
    w.frame(1 / 60);
    log.clear();

    b.enabled = false;
    expect(b.isActive, isTrue, reason: 'applied at the next boundary, not now');
    w.frame(1 / 60);
    expect(log, ['onDisable']);
    expect(b.isActive, isFalse);
    log.clear();
    w.frame(1 / 60);
    expect(log, isEmpty);

    b.enabled = true;
    w.frame(1 / 60);
    expect(log, ['onEnable', 'fixedUpdate@4', 'update', 'lateUpdate'], reason: 'no second start');

    // Disabled at attach time: created, never enabled, never started.
    log.clear();
    final c = go.addBehaviour(Recorder(log, tag: 'c:')..enabled = false);
    w.frame(1 / 60);
    expect(log.where((l) => l.startsWith('c:')), ['c:onCreate']);
    expect(c.hasStarted, isFalse);
  });

  test('destruction is deferred to a safe boundary: behaviour.destroy() and gameObject.destroy()', () {
    final w = world();
    addTearDown(w.dispose);
    final log = <String>[];
    final go = w.spawn();
    final a = go.addBehaviour(Recorder(log, tag: 'a:'));
    final b = go.addBehaviour(Recorder(log, tag: 'b:'));
    w.frame(1 / 60);
    w.frame(1 / 60);
    log.clear();

    a.destroy();
    expect(a.isDestroyed, isFalse);
    expect(go.behaviours, [b], reason: 'membership commits now; callbacks wait for the boundary');
    w.frame(1 / 60);
    expect(log.where((l) => l.startsWith('a:')), ['a:onDisable', 'a:onDestroy']);
    expect(a.isDestroyed, isTrue);
    expect(go.behaviours, [b]);
    expect(w.behaviours, [b]);

    log.clear();
    go.destroy();
    w.frame(1 / 60);
    // Adoption publishes the queued destruction before lifecycle dispatch.
    expect(log, ['b:onDisable', 'b:onDestroy']);
    expect(go.isAlive, isFalse);
    expect(go.behaviours, isEmpty);
    expect(() => go.addBehaviour(Recorder(log)), throwsStateError);
    expect(() => w.spawn().addBehaviour(a), throwsStateError, reason: 'a behaviour attaches once');
  });

  test('a behaviour destroying itself inside a callback gets no further callbacks', () {
    final w = world();
    addTearDown(w.dispose);
    final log = <String>[];
    final go = w.spawn();
    late Behaviour self;
    self = go.addBehaviour(_OnFixed(log, (b) => b.destroy()));
    w.frame(1 / 60);
    expect(log, ['fixedUpdate', 'onDisable', 'onDestroy'], reason: 'no update/lateUpdate after destroy()');
    expect(self.isDestroyed, isTrue);
  });

  test('callback order is order, then attach sequence — never names or scene order', () {
    final w = world();
    addTearDown(w.dispose);
    final log = <String>[];
    final zed = w.spawn(name: 'zed');
    final alpha = w.spawn(name: 'alpha');
    zed.addBehaviour(Recorder(log, tag: 'late:', priority: 10));
    alpha.addBehaviour(Recorder(log, tag: 'second:'));
    zed.addBehaviour(Recorder(log, tag: 'third:'));
    alpha.addBehaviour(Recorder(log, tag: 'first:', priority: -1));
    w.frame(1 / 60);
    log.clear();
    w.frame(1 / 60);
    final fixed = log.where((l) => l.contains('fixedUpdate')).toList();
    expect(fixed, [
      'first:fixedUpdate@1',
      'second:fixedUpdate@1',
      'third:fixedUpdate@1',
      'late:fixedUpdate@1',
    ]);
    expect(w.behaviours.map((b) => (b as Recorder).tag), ['first:', 'second:', 'third:', 'late:']);

    // Attaching from inside a callback: created at the next boundary, so it
    // does not run in the pass that attached it.
    log.clear();
    alpha.addBehaviour(
      _OnFixed(log, (b) {
        b.gameObject.addBehaviour(Recorder(log, tag: 'nested:'));
        b.destroy();
      }),
    );
    w.frame(1 / 60);
    expect(log.indexOf('nested:onCreate'), greaterThan(log.indexOf('fixedUpdate')));
    expect(log.contains('nested:fixedUpdate@2'), isFalse);
    expect(log, contains('nested:update'));
  });

  test('afterPhysics is opt-in and runs right after the step with the completed tick', () {
    final w = world();
    addTearDown(w.dispose);
    final log = <String>[];
    final go = w.spawn();
    go.addBehaviour(Recorder(log, tag: 'in:', afterPhysicsToo: true));
    go.addBehaviour(Recorder(log, tag: 'out:'));
    w.frame(1 / 60);
    log.clear();
    w.frame(1 / 60);
    expect(log, [
      'in:fixedUpdate@1',
      'out:fixedUpdate@1',
      'in:afterPhysics@1',
      'in:update',
      'out:update',
      'in:lateUpdate',
      'out:lateUpdate',
    ]);
  });

  test('every non-physics event reaches exactly one fixedUpdate', () {
    final w = world();
    addTearDown(w.dispose);
    final seen = <List<EngineEvent>>[];
    final go = w.spawn();
    go.addBehaviour(_OnFixed(<String>[], (b) {}, onEvents: seen.add));
    w.frame(0);
    for (var temp = 1; temp <= 2; temp++) {
      w.native.submit(
        CommandLane.simulation,
        (CommandBatch()..spawn(SpawnCommand(tempId: temp, transform: identityTransform, name: 'raw$temp')))
            .encoded(),
      );
      w.frame(1 / 60);
    }
    w.frame(1 / 60);
    expect(seen.map((events) => events.whereType<EntitySpawned>().map((e) => e.tempId).toList()), [
      <int>[],
      [1],
      [2],
    ]);
  });

  test('services resolve through the current GameContext; a test context is installable', () {
    final w = world();
    addTearDown(w.dispose);
    expect(() => Time.delta, throwsStateError, reason: 'no context outside callbacks');
    expect(GameContext.current, isNull);

    final seen = <String>[];
    final go = w.spawn();
    go.addBehaviour(
      _OnFixed(<String>[], (b) {
        seen.add(
          'tick=${Time.tick} fixed=${Time.fixedDelta.toStringAsFixed(4)} delta=${Time.delta.toStringAsFixed(4)} frame=${Time.frame}',
        );
        expect(GameContext.current, same(w.context));
        expect(b.context, same(w.context));
      }),
    );
    w.frame(1 / 60);
    w.frame(1 / 30);
    expect(seen, [
      'tick=0 fixed=0.0167 delta=0.0167 frame=1',
      'tick=1 fixed=0.0167 delta=0.0167 frame=2',
      'tick=2 fixed=0.0167 delta=0.0167 frame=2',
    ], reason: 'a 1/30 s frame runs two ticks; in fixedUpdate Time.delta is the fixed step (Unity)');

    // A test context: its clock and log, its installed services.
    final ctx = TestGameContext(time: TestTime(fixedDelta: 0.5, tick: 41));
    ctx.install<_Scores>(_Scores());
    w.context = ctx;
    seen.clear();
    go.addBehaviour(
      _OnFixed(<String>[], (b) {
        Diagnostics.log('fixed=${Time.fixedDelta} tick=${Time.tick}');
        GameContext.require().service<_Scores>().total += 1;
        expect(() => GameContext.require().service<_Unused>(), throwsStateError);
      }),
    );
    w.frame(1 / 60);
    expect(ctx.diagnostics.lines, ['[game] fixed=0.5 tick=41']);
    expect(ctx.service<_Scores>().total, 1);

    // Outside the world: explicit run.
    final r = ctx.run(() => Time.elapsed);
    expect(r, 20.5);
    expect(GameContext.current, isNull);
    ctx.run(() {
      expect(GameContext.current, same(ctx));
      TestGameContext().run(() => expect(GameContext.current, isNot(same(ctx))));
      expect(GameContext.current, same(ctx));
    });
  });

  test(
    'registry: a behaviour built from authored data, with its fields set; bad data is a BehaviourDataError',
    () {
      final registry = BehaviourRegistry()..register(spinnerType);
      expect(() => registry.register(spinnerType), throwsArgumentError);
      registry.register(spinnerType, replace: true);
      expect(registry.types.map((t) => t.id), ['game.Spinner']);
      expect(registry.lookup('game.Spinner')!.field('speed')!.defaultValue, 1.0);

      final w = world(registry: registry);
      addTearDown(w.dispose);
      final go = w.spawn();
      final s = go.addBehaviourFromData('game.Spinner', fields: {'speed': 2.5}) as Spinner;
      expect(s.speed, 2.5);
      expect(go.behaviour<Spinner>(), same(s));
      expect(spinnerType.field('speed')!.get(s), 2.5);
      expect(
        s.behaviourType,
        same(spinnerType),
        reason: 'the editor reads its fields through it (ADR-0021 P7)',
      );
      expect(s.authoredComponentId, isNull, reason: 'no document authored it');
      expect(Spinner().behaviourType, isNull, reason: 'constructed by the game');
      expect((go.addBehaviourFromData('game.Spinner') as Spinner).speed, 1.0, reason: 'defaults kept');

      expect(
        () => go.addBehaviourFromData('game.Missing'),
        throwsA(isA<BehaviourDataError>().having((e) => e.typeId, 'typeId', 'game.Missing')),
      );
      expect(
        () => go.addBehaviourFromData('game.Spinner', fields: {'spedd': 1}),
        throwsA(isA<BehaviourDataError>().having((e) => e.field, 'field', 'spedd')),
      );
      expect(
        () => go.addBehaviourFromData('game.Spinner', fields: {'speed': 'fast'}),
        throwsA(isA<BehaviourDataError>().having((e) => e.field, 'field', 'speed')),
      );
      expect(() => go.addBehaviourFromData('game.Spinner', version: 2), throwsA(isA<BehaviourDataError>()));
      expect(go.behaviours, hasLength(2), reason: 'nothing was attached by the failures');
    },
  );

  test('a Spinner really rotates its entity, at the fixed rate', () {
    final w = world();
    addTearDown(w.dispose);
    final go = w.spawn();
    final s = go.addBehaviour(Spinner()..speed = math.pi); // half a turn per second
    for (var i = 0; i < 30; i++) {
      w.frame(1 / 60);
    }
    // 30 fixedUpdates (the first frame only spawned): 30/60 s → 90°.
    expect(s.angle, closeTo(math.pi / 2, 1e-9));
    final q = go.worldTransform!.rotation;
    expect(q.y, closeTo(math.sin(math.pi / 4), 1e-4));
    expect(q.w, closeTo(math.cos(math.pi / 4), 1e-4));
  });

  test('attach(): behaviours on entities loaded from a project', () {
    final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
    final cache = Directory.systemTemp.createTempSync('fynkel-behaviour-cache-').path;
    addTearDown(() => Directory(cache).deleteSync(recursive: true));
    cookProject(fixture, cacheDir: cache);
    final w = world();
    addTearDown(w.dispose);
    w.loadProject(fixture, cacheDir: cache);
    final entities = w.allEntities();
    expect(entities, hasLength(6));
    final go = w.attach(entities.first);
    expect(w.attach(entities.first), same(go));
    expect(go.isPending, isFalse);
    expect(go.name, w.nameOf(go));
    final log = <String>[];
    go.addBehaviour(Recorder(log));
    w.frame(1 / 60);
    expect(log.first, 'onCreate');
    w.frame(1 / 60);
    expect(go.worldTransform, isNotNull, reason: 'subscribed on attach');
  });

  test('a behaviour loaded from a scene knows its type and its authored component', () {
    final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
    final cache = Directory.systemTemp.createTempSync('fynkel-behaviour-cache-').path;
    addTearDown(() => Directory(cache).deleteSync(recursive: true));
    cookProject(fixture, cacheDir: cache);
    // The fixture's Spinner also authors an `axis`.
    final type = BehaviourType(
      id: 'game.Spinner',
      create: Spinner.new,
      fields: [
        ...spinnerType.fields,
        BehaviourField(name: 'axis', type: 'vec3', get: (_) => null, set: (_, _) {}),
      ],
    );
    final w = world(registry: BehaviourRegistry()..register(type));
    addTearDown(w.dispose);
    w.loadProject(fixture, cacheDir: cache);
    final spinners = [for (final e in w.allEntities()) ...w.attach(e).behavioursOf<Spinner>()];
    expect(spinners, hasLength(1), reason: "the Robot's game.Spinner");
    expect(spinners.single.behaviourType, same(type));
    expect(spinners.single.authoredComponentId, 'c0000000-0000-4000-8000-0000000000c2');
  });
}

class _OnFixed extends Behaviour {
  _OnFixed(this.log, this.onFixed, {this.onEvents});
  final List<String> log;
  final void Function(_OnFixed) onFixed;
  final void Function(List<EngineEvent>)? onEvents;

  @override
  void fixedUpdate(FixedUpdateContext context) {
    log.add('fixedUpdate');
    onEvents?.call(context.events);
    onFixed(this);
  }

  @override
  void update(UpdateContext context) => log.add('update');
  @override
  void lateUpdate(UpdateContext context) => log.add('lateUpdate');
  @override
  void onDisable() => log.add('onDisable');
  @override
  void onDestroy() => log.add('onDestroy');
}

class _Scores {
  int total = 0;
}

class _Unused {}
