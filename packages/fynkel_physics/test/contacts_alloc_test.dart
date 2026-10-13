// A step where no contact starts or ends allocates nothing
// (docs/stage-5.md, step 5), stepping between frames, counted as
// alloc_test.dart counts; contacts_fixed_alloc_test.dart steps in the
// fixed step (one scenario a file: in the JIT one's code can box doubles
// in the next's).

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'package:test/test.dart';

import 'allocations.dart';
import 'contact_scene.dart';

void main() {
  final madeEachFrame = allocations();

  test('frames of stays allocate nothing, stepping between frames', () async {
    final (world, listeners) = scene(betweenFrames: true);
    expect(await madeEachFrame(world, 20000), isEmpty);
    for (final listener in listeners) {
      // A trigger stay and a collision stay a step, through at least
      // 120,000 frames of 1/60 s at 50 steps a second.
      expect(listener.stays, greaterThan(150000));
    }
  });
}
