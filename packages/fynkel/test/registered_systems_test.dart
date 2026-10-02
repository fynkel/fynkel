import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class Recorder extends Behaviour {
  Recorder(this.events);
  final List<String> events;
  GameObject? target;
  @override
  void fixedUpdate(FixedUpdateContext context) {
    events.add('behaviour');
  }
}

class PhaseRecorder extends Behaviour {
  PhaseRecorder(this.ticks);
  final Map<GameplayPhase, List<int>> ticks;
  @override
  bool get wantsAfterPhysics => true;
  @override
  void fixedUpdate(FixedUpdateContext c) =>
      ticks[GameplayPhase.fixedUpdate]!.add(c.tick);
  @override
  void afterPhysics(AfterPhysicsContext c) =>
      ticks[GameplayPhase.afterPhysics]!.add(c.tick);
  @override
  void update(UpdateContext c) => ticks[GameplayPhase.update]!.add(c.tick);
  @override
  void lateUpdate(UpdateContext c) =>
      ticks[GameplayPhase.lateUpdate]!.add(c.tick);
}

void main() {
  late World w;
  setUp(() => w = World.create());
  tearDown(() => w.dispose());
  test('same-count metadata replacement refreshes dead reference fields on existing behaviours', () {
    final target = w.spawn();
    final b = Recorder([])..target = target;
    w.registry.register(
      BehaviourType(
        id: 'game.Recorder',
        dartType: Recorder,
        create: () => Recorder([]),
      ),
    );
    w.spawn().addBehaviour(b);
    w.frame(0);
    w.registry.register(
      BehaviourType(
        id: 'game.Recorder',
        dartType: Recorder,
        create: () => Recorder([]),
        fields: [
          BehaviourField(
            name: 'target',
            type: 'gameObject',
            get: (b) => null,
            target: (b) => (b as Recorder).target,
            set: (b, value) => (b as Recorder).target = value as GameObject?,
          ),
        ],
      ),
      replace: true,
    );
    target.destroy();
    w.frame(0);
    expect(b.target, isNull);
  });
  test('generated runtime type avoids constructor probing and same-count replacement refreshes participation', () {
    final log = <String>[];
    var constructions = 0;
    BehaviourType type(Set<String> callbacks) => BehaviourType(
      id: 'game.Recorder',
      dartType: Recorder,
      create: () {
        constructions++;
        return Recorder(log);
      },
      callbacks: callbacks,
    );
    w.registry.register(type({}));
    w.spawn().addBehaviour(Recorder(log));
    expect(constructions, 0);
    w.frame(w.fixedDt);
    expect(log, isEmpty);
    w.registry.register(type({'fixedUpdate'}), replace: true);
    w.frame(w.fixedDt);
    expect(log, ['behaviour']);
    expect(constructions, 0);
  });
  test('systems and behaviours share phase ticks across zero and multiple tick frames', () {
    final scripts = {for (final p in GameplayPhase.values) p: <int>[]};
    final before = {for (final p in GameplayPhase.values) p: <int>[]};
    final after = {for (final p in GameplayPhase.values) p: <int>[]};
    w.spawn().addBehaviour(PhaseRecorder(scripts));
    w.registerSystems([
      for (final p in GameplayPhase.values) ...[
        GameplaySystem(
          id: 'game.Before${p.name}',
          phase: p,
          before: ['fynkel.behaviours.${p.name}'],
          run: (c) => before[p]!.add(c.tick),
        ),
        GameplaySystem(
          id: 'game.After${p.name}',
          phase: p,
          after: ['fynkel.behaviours.${p.name}'],
          run: (c) => after[p]!.add(c.tick),
        ),
      ],
    ]);
    w.frame(0);
    w.frame(w.fixedDt * 2);
    w.frame(w.fixedDt);
    expect(scripts[GameplayPhase.fixedUpdate], [0, 1, 2]);
    expect(scripts[GameplayPhase.afterPhysics], [0, 1, 2]);
    expect(scripts[GameplayPhase.update], [0, 2, 3]);
    expect(scripts[GameplayPhase.lateUpdate], [0, 2, 3]);
    expect(before, scripts);
    expect(after, scripts);
  });
  test(
    'registered systems order around behaviour phase and rollback a failure',
    () {
      final log = <String>[];
      final p = w.spawn(name: 'player');
      p.addBehaviour(Recorder(log));
      w.registerSystems([
        GameplaySystem(
          id: 'game.After',
          phase: GameplayPhase.fixedUpdate,
          after: ['fynkel.behaviours.fixedUpdate'],
          run: (_) {
            expect(p.name, 'player');
            log.add('after');
          },
        ),
        GameplaySystem(
          id: 'game.Before',
          phase: GameplayPhase.fixedUpdate,
          before: ['fynkel.behaviours.fixedUpdate'],
          run: (_) {
            log.add('before');
            p.name = 'discard';
            throw StateError('reject');
          },
        ),
      ]);
      w.frame(w.fixedDt);
      expect(log, ['before', 'behaviour', 'after']);
      expect(p.name, 'player');
      expect(w.drainSystemFailures().single.id, 'game.Before');
      expect(w.gameplaySystemTimes.length, 2);
    },
  );
  test(
    'missing dependency cycle and cross-phase reversal preserve schedule',
    () {
      var calls = 0;
      w.registerSystems([
        GameplaySystem(
          id: 'game.Valid',
          phase: GameplayPhase.update,
          run: (_) => calls++,
        ),
      ]);
      expect(
        () => w.registerSystems([
          GameplaySystem(
            id: 'game.Missing',
            phase: GameplayPhase.update,
            after: ['game.No'],
            run: (_) {},
          ),
        ]),
        throwsStateError,
      );
      expect(
        () => w.registerSystems([
          GameplaySystem(
            id: 'game.X',
            phase: GameplayPhase.update,
            after: ['game.Y'],
            run: (_) {},
          ),
          GameplaySystem(
            id: 'game.Y',
            phase: GameplayPhase.update,
            after: ['game.X'],
            run: (_) {},
          ),
        ]),
        throwsStateError,
      );
      expect(
        () => w.registerSystems([
          GameplaySystem(
            id: 'game.Reversed',
            phase: GameplayPhase.fixedUpdate,
            after: ['game.Valid'],
            run: (_) {},
          ),
        ]),
        throwsStateError,
      );
      w.frame(0);
      expect(calls, 1);
    },
  );
  test('generated participation avoids a known-unused callback but manual registration stays conservative', () {
    final log = <String>[];
    final type = BehaviourType(
      id: 'game.Recorder',
      create: () => Recorder(log),
      callbacks: const {},
    );
    w.registry.register(type);
    w.spawn().addBehaviourFromData(type.id);
    w.frame(w.fixedDt);
    expect(log, isEmpty);
  });
}
