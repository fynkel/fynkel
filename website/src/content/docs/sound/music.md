---
title: "Music and volume groups"
description: "Music, streaming, volume sliders, and testing what is heard."
---

**Music** is `audio.music`: one track at a time, looping unless told not to,
playing on through a pause (as a pause menu's music should;
`audio.music.ignorePause = false` pauses it with the game, from the next track
it plays), and never given up for other sounds.

`play(clip, fadeIn:)` starts a track (the one already playing plays on, so a
level that starts its music again doesn't restart it), `crossFade(clip,
seconds)` fades the next in as the last fades out, and `stop(fadeOut:)` ends
it. `playTime` is where the track is, as a source's: set it to seek, or,
before `play`, to start partway in (say, where a track was left before a scene
change).

Its `onEnd` is called when a track that doesn't `loop` reaches its end. `clip`
is the track playing (the one faded to, in a cross-fade), `isPlaying` whether
one is, and `volume` the music's own.

## Long clips stream

**A long clip streams.** A file over 10 seconds is decoded as it plays, a
second ahead, on a thread of its own, so a three-minute song takes its file's
memory rather than 30 MB of samples. `AudioClip.streamed` says which;
`AudioClip.decode(bytes, stream: true)` streams a short one, and `stream:
false` decodes a long one whole. It plays as it would decoded, from any
`playTime`, looping without a gap; `audio.stats.starved` counts the times a
stream wasn't ready (it should stay 0).

## Groups

Volume groups are a settings screen's "music" and "effects" (a group of Unity's
`AudioMixer`): `audio.group('effects')` is made the first time it's asked for,
and up to 15 can be. Each has a `volume` and `mute` under `audio.volume`, and
`fade(to, seconds)`, all heard within a few milliseconds, without a click.

A group is passed as itself, never by name: a source joins one as it's made,
`AudioSource(clip: hit, group: audio.group('effects'))` (or later, with its
`group`), a one-shot with `playOneShot(clip, group: audio.group('effects'))`
or `playAt(..., group:)`, and the music plays in `audio.group('music')`. A
group belongs to its world's `audio`: another world's throws.

```dart
// lib/sound_settings.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';

/// Calm music that turns to a chase when an enemy sees the player, and
/// sliders' values for the settings screen.
class Soundtrack extends Behaviour {
  late final AudioClip calm, chase;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    calm = await assets.audio('music/calm.ogg');
    chase = await assets.audio('music/chase.ogg');
    audio.music.play(calm, fadeIn: 2);
  }

  void seen() => audio.music.crossFade(chase, 1.5);
  void lost() => audio.music.crossFade(calm, 4);

  /// From the settings screen's sliders, 0 to 1.
  void settings({required double music, required double effects}) {
    audio.group('music').volume = music;
    audio.group('effects').volume = effects;
  }

  /// Quieter music while a character speaks.
  void speaking(bool on) => audio.group('music').fade(on ? 0.25 : 1, 0.5);
}
```

## Hot reload

Hot reload reads a changed sound file again: the clip sounds as the
file now does from the next time it's played (what plays plays on). A
file that's no longer sound keeps the old, and the error goes to
`world.errors`.

## In a test

In a test, `audio.render(frames)` makes the mix as a speaker would have
played it, left and right, to check what plays and where it's heard.
It's the sound's clock while it listens: the sound moves on by what it
makes. Frames run (`world.update`) move it on too, and what they played
was heard by no one, so listen *instead of* running frames: play the
sound, then `render` the 48,000ths you want to hear.

## Loud together

The sounds playing at once add up, and the mix is cut
flat at full scale, as Unity's is: four sounds at half volume together
are already too loud, and crackle. Leave room: effects at 0.3 to 0.5,
the music under them, and a group's `volume` for the rest.
`render` shows it in a test (samples at ±1.0 are cut).

Values out of range (a volume over 1, a pitch of 0) throw an
`ArgumentError`; a file that isn't sound throws a `FormatException` with
its name.
