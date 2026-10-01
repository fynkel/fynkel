// Clone and instantiate publish data first; lifecycle uses the next normal boundary.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

final List<String> calls = [];

class Enemy extends Behaviour {
  int hp = 0;
  String label = '';
  bool started = false;

  @override
  void onCreate() {
    hp = 10; // Awake's defaults
    calls.add('$label onCreate');
  }

  @override
  void onEnable() => calls.add('$label onEnable');

  @override
  void start() {
    started = true;
    calls.add('$label start');
  }

  @override
  void update(UpdateContext context) => calls.add('$label update');
}

final enemyType = BehaviourType(
  id: 'awake.Enemy',
  create: Enemy.new,
  fields: [
    BehaviourField(
      name: 'label',
      type: 'string',
      get: (b) => (b as Enemy).label,
      set: (b, v) => (b as Enemy).label = v as String,
    ),
  ],
);

/// Clones [template] in its update, as a spawner does, and sets the copy's
/// hp right after — what `Init` would.
class Spawner extends Behaviour {
  Spawner(this.template);
  final GameObject template;
  bool armed = false;
  GameObject? made;
  List<String>? duringClone;

  @override
  void update(UpdateContext context) {
    if (!armed) return;
    armed = false;
    final before = calls.length;
    final copy = made = template.clone(name: 'Copy');
    duringClone = calls.sublist(before);
    copy.behaviour<Enemy>()!.hp = 3;
  }
}

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8)
      ..throwOnDiagnostics = true
      ..registry.registerAll([enemyType]);
    addTearDown(w.dispose);
    return w;
  }

  test('clone in an automatic callback publishes before its deferred creation callbacks', () {
    final w = fresh();
    final errors = <BehaviourError>[];
    w.onBehaviourError = errors.add;
    final template = w.spawn(name: 'Template')..addBehaviour(Enemy()..label = 'root');
    final spawner = w.spawn(name: 'Spawner').addBehaviour(Spawner(template));
    w.frame(1 / 60);
    calls.clear();
    spawner.armed = true;
    w.frame(1 / 60);
    expect(errors, isEmpty);
    expect(spawner.duringClone, isEmpty, reason: 'no lifecycle code before the creating transaction commits');
    final copy = spawner.made!;
    expect(w.find('Copy'), same(copy));
    expect(copy.behaviour<Enemy>()!.hp, 3, reason: 'onCreate has not run yet');
    expect(copy.behaviour<Enemy>()!.isCreated, isFalse);
    w.frame(1 / 60);
    expect(copy.behaviour<Enemy>()!.isCreated, isTrue);
    expect(copy.behaviour<Enemy>()!.hp, 10, reason: 'onCreate runs at the later lifecycle boundary');
    expect(calls.where((call) => call == 'root onCreate'), hasLength(1));
  });

  test('a clone made outside a frame defers creation like a callback clone', () {
    final w = fresh();
    final template = w.spawn(name: 'Template')..addBehaviour(Enemy()..label = 'root');
    final child = w.spawn(name: 'Child', parent: template)..addBehaviour(Enemy()..label = 'child');
    final off = w.spawn(name: 'Off', parent: template)..setActive(false);
    off.addBehaviour(Enemy()..label = 'off');
    w.frame(1 / 60);
    expect(child.behaviour<Enemy>()!.started, isTrue);
    calls.clear();
    final copy = template.clone();
    expect(calls, isEmpty);
    expect(copy.behaviour<Enemy>()!.started, isFalse);
    copy.behaviour<Enemy>()!.hp = 3;
    w.frame(1 / 60);
    expect(copy.behaviour<Enemy>()!.started, isTrue);
    expect(
      copy.behaviour<Enemy>()!.hp,
      10,
      reason: 'onCreate runs at the same deferred boundary as callback clones',
    );
  });
}
