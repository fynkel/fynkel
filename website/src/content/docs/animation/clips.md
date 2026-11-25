---
title: "Playing clips"
description: "Play, cross-fade and time a model's animations."
---

The examples here use [examples/garden](https://github.com/fynkel/fynkel/blob/main/examples/garden)'s `character.glb`, whose clips are
`idle`, `walk`, `sprint`, `pick-up`, `emote-yes` and more. It's a blocky toy
about 2.7 m tall, its origin at its feet: for a person-sized player in a
`CharacterController` ([Character controllers](/physics/characters/)), scale
it (`localScale = Vec3.all(0.6)`) and its walking pace with it. `model.bounds`
([Loading assets](/assets/loading/)) says how big a model is.

The examples here use [examples/garden](https://github.com/fynkel/fynkel/blob/main/examples/garden)'s `character.glb`, whose clips are
`idle`, `walk`, `sprint`, `pick-up`, `emote-yes` and more. It's a blocky toy
about 2.7 m tall, its origin at its feet: for a person-sized player in a
`CharacterController` ([Character controllers](/physics/characters/)), scale
it (`localScale = Vec3.all(0.6)`) and its walking pace with it.

```dart
class Hero extends GameObject {
  late final Animator _animator;
  Vec3 _velocity = Vec3.zero;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final model = add(await assets.instantiate('character.glb'));
    _animator = model.animator!..play('idle');
    _animator.at('walk', 0.3, () => print('step')); // a footstep
  }

  void pickUp() {
    _animator
      ..crossFade('pick-up', seconds: 0.1, loop: false)
      ..onEnd = (clip) => _animator.crossFade('idle');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    position += _velocity * time.delta;
    if (_animator.current != 'pick-up') {
      _animator.crossFade(_velocity.length > 0.1 ? 'walk' : 'idle', seconds: 0.2);
    }
  }
}
```

- `play(name)` starts a clip at once; `crossFade(name, seconds:)` fades to it
  from what plays. Asking for the clip that's already playing changes nothing,
  so an update can say what it wants every frame.
- `loop: false` holds a clip's last pose at its end (a pick-up, a fall) and
  calls `onEnd`; clips loop unless told not to.
- `speed` (1 as made, 0 paused, negative backwards), `time`, `length`,
  `current` (a clip's name or a blend's), `isPlaying(name)` and `weight(name)`
  (each taking a blend's name too) are there, and `stop()` keeps the pose it's
  in. Game time scales it: a paused world pauses its clips.
- `at(clip, seconds, callback)` calls back each time a clip passes that time:
  footsteps, the moment a sword hits. Each call adds one more:
  `removeMark(clip, seconds, callback)` takes one away (a pooled object that
  adds it in `onSpawn` takes it away in `onRelease`), and `removeMarks(clip)`
  all of a clip's, or `removeMarks()` all.
- The clips play in the engine, once a frame after `onUpdate` and the tweens
  and before `onLateUpdate`. Each moves only the parts it moves. To turn a
  part a clip moves (a head that looks at you), set it in `onLateUpdate`,
  after the clip; [examples/garden](https://github.com/fynkel/fynkel/blob/main/examples/garden) does.
- A clip moves the model's parts in place. Moving the character through the
  world is the game's code, so physics stays in charge of where it is; match
  the speed to the walk's pace so the feet don't slide.
- The parts a clip moves are the model's objects: a sword added to the hand
  (`model.find('hand')!.add(sword)`) follows it.
