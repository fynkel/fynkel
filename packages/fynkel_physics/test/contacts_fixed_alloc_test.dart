// As contacts_alloc_test.dart, stepping in the fixed step.

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'package:test/test.dart';

import 'allocations.dart';
import 'contact_scene.dart';

void main() {
  final madeEachFrame = allocations();

  test('frames of stays allocate nothing, in the fixed step', () async {
    final (world, listeners) = scene(betweenFrames: false);
    expect(await madeEachFrame(world, 20000), isEmpty);
    for (final listener in listeners) {
      // A trigger stay and a collision stay a step, through at least
      // 120,000 frames of 1/60 s at 50 steps a second.
      expect(listener.stays, greaterThan(150000));
    }
  });
}
