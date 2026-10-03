import 'package:fynkel/gameplay.dart';

/// Explicit target fixtures for tests of playback state on a pending animator.
void addRigTargets(World world, GameObject root) {
  for (final (index, label) in [
    (2, 'hips'),
    (3, 'spine'),
    (4, 'head'),
    (5, 'Prop'),
  ]) {
    world
        .spawn(name: label, parent: root)
        .set(
          AnimationBinding(
            key: 'c0000000-0000-4000-8000-000000000005/node/$index',
          ),
        );
  }
}
