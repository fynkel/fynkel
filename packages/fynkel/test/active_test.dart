// SetActive / activeInHierarchy on the Dart side (docs/design/set-active.md
// § 3): activeSelf read-your-writes, activeInHierarchy after the tick, a
// behaviour live only while its object is active in the hierarchy,
// onDisable children first and onEnable parents first, onCreate deferred
// until the object is first active, and no onDestroy without an onCreate.

import 'package:fynkel/advanced.dart';
import 'package:test/test.dart';

class Recorder extends Behaviour {
  Recorder(this.log, this.tag);

  final List<String> log;
  final String tag;

  @override
  void onCreate() => log.add('$tag.onCreate');
  @override
  void onEnable() => log.add('$tag.onEnable');
  @override
  void start() => log.add('$tag.start');
  @override
  void fixedUpdate(FixedUpdateContext context) => log.add('$tag.fixedUpdate');
  @override
  void onDisable() => log.add('$tag.onDisable');
  @override
  void onDestroy() => log.add('$tag.onDestroy');
}

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  World world() => World.create(library: lib)..throwOnDiagnostics = true;

  test('activeSelf reads back at once, activeInHierarchy after the tick', () {
    final w = world();
    addTearDown(w.dispose);
    final parent = w.spawn(name: 'parent');
    final child = w.spawn(name: 'child', parent: parent);
    expect(parent.activeSelf, isTrue);
    expect(parent.activeInHierarchy, isTrue, reason: 'ordinary spawn is committed');
    w.step();
    w.drainEvents();
    expect(parent.activeInHierarchy && child.activeInHierarchy, isTrue);

    parent.setActive(false);
    expect(parent.activeSelf, isFalse, reason: 'read-your-writes');
    expect(parent.activeInHierarchy, isFalse, reason: 'activity commits immediately');
    w.step();
    expect(parent.activeSelf, isFalse, reason: 'now from the world');
    expect(parent.activeInHierarchy, isFalse);
    expect(child.activeSelf, isTrue, reason: "the child's own flag is untouched");
    expect(child.activeInHierarchy, isFalse);

    parent.setActive(true);
    w.step();
    expect(child.activeInHierarchy, isTrue);
  });

  test('a subtree switched off disables children first and comes back parents first', () {
    final w = world();
    addTearDown(w.dispose);
    final log = <String>[];
    final parent = w.spawn(name: 'parent');
    final child = w.spawn(name: 'child', parent: parent);
    // Attach order puts the child's behaviour first in callback order.
    child.addBehaviour(Recorder(log, 'child'));
    parent.addBehaviour(Recorder(log, 'parent'));
    w.frame(1 / 60);
    expect(log, [
      'parent.onCreate',
      'parent.onEnable',
      'child.onCreate',
      'child.onEnable',
      'child.start',
      'parent.start',
      'child.fixedUpdate',
      'parent.fixedUpdate',
    ]);

    log.clear();
    parent.setActive(false);
    w.frame(1 / 60);
    // Queue adoption applies activity before lifecycle and phase dispatch.
    expect(log, ['child.onDisable', 'parent.onDisable']);
    log.clear();
    w.frame(1 / 60);
    expect(log, isEmpty, reason: 'nothing runs while inactive');

    parent.setActive(true);
    w.frame(1 / 60);
    expect(log, [
      'parent.onEnable',
      'child.onEnable',
      'child.fixedUpdate',
      'parent.fixedUpdate',
    ], reason: 'no second onCreate or start');
  });

  test('onCreate waits until the object is first active; never created, never destroyed', () {
    final w = world();
    addTearDown(w.dispose);
    final log = <String>[];
    final go = w.spawn(name: 'late')..setActive(false);
    final b = go.addBehaviour(Recorder(log, 'late'));
    final never = w.spawn(name: 'never')..setActive(false);
    never.addBehaviour(Recorder(log, 'never'));
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(log, isEmpty);
    expect(b.isCreated, isFalse);
    expect(go.activeInHierarchy, isFalse);

    go.setActive(true);
    never.destroy();
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(log, ['late.onCreate', 'late.onEnable', 'late.start', 'late.fixedUpdate', 'late.fixedUpdate']);
    expect(b.isCreated && b.isActive, isTrue);
  });

  test('a disabled behaviour on an object that comes back stays disabled', () {
    final w = world();
    addTearDown(w.dispose);
    final log = <String>[];
    final go = w.spawn(name: 'thing');
    final b = go.addBehaviour(Recorder(log, 'b'));
    w.frame(1 / 60);
    b.enabled = false;
    go.setActive(false);
    w.frame(1 / 60);
    go.setActive(true);
    w.frame(1 / 60);
    log.clear();
    w.frame(1 / 60);
    expect(log, isEmpty);
    expect(b.isActive, isFalse);
    b.enabled = true;
    w.frame(1 / 60);
    expect(log, ['b.onEnable', 'b.fixedUpdate']);
  });
}
