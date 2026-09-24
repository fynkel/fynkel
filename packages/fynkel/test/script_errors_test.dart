// One script throwing (docs/unity-parity.md § Tier 2): as in Unity, only
// that call stops — every other behaviour still runs that pass, the one
// that threw is called again next time, and the error reaches the
// editor's Console with its Dart stack — over the real native library.

import 'dart:async';
import 'dart:convert';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class Counter extends Behaviour {
  int updates = 0, fixed = 0, lates = 0;

  @override
  void fixedUpdate(FixedUpdateContext context) => fixed++;
  @override
  void update(UpdateContext context) => updates++;
  @override
  void lateUpdate(UpdateContext context) => lates++;
}

class Thrower extends Behaviour {
  Thrower({this.inStart = false, this.inCreate = false});

  final bool inStart, inCreate;
  int updates = 0;

  @override
  int get order => -1; // before the Counters

  @override
  void onCreate() {
    if (inCreate) throw StateError('no create');
  }

  @override
  void start() {
    if (inStart) throw StateError('no start');
  }

  @override
  void fixedUpdate(FixedUpdateContext context) => throw StateError('no fixed');

  @override
  void update(UpdateContext context) {
    updates++;
    throw StateError('no ground');
  }
}

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  test('a throwing update stops only itself; it runs again the next frame', () {
    final w = World.create(library: lib)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    final errors = <BehaviourError>[];
    w.onBehaviourError = errors.add;
    final hero = w.spawn(name: 'Hero');
    final thrower = Thrower();
    final before = Counter(), after = Counter();
    hero.addBehaviour(thrower);
    hero.addBehaviour(after);
    w.spawn(name: 'Other').addBehaviour(before);

    w.frame(1 / 60);
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(thrower.updates, greaterThanOrEqualTo(2), reason: 'still called every frame');
    for (final c in [before, after]) {
      expect(c.updates, thrower.updates, reason: 'every other behaviour runs its update');
      expect(c.lates, thrower.updates, reason: 'and its lateUpdate');
      expect(c.fixed, greaterThanOrEqualTo(2));
    }
    expect(errors.map((e) => e.callback).toSet(), {'fixedUpdate', 'update'});
    final update = errors.firstWhere((e) => e.callback == 'update');
    expect(update.message, 'Thrower.update on "Hero" threw: Bad state: no ground');
    expect(update.stack.toString(), contains('script_errors_test.dart'));
    expect(update.behaviour, same(thrower));
  });

  test('a throwing onCreate or start is reported once; the behaviour still updates', () {
    final w = World.create(library: lib)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    final errors = <BehaviourError>[];
    w.onBehaviourError = errors.add;
    final create = Thrower(inCreate: true), start = Thrower(inStart: true);
    final counter = Counter();
    w.spawn(name: 'A')
      ..addBehaviour(create)
      ..addBehaviour(start)
      ..addBehaviour(counter);
    w.frame(1 / 60);
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(errors.where((e) => e.callback == 'onCreate'), hasLength(1));
    expect(errors.where((e) => e.callback == 'start'), hasLength(1));
    expect(create.updates, counter.updates, reason: 'enabled and updating, as in Unity');
    expect(start.updates, counter.updates - 1, reason: 'the frame whose start threw skips its update');
    expect(counter.updates, greaterThanOrEqualTo(2));
  });

  test('under an editor, the error is a Console record with the stack', () {
    final lines = <String>[];
    EditorChannel.debugUse(enabled: true, sink: lines.add);
    addTearDown(EditorChannel.debugUse);
    final w = World.create(library: lib)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    w.spawn(name: 'Hero').addBehaviour(Thrower());
    w.frame(1 / 60);
    final records = [for (final l in lines) jsonDecode(l) as Map<String, Object?>];
    final errors = records.where((r) => r['source'] == 'exception').toList();
    expect(errors, isNotEmpty);
    final update = errors.firstWhere((r) => (r['message'] as String).startsWith('Thrower.update'));
    expect(update['severity'], 'error');
    expect(update['message'], 'Thrower.update on "Hero" threw: Bad state: no ground');
    expect(update['stack'], contains('script_errors_test.dart'));
  });

  test('with no handler set, a test zone sees the error (a test fails on a broken script)', () {
    final w = World.create(library: lib)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    w.spawn(name: 'Hero').addBehaviour(Thrower());
    final seen = <Object>[];
    runZonedGuarded(() => w..frame(1 / 60)..frame(1 / 60), (e, _) => seen.add(e));
    expect(seen.whereType<StateError>().map((e) => e.message), containsAll(['no fixed', 'no ground']));
  });
}
