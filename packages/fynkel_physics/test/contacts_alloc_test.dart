// A step where no contact starts or ends allocates nothing
// (docs/stage-5.md, step 5), stepping between frames, counted as
// alloc_test.dart counts; contacts_fixed_alloc_test.dart steps in the
// fixed step (one scenario a file: in the JIT one's code can box doubles
// in the next's).

import 'package:test/test.dart';

import 'allocations.dart';
import 'contact_scene.dart';

void main() {
  final madeEachFrame = allocations();

  // Awake bodies make their new transforms each step (step 4); the stays
  // must add nothing to that.
  test('frames of stays allocate nothing more than the same frames unheard, stepping between frames', () async {
    const frames = 20000;
    final (silent, _) = scene(betweenFrames: true, listen: false);
    final unheard = await madeEachFrame(silent, frames);
    final (world, listeners) = scene(betweenFrames: true, listen: true);
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
