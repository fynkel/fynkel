---
title: "Sound in 3D"
description: "Sounds heard from where they are."
---

A source is 2D unless told otherwise: the same in both ears
wherever it is, as music and clicks should be. Give it a `spatialBlend`
of 1 and it's heard from where its object is, as Unity's:

- **Left and right** from where it is around whoever hears, and a little
  quieter and duller right behind, so behind isn't in front.
- **Fading with distance**: full within `minDistance` (1 m unless set), then
  by `rolloffMode`. `AudioRolloffMode.logarithmic` (the default) halves with
  each doubling of the distance, as sound does, and stops falling at
  `maxDistance` (500 m); `linear` falls in a straight line to silence at
  `maxDistance`, which is easier to place.
- **Who hears** is the camera, unless an object has an `AudioListener` (the
  player, in a game whose camera flies far behind). With more than one, the
  last enabled hears.
- **It moves** with its object. Once a frame, after the cameras move, where
  every playing 3D source and the listener are is read in one call, and the
  mixer slides each sound to its new place over a few milliseconds, so one
  that flies past doesn't crackle.
- `spatialBlend` between 0 and 1 blends the two; `panStereo` (-1 left to 1
  right) places the 2D part.
- `audio.playAt(clip, position)` plays a one-shot at a place that belongs to
  no object (Unity's `PlayClipAtPoint`): an explosion, a coin picked up. It
  stays where it was played.
- `audio.stopOneShots()` stops every sound `playOneShot` and `playAt` started,
  on leaving a level, say; sources and the music play on.
- `audio.drawSources = true` draws each 3D source's `minDistance` and
  `maxDistance` as spheres with the debug lines ([Layers and debug
  drawing](/graphics/layers-and-debug/)), to see where each can be heard.

```dart
// lib/hive.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';

/// A hive that hums, heard from where it is, and bursts with a pop.
class Hive extends Behaviour {
  late final AudioClip pop;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    pop = await assets.audio('pop.wav');
    gameObject.addComponent(
      AudioSource(
        clip: await assets.audio('hum.ogg'),
        loop: true,
        spatialBlend: 1,
        maxDistance: 25,
        rolloffMode: AudioRolloffMode.linear,
      ),
    );
  }

  void burst() {
    audio.playAt(pop, gameObject.position);
    gameObject.destroy();
  }
}
```

Sound in 3D costs the frame little: on the Note10+, 32 sounds moving
take under a tenth of a millisecond.
