// What the component lookups return around `destroy()` (docs/foundation.md
// 1.9). Unity's rule: `Destroy(component)` takes effect at the end of the
// frame, so until then `GetComponent`, `GetComponentInChildren` and
// `GetComponentInParent` still return it; after it, none does. Here the
// destroy takes effect at the next lifecycle boundary, and every lookup
// agrees on it.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class Health extends Behaviour {}

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

  test('a destroy-requested behaviour is found by every lookup until the boundary, by none after (foundation 1.9)', () {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    final parent = w.spawn(name: 'Parent');
    final child = w.spawn(name: 'Child', parent: parent);
    final health = child.addBehaviour(Health());
    final killer = parent.addBehaviour(Killer(child));
    w.frame(1 / 60);
    w.frame(1 / 60);

    killer.armed = true;
    w.frame(1 / 60);
    expect(killer.seen, {
      'behaviour': health,
      'getInChildren': health,
      'getAllInChildren': 1,
      'getInParent': health,
    }, reason: 'the same frame: Destroy has not taken effect (Unity)');

    w.frame(1 / 60);
    expect(health.isDestroyed, isTrue);
    expect(child.behaviour<Health>(), isNull);
    expect(parent.getInChildren<Health>(), isNull);
    expect(child.getInParent<Health>(), isNull);
  });
}
