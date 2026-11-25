---
title: "A character in a game"
description: "Legs that blend and gestures over them, put together."
---

Put together, a character is a blend for its legs and a layer or two
over it. This one walks where it's told: it runs to a place far away,
slows to a walk near it and stands there, and nods yes from the waist up
without stopping. examples/coins's player is the same, with a coin in
its hand:

```dart
class Farmer extends GameObject {
  late final Animator _animator;
  Vec3 _goal = Vec3.zero;
  double _speed = 0; // m/s, eased, so the blend eases too

  static const walk = 2.0, run = 5.0; // the speeds the clips' feet go

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final model = add(await assets.instantiate('character.glb'));
    _animator = model.animator!
      ..blend('move', const ['idle', 'walk', 'sprint'], thresholds: const [0, walk, run], at: 0, seconds: 0)
      // A gesture is played once; then the layer fades out.
      ..onEnd = (_) => _animator.layer(1).stop(seconds: 0.15);
    _animator.layer(1, mask: 'torso'); // the legs stay the blend's
  }

  void walkTo(Vec3 point) => _goal = point.flat;

  void nod() => _animator.layer(1).crossFade('emote-yes', seconds: 0.1, loop: false);

  @override
  void onUpdate() {
    super.onUpdate();
    final to = _goal - transform.position;
    final distance = to.length;
    final wanted = distance < 0.05 ? 0.0 : (distance > 3 ? run : walk);
    _speed += (wanted - _speed) * (6 * time.delta).clamp(0, 1);
    if (distance >= 0.05) {
      final step = to.normalized * (_speed * time.delta).clamp(0, distance);
      // A glTF model faces +Z, and lookRotation turns -Z (forward) the way
      // given: so the other way.
      transform = Transform(position: transform.position + step, rotation: Quat.lookRotation(-to));
    }
    _animator.blend('move', const ['idle', 'walk', 'sprint'], thresholds: const [0, walk, run], at: _speed);
  }
}
```

- **A model faces +Z**, as glTF files are made, while an object's forward is
  −Z ([Moving things](/basics/moving/)): `Quat.lookRotation(-to)` turns its
  face to `to`. With `lookRotation(to)` it walks backwards.
- The speed it moves at is the number the blend gets, and the thresholds are
  the speeds the clips' feet go: the feet don't slide at any speed between.
- Easing the speed eases the blend: it slows into a walk, not snaps.
- `onEnd` is on the animator, for every layer: here only the gesture layer
  plays clips that end.
- A test can check all of it frame by frame ([Testing a
  game](/making/testing/)): `animator.weight('sprint')`,
  `animator.layer(1).current`.
