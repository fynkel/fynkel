// Unity calls every pending `Start` before any `FixedUpdate` or `Update` of
// that pass (docs/foundation.md 1.8): a script whose `update` comes first
// in callback order still sees the `start` of one that comes after it —
// the usual "Start finds the other script, Update uses what it set up".

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

final List<String> calls = [];

class Early extends Behaviour {
  Early(this.late);
  final Late late;

  @override
  int get order => -1;

  @override
  void start() => calls.add('early start');

  @override
  void fixedUpdate(FixedUpdateContext context) => calls.add('early fixedUpdate: late started ${late.ready}');

  @override
  void update(UpdateContext context) => calls.add('early update: late started ${late.ready}');
}

class Late extends Behaviour {
  bool ready = false;

  @override
  void start() {
    ready = true;
    calls.add('late start');
  }

  @override
  void fixedUpdate(FixedUpdateContext context) => calls.add('late fixedUpdate');

  @override
  void update(UpdateContext context) => calls.add('late update');
}

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  World world() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    return w;
  }

  test('every pending start runs before the first update of the pass (foundation 1.8)', () {
    final w = world();
    final go = w.spawn(name: 'pair');
    final late = Late();
    go.addBehaviour(Early(late));
    go.addBehaviour(late);
    calls.clear();
    w.frame(1 / 60); // the tick resolves the object; update starts the scripts
    expect(calls, ['early start', 'late start', 'early update: late started true', 'late update']);
  });

  test('…and before the first fixedUpdate', () {
    final w = world();
    final go = w.spawn(name: 'pair');
    final late = Late();
    final early = Early(late);
    go.addBehaviour(early);
    go.addBehaviour(late);
    early.enabled = false;
    late.enabled = false;
    w.frame(1 / 60); // created, disabled: not started
    early.enabled = true;
    late.enabled = true;
    calls.clear();
    w.frame(1 / 60); // one tick, one update
    expect(calls, [
      'early start',
      'late start',
      'early fixedUpdate: late started true',
      'late fixedUpdate',
      'early update: late started true',
      'late update',
    ]);
  });
}
