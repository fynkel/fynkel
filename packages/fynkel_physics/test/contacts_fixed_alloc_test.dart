// As contacts_alloc_test.dart, stepping in the fixed step.

import 'package:test/test.dart';

import 'allocations.dart';
import 'contact_scene.dart';

void main() {
  final madeEachFrame = allocations();

  // Awake bodies make their new transforms each step (step 4); the stays
  // must add nothing to that.
  test('frames of stays allocate nothing more than the same frames unheard, in the fixed step', () async {
    const frames = 20000;
    final (silent, _) = scene(betweenFrames: false, listen: false);
    final unheard = await madeEachFrame(silent, frames);
    final (world, listeners) = scene(betweenFrames: false, listen: true);
    final heard = await madeEachFrame(world, frames);
    expect({
      for (final MapEntry(:key, :value) in heard.entries)
        if (value - (unheard[key] ?? 0) >= frames) key: value - (unheard[key] ?? 0),
    }, isEmpty);
    for (final listener in listeners) {
      // A trigger stay and a collision stay a step, through 120,000 frames
      // of 1/60 s at 50 steps a second.
      expect(listener.stays, greaterThan(150000));
    }
  });
}
