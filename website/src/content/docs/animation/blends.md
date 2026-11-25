---
title: "Blends and layers"
description: "Walk to run at any speed, and a wave over the top."
---

Two tools mix clips: a **blend**, for moving at any speed, and
**layers**, for playing one clip on part of the body over another:

```dart
class Runner extends GameObject {
  late final Animator _animator;
  double speed = 0; // m/s, as the game's input says

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final model = add(await assets.instantiate('knight.glb'));
    _animator = model.animator!;
    // Layer 1: the right arm, from the shoulder down.
    _animator.layer(1, mask: 'upperarm.r');
  }

  void wave(bool on) {
    final arm = _animator.layer(1);
    on ? arm.crossFade('Cheer', seconds: 0.3) : arm.stop(seconds: 0.3);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    position += Vec3(0, 0, speed * time.delta);
    // Idle standing, the walk at 1.1 m/s, the run at 3.4.
    _animator.blend('move', const ['Idle', 'Walking_A', 'Running_A'], thresholds: const [0, 1.1, 3.4], at: speed);
  }
}
```

- `blend(name, clips, thresholds:, at:)` is Unity's 1D blend tree, as a call:
  each clip at its threshold (0, 1, 2… if left out), and `at` mixes the two
  around it. The clips keep one pace, each as far through itself, so the feet
  land together at any mix; with thresholds at the speeds the clips' feet go,
  they don't slide.

  Past the last threshold the last clip plays at its own pace, so feet faster
  than it slide: set the animator's `speed` to `at / last threshold` there.
  Call it every frame with the number: it's made the first time, and fades in
  (`seconds:`) only when something else was playing.
- `layer(1…7, mask:, additive:, strength:)` plays over the layers below it: on
  the whole model, or from a part down (`mask`, a name as `find` takes). An
  additive layer adds its clip's movement from the clip's first pose (a
  breath, a flinch over any pose) instead of taking the parts' place.

  `strength` (Unity's layer weight) shows it part-way. A layer has the
  animator's own `play`, `crossFade`, `blend`, `stop`, `current`, `time` and
  `weight`; the animator itself is the base layer.
- Every fade starts from the pose shown, however it got there: mid-fade,
  mid-blend, or held after a `stop()`. A game can change its mind every frame
  and the character never jumps. `stop(seconds:)` fades a layer out to what's
  underneath.
- A layer's clips call `onEnd` and `at(…)` callbacks as the base's do.
