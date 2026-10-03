// Transactional removal is immediately absent from gameplay lookups; lifecycle
// cleanup waits for its safe boundary. Retired scripts receive no later callbacks.

import 'package:fynkel/advanced.dart';
import 'package:test/test.dart';

class Health extends Behaviour {
  int lates = 0;
  @override
  void lateUpdate(UpdateContext context) => lates++;
}

/// Destroys the child's Health in its update, then looks it up every way.
class Killer extends Behaviour {
  Killer(this.child);
  final GameObject child;
  bool armed = false;
  final Map<String, Object?> seen = {};

  @override
  void update(UpdateContext context) {
    if (!armed) return;
    armed = false;
    child.behaviour<Health>()!.destroy();
    seen['behaviour'] = child.behaviour<Health>();
    seen['getInChildren'] = gameObject.getInChildren<Health>();
    seen['getAllInChildren'] = gameObject.getAllInChildren<Health>().length;
    seen['getInParent'] = child.getInParent<Health>();
  }
}

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  test('transactional destruction hides a behaviour immediately and suppresses later callbacks', () {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    final parent = w.spawn(name: 'Parent');
    final child = w.spawn(name: 'Child', parent: parent);
    final health = child.addBehaviour(Health());
    final killer = parent.addBehaviour(Killer(child));
    w.frame(1 / 60);
    w.frame(1 / 60);

    final lates = health.lates;
    killer.armed = true;
    w.frame(1 / 60);
    expect(killer.seen, {
      'behaviour': null,
      'getInChildren': null,
      'getAllInChildren': 0,
      'getInParent': null,
    }, reason: 'all lookup paths use the staged removal');

    w.frame(1 / 60);
    expect(health.lates, lates, reason: 'no later callback after committed removal');
    expect(health.isDestroyed, isTrue);
    expect(child.behaviour<Health>(), isNull);
    expect(parent.getInChildren<Health>(), isNull);
    expect(child.getInParent<Health>(), isNull);
  });
}
