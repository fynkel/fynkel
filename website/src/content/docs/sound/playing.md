---
title: "Playing sounds"
description: "Clips, sources, one-shots and the phone's own sound."
---

Sound is a package, `fynkel_audio`, with a mixer of its own. Add it to
the pubspec, next to `fynkel`, and a game without it ships none of it:

```sh
flutter pub add fynkel_audio
```

## Clips

**A clip** is a sound file, `.wav`, `.ogg` or `.mp3`, loaded by name as any
asset is ([Loading assets](/assets/loading/)): `await
assets.audio('bark.ogg')` (the names here are your own files'; the examples
ship only the lab's music). It's decoded once, off the UI thread, shared, and
freed when the last object that asked for it is destroyed.

`AudioClip.fromSamples(samples, rate:, channels:)` makes one from numbers,
freed when nothing reaches it, or at once with `dispose()`; sounds already
playing it play to their end either way.

## Sources

An `AudioSource` plays sounds from its object, as Unity's.

| | |
| --- | --- |
| `play()`, `pause()`, `unPause()`, `stop()` | Its `clip` |
| `playOneShot(clip, volume:, pitch:)` | Any clip over it: footsteps, hits |
| `volume`, `mute` | How loud |
| `pitch` | 2 is an octave up and twice as fast |
| `loop` | Over and over |
| `playOnAwake` | On unless set: it plays `clip` when it's enabled |
| `isPlaying`, `playTime` | Where it is (Unity's `time`; here `time` is the game's). Set `playTime` to seek, but not past the clip's end |
| `priority` | Which sound gives way past 32 at once |
| `onEnd` | Called when `clip` reaches its end, not when it's stopped |

Disabling the source, or destroying its object, stops its sounds. As in
Unity, a source that's disabled, or whose object is inactive, refuses to
play (a `StateError`), so no sound outlives its object. What an `onLoad`
asks for plays when the object is shown.

```dart
// lib/dog.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';

/// Barks on B, and steps as it walks.
class Dog extends Behaviour {
  late final AudioSource voice;
  late final AudioClip bark, step;
  var _walked = 0.0;
  var _left = true;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    bark = await assets.audio('bark.ogg');
    step = await assets.audio('step.wav');
    voice = gameObject.addComponent(AudioSource(clip: bark, playOnAwake: false));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (input.wasPressed(KeyCode.b)) voice.play();
    final move = input.axis2('move');
    gameObject.position += Vec3(move.x, 0, -move.y) * (3 * time.delta);
    _walked += move.length * 3 * time.delta;
    if (_walked > 0.8) {
      _walked = 0;
      _left = !_left;
      voice.playOneShot(step, pitch: _left ? 0.95 : 1.05);
    }
  }
}
```

## The world's sound

**The world's sound**, `audio` on every object and behaviour, is Unity's
`AudioListener` and `AudioSettings` together: `audio.volume` for everything (a
settings screen's master volume), `audio.playOneShot(clip)` for a sound that
belongs to no object (a button's click), `audio.stats` (`voices` playing now,
`stolen` that gave way to more important sounds, `refused` not played,
`underruns` for crackles, `starved` for streams) and `audio.outputError`. It's
package:fynkel_audio's (an extension, `WorldAudio`), so a test that reaches it
through a `World` imports it: `import 'package:fynkel_audio/fynkel_audio.dart'
show WorldAudio;`.

Physics is the same: `world.physics` is package:fynkel_physics'
`WorldPhysics`, and any object's `physics` is the same thing.

## Pausing

While the game is paused (`world.paused`, [Keys and pausing](/input/pause/)) its sounds
hold where they are, and go on with it. A source with `ignorePause`
plays on, as a pause menu's sounds should, and so does
`audio.playOneShot(click, ignorePause: true)`. While the app is away
(in the background, or a call comes in), nothing plays.

## Slow motion

`time.scale` doesn't change sound, as in Unity, unless
a source has `followTimeScale`: then it goes slower and lower with the
game, and holds when the game's time stands still.

## 32 sounds at once

As many sounds as that play together. Past it the least important gives way:
`priority` 0 is the most important, 256 the least, 128 unless set; of equals,
one fading out goes first, then the oldest. A sound of priority 0 never gives
way, as music doesn't, so keep 0 for music.

A sound fading out (`music.crossFade`, `stop(fadeOut:)`) keeps its importance
until it's done. The one that gives way fades out over a few milliseconds, so
it doesn't click.

## The silent switch, calls and the mic

The iPhone's silent switch mutes the game and the player's own music plays on
under it, as most games do. `audio.session = AudioSession.playback` plays
through the switch and stops their music, for a game whose sound is the point;
on Android it takes the audio focus, which stops their music the same way.

A game that listens on the mic while it plays, like a call, sets
`AudioSession.voiceChat` before its first sound: the mic works alongside the
game's sound, which plays from the loudspeaker through the switch, and the
mic's plugin is told to leave the session alone (`record`'s
`IosRecordConfig(manageAudioSession: false)`).

On both, the game's sound waits while a call rings or is on; on Android, with
`voiceChat`, a call over the internet doesn't count (the game's own voice chat
puts the phone in that call's mode), only a phone call.

On the iPhone it waits the same way while Siri, an alarm or a call shown over
the game holds the sound, and goes on where it was when that's over.

## When it starts

The sound starts when the world is first shown on screen. In a
test, which shows nothing, the sound is made and timed the same way
with no device: `world.update` steps it, a clip ends after its length,
and `onEnd` is called.
