# Stage 11 report: sound

[The stage 11 plan](stage-11.md). Phases 1 to 3 are merged (#134, #135,
#136). Phase 4, sound in the three games, was left out on 2026-10-23
(below).

## The result

Stage 11's goal was sound as Unity has it: clips, sources on objects, a
listener, 3D sound that fades with distance and moves from ear to ear,
music that streams and crossfades, and volume groups a settings screen
can turn down. On a phone it had to behave as a player expects: the
silent switch, calls, the app in the background.

**The engine's part of that is done.** It's a package a game adds,
`package:fynkel_audio`, with its own native library. A game without it
ships no sound code. Each phase has its lab scene: "Sound: the basics",
"Sound: in 3D" and "Sound: music and the mix". They play on the Mac, the
iPhone 16 Pro Max and the Note10+. **The games have no sound yet**: that
was phase 4, and so were most of the plan's measurements (§6). What was
measured and what wasn't is below.

What a game writes:

```dart
class Dog extends GameObject {
  late final AudioSource _voice;

  @override
  Future<void> onLoad() async {
    _voice = addComponent(AudioSource(spatialBlend: 1, maxDistance: 30));
    final bark = await assets.audio('bark.ogg');
    on(input.onTap, (_) => _voice.playOneShot(bark, pitch: 0.95));
  }
}

class Meadow extends GameObject {
  @override
  Future<void> onLoad() async {
    audio.music.play(await assets.audio('meadow.ogg'), fadeIn: 2);
    audio.group('music').volume = 0.6; // from the settings screen's slider
  }
}
```

## What stage 11 did

| Phase | What | PR |
|---|---|---|
| 1 | `fynkel_audio`: clips (WAV, OGG, MP3) decoded when loaded, a mixer on `cpal` that never waits, allocates or frees on the audio thread; `AudioSource` in 2D, 32 voices with priority, starts and stops without clicks; pausing with the world, the background, the iPhone's silent switch and interruptions; a null output for tests | #134 |
| 2 | 3D sound: `AudioListener` (the camera's by default), `spatialBlend`, rolloff, `minDistance` and `maxDistance`, panning, behind a little quieter and duller, `audio.playAt`, `audio.drawSources`; positions sent once a frame in one call and smoothed in the mixer | #135 |
| 3 | Streaming (clips over 10 s), `audio.music` with fades and crossfades, groups with `volume`, `mute` and `fade`, hot reload of clips, Android's audio focus and calls | #136 |

The sound library's C interface is at ABI 4. The engine's own didn't
change.

## Measured

- **No gap in ten minutes** of a streamed track (phase 3's bar),
  2026-10-22. On the Mac, 56,251 callbacks. On the iPhone and the Note10+,
  the lab's scene with a crossfade every 30 s. On all three, no callback
  starved and no underrun.
- **The frame's cost of 3D sound** on the Note10+, 32 sources circling:
  0.086–0.102 ms a frame. About 0.035 ms of that is the camera sending
  the frame's moves to the engine early, which drawing would do anyway,
  so sound's own share is about 0.05 ms. The bar was 0.1 ms.
- **No garbage**: the Dart allocation tests play, move, crossfade
  streamed tracks, move the sliders, mute and fade, and allocate
  nothing. On the audio thread, a test counts the mixer's allocations
  and finds none.
- **Goldens**: a source left, right, near, far and behind, one flying
  past, and a crossfade (`crates/fynkel_audio/tests/goldens/`).

## Not measured (phase 4)

These were §6's bars inside a game. They go to stage 14, shipping, when
the games are measured again:

- **No fps lost**: a game with sound at each quality level as fast as
  without.
- **The audio thread's share of a core** on the Note10+ at 32 voices.
- **Latency**, a tap to its sound: under 50 ms on the iPhone, said for
  the Note10+.
- **Size**: what the package adds to an app, per platform.

## Found on the way

- **Doubles passed to a native call box in Dart's JIT.** Each one is an
  allocation, so fades and group volumes go to the library through a
  struct it keeps (`FaFade`), not as arguments.
- **Logarithmic rolloff doesn't go silent** at `maxDistance`; it stops
  falling there, as Unity's does. The plan had said silent.
- **The soak test's memory check can fail without a leak.** It holds the
  lowest of the later rounds against the lowest of the earlier ones, and
  one deep dip early on (208 MB against 218) failed #136 once while
  malloc and the Dart heap stayed flat. The re-run passed.
- **Android's audio focus is half there.** Playing takes the focus, so
  the player's music stops, and a call holds the sound. Another app
  taking the focus isn't heard: that needs a Java listener.

## Not in this stage

Effects on the sound (reverb, echo, a low-pass), reverb zones, Doppler,
HRTF, sound blocked by walls, ducking by itself, the microphone, and
sound made in code. They're in the guide's §21.

## Next

Stage 12, particles ([plan](stage-12.md)). Then animation 2, cut down,
and shipping. Shipping is now stage 14.
