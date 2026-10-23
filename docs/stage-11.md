# Stage 11 plan: sound

Status: accepted 2026-10-21 ("let's go with your recommendation"), with the recommended answer to each decision (§7). Scope and rules:
[ADR-0001](decisions/0001-fynkel-2-code-first.md). Stage 10 is done
([report](stage-10-report.md)).

A Fynkel game today is silent. There is no sound effect, no music, no
footstep, no click when a button is pressed. Every game needs sound, and
a game without it can't ship. Stage 11 adds **sound, as Unity has it**:
clips, sources on objects, a listener, 3D sound that fades with distance
and moves from ear to ear, music that streams and crossfades, and volume
groups a settings screen can turn down.

Like physics, sound is **a package a game adds**, `package:fynkel_audio`,
with its own native library. A game without it ships no sound code and
runs exactly as now.

**The goal.** A game plays sounds in a line or two, and they behave the
way a player expects on a phone:

```dart
import 'dart:math';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';

class Dog extends GameObject {
  final _random = Random();
  late final AudioSource _voice;

  @override
  Future<void> onLoad() async {
    final model = add(await assets.instantiate('dog.glb'));
    _voice = addComponent(AudioSource(spatialBlend: 1, maxDistance: 30));
    final bark = await assets.audio('bark.ogg');
    final step = await assets.audio('step.ogg');
    // A footstep, a little different each time.
    model.animator!.at('run', 0.25, () => _voice.playOneShot(step, pitch: 0.9 + _random.nextDouble() * 0.2));
    on(input.onTap, (_) => _voice.playOneShot(bark));
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

**The road after it**: particles, animation 2 (cut down to 2D blends and
look-at and foot IK), then stage 12 shipping = 1.0. Agreed 2026-10-19:
sound moves ahead of animation 2.

## 1. What there is today

- **No sound at all**, in the engine, the packages or the games.
- **Where it plugs in.** Stage 5's `package:fynkel/extend.dart` lets a
  package take part in the frame: a `WorldSystem` the world runs at fixed
  points, and many objects' world transforms read in one call. Physics
  uses it; sound uses it the same way, to tell the mixer where each
  source and the listener are.
- **Time.** `world.paused` and `time.scale` exist; `FynkelView` already
  tells the world when the app goes to the background
  (`world.applicationPaused`).
- **A hook for footsteps.** Stage 9's `animator.at(clip, time, callback)`
  was made with this stage in mind.
- **Assets** load by name (`assets.load(name, kind)`), and a package can
  add its own kind, as shaders did in stage 10.

What Fynkel 1 taught (its `docs/audio.md` and `crates/fynkel_audio`,
read again for this plan):

- **Our own mixer on `cpal` was right.** About 400 lines over fixed
  tables, with lock-free rings both ways, so the audio thread never
  allocates, locks or frees. A test counted allocations inside the mixer
  and found none. `cpal` speaks CoreAudio on Apple and AAudio on Android.
- **Decisions on the game's side.** The game decides when a sound starts
  and ends; the audio thread only plays what it was told. Tests can run
  without a speaker and still know which sounds played.
- **Start and stop without clicks.** A short gain ramp (5 ms) on every
  start, stop and pause.
- **What it never had**: 3D sound (it was "later"), Android, the iPhone's
  silent switch and phone calls, and a mixer a settings screen could use
  simply. It was also heavy around the edges: components, schemas, cooked
  chunks, an editor page. Fynkel 2 needs none of that.

## 2. Unity's way, and Fynkel's

| In Unity | In Fynkel after stage 11 |
|---|---|
| Audio is built in. | A package, `fynkel_audio`: add it to `pubspec.yaml`; leave it out and nothing of it is built. |
| `AudioClip`, imported from WAV, OGG, MP3; load types *Decompress On Load*, *Compressed In Memory*, *Streaming*. | `AudioClip` from `assets.audio('coin.ogg')`: WAV, OGG Vorbis, MP3. Short clips are decoded when loaded; long ones (music) stream (`assets.audio(name, stream: true)`, and by default for clips over 10 s). |
| `AudioSource`: `clip`, `volume`, `pitch`, `loop`, `playOnAwake`, `mute`, `priority`, `panStereo`, `spatialBlend`, rolloff, `minDistance`, `maxDistance`, `outputAudioMixerGroup`. | The same, as a behaviour; `group` instead of the mixer group. |
| `Play`, `Pause`, `UnPause`, `Stop`, `PlayOneShot(clip, volumeScale)`, `PlayDelayed`, `isPlaying`, `time`. | The same, plus `playOneShot(clip, volume:, pitch:)`, since varying the pitch of repeated sounds is what every game does. |
| `AudioSource.PlayClipAtPoint(clip, position)`. | `audio.playAt(clip, position)`. |
| `AudioListener`, usually on the main camera; one at a time. | The same; without one, the world's camera listens. |
| `AudioListener.pause`, `AudioSource.ignoreListenerPause`. | Game sounds pause with `world.paused`; a source with `ignorePause` (menu clicks) doesn't. |
| `AudioListener.volume`. | `audio.volume`. |
| An `AudioMixer` asset with groups, exposed parameters, snapshots and effects. | `audio.group('music')`, groups made by name, each with `volume`, `mute` and `fade(to, seconds)`, under one master. No mixer asset, snapshots or effects (§5). |
| 32 real voices, priority 0 (most important) to 256. | The same. |
| Music: a looping `AudioSource`, crossfades written by hand. | `audio.music.play(clip, fadeIn:)`, `crossFade(clip, seconds)`, `stop(fadeOut:)`: one music player, because every game writes it. |

## 3. Design decisions

### 3.1 Two threads, no waiting

- **The game's side** (Dart, then a native call once a frame) decides
  everything: which sounds start, stop, pause, where each source and the
  listener are, every gain. It sends the frame's changes in **one
  buffer**, after the late updates, through the `WorldSystem`.
- **The audio thread** (`cpal`'s callback, on Rust's own thread as
  ADR-0001 says) mixes. It reads commands from a lock-free ring, never
  allocates, locks or frees, and sends finished voices and released clips
  back on a second ring to be freed elsewhere. A test counts it.
- **Positions are smoothed.** Each frame's positions and gains arrive at
  the frame rate; the mixer ramps to them over one callback, so a source
  moving fast doesn't crackle, at 30 fps or 120.
- **When a sound ended** is known to Dart (`isPlaying` goes false,
  `onEnd`) from what the audio thread reports back, read once a frame.

### 3.2 Clips

- **Formats**: WAV, OGG Vorbis and MP3, decoded with `symphonia` (pure
  Rust, already proven in Fynkel 1).
- **Decoded when loaded**, off the game's thread, for short clips: 32-bit
  floats at the clip's own rate, resampled while mixing. **Streamed** for
  long ones: decoded as they play, a second ahead, on a decoder thread.
- **Memory is said plainly.** `clip.bytes` and a total in the audio stats,
  because a decoded minute of stereo is about 21 MB.
- **Unloading**: a clip no longer referenced is freed after its last voice
  stops, never on the audio thread.

### 3.3 3D sound

- **`spatialBlend`** 0 (2D: same in both ears, for music and the UI) to 1
  (3D), as Unity.
- **Fading with distance**: Unity's logarithmic rolloff by default (full
  volume within `minDistance`, falling as 1/distance, and no lower past
  `maxDistance`, as Unity's is), and linear (silent at `maxDistance`).
- **Left and right** from where the source is around the listener,
  with equal-power panning. A sound just behind is a little quieter and
  duller, so front and back aren't the same.
- **Not in this stage**: Doppler, reverb zones, HRTF ("3D over
  headphones"), occlusion by walls (§5).
- **Seeing it**: `audio.drawSources = true` draws each 3D source's
  `minDistance` and `maxDistance` with the debug lines.

### 3.4 Phones

What players expect, and what a game shouldn't have to know about:

- **iPhone silent switch.** By default the game's sound follows it, and
  the player's own music (Spotify, Apple Music) keeps playing under the
  game. `audio.session = AudioSession.playback` changes both: sound plays
  with the switch on, and the player's music stops. (Unity's *Mute Other
  Audio Sources* setting.)
- **Phone calls, alarms, Siri** interrupt the sound; it comes back after.
  On Android the same through audio focus.
- **The app in the background** stops the output and frees the device;
  it comes back where it was. A paused game (`world.paused`) pauses its
  game sounds.
- **Headphones or Bluetooth** plugged or unplugged: the sound moves to the
  new output without stopping the game.
- **Latency**: on Android, `cpal`'s AAudio output in its low-latency
  mode. Measured from a tap to the sound (§6).
- These are done in the package's native library (AVAudioSession on
  Apple, through Objective-C calls from Rust; audio focus on Android, as
  the engine already calls Java), so a game adds no platform code.

### 3.5 Tests and a game without a speaker

- **Without a device** (`dart test`, CI), the mixer runs on a "null"
  output: the same voices, the same ends, the same counts, rendered on
  demand. A test can ask what's playing, and render a second of the mix
  to compare against a reference ("audio goldens": a short WAV, with a
  tolerance).
- **Never opened by tests**: the device opens when a `FynkelView` shows
  a world that has sound, and closes with it.

## 4. Phases

Each phase is one PR or a few, with its lab scene, its guide section, its
tests, and plain words. A phase is done when its PRs are merged with CI
green and it's been heard on the Mac, the iPhone and the Note10+.

### Phase 1: a sound plays

- `crates/fynkel_audio` (the mixer, clips, decoding, the null and device
  outputs), `crates/fynkel_audio_ffi`, `packages/fynkel_audio` with its
  build hook, as physics is.
- `AudioClip` (`assets.audio`), decoded when loaded.
- `AudioSource` in 2D: `play`, `pause`, `unPause`, `stop`,
  `playOneShot(clip, volume:, pitch:)`, `volume`, `pitch`, `loop`,
  `playOnAwake`, `mute`, `isPlaying`, `time`, `onEnd`; a disabled or
  destroyed source stops.
- 32 voices with priority; clicks-free starts and stops.
- Pausing with `world.paused` and `ignorePause`; the app in the
  background; the iPhone's silent switch and interruptions; Android's
  audio focus.
- The lab: "sound, the basics": buttons that play, loop, pause, change
  the pitch.

*Done when* the lab's sounds play on all three devices; the mixer's
allocation test finds none; a sound's end is known in `dart test` without
a device; and a world without sound is unchanged.

### Phase 2: 3D sound

- `AudioListener`, the camera's by default; `spatialBlend`, rolloff,
  `minDistance`, `maxDistance`, `panStereo`, panning.
- `audio.playAt(clip, position)`.
- Positions sent once a frame for every playing 3D source (one call),
  smoothed in the mixer.
- `audio.drawSources`.
- The lab: a sound circling the camera, and one you walk towards.

*Done when* the audio goldens of a source left, right, near and far
match; a fast source doesn't crackle (checked in its golden); and 32
moving 3D voices cost the frame less than 0.1 ms on the Note10+.

*As built* (branch stage-11-3d, sound ABI 3):

- The mixer gets each voice's four gains (each side of the clip to each
  ear) and how dull it is, and slides to them across one callback; a
  one-pole low pass at 5 kHz makes behind duller, and behind is 30%
  quieter. Equal-power panning, the centre at full volume in both ears.
- Logarithmic rolloff stops falling at `maxDistance` rather than going
  silent, as Unity's does (the plan said silent); linear is silent there.
- Each frame, after the cameras move, one `readWorldTransforms` reads
  every playing 3D source and the listener, and one `fa_audio_place`
  sends them. The listener is the last enabled `AudioListener`, else the
  world's camera, else the origin looking along -Z.
- Goldens: crates/fynkel_audio/tests/goldens/spatial_{left,right,near,
  far,behind,fly_past}.wav. Without the slide, the fly-past jumps 0.098
  between samples; with it, under 0.035.
- The Note10+, 32 bees circling (the lab's swarm): 0.086–0.102 ms a
  frame, of which about 0.035 ms is reading the camera, which sends the
  frame's moves to the engine early (drawing would send them anyway).
  Sound's own share is about 0.05 ms. The 0.1 ms bar is met for sound's
  share, and is at the edge counted the stricter way.
- The guide's §13b has the 3D part; the dart allocation tests play,
  move, turn and playAt with no garbage.

### Phase 3: music and the mix

- Streaming clips (`stream: true`, and clips over 10 s), a second ahead.
- `audio.music`: `play(clip, fadeIn:)`, `crossFade`, `stop(fadeOut:)`,
  looping by default.
- Groups: `audio.group(name)` with `volume`, `mute`, `fade(to, seconds)`,
  under `audio.volume`; a source's `group`.
- Hot reload re-reads a changed clip.
- The lab: a settings panel with music and effects sliders over music
  that crossfades between two tracks.

*Done when* a streamed track plays for 10 minutes on each device without
a gap (counted: no starved callbacks); a crossfade's golden matches; and
a group's volume reaches its voices within one callback.

*As built* (branch stage-11-mix, sound ABI 4):

- A streamed clip keeps its file. Each voice that plays it gets its own
  decoder and a ring of a second of frames, filled by one decoder thread
  that wakes every 20 ms (and at once for a new voice); without a device
  it's filled between the mixer's chunks, so tests hear the same. A
  start far in seeks a fifth of a second early and decodes up to it, as
  OGG and MP3 seeks land near rather than on; wav, ogg and mp3 streamed
  match the same clip decoded to within 1e-6, from the start, from 2.1 s
  in and looping (tests/stream.rs). A clip whose length the file doesn't
  say is decoded until it passes 10 s, then streamed.
- A stream that isn't ready holds where it is (silence, not a skip) and
  counts in `stats.starved`. A looping stream told to stop looping ends
  at the end of the time round it's in, though its decoder is ahead.
- Fades and groups are exact per sample, a straight line, there by the
  end of their seconds (0 seconds: by the end of the next callback, so
  there's no click). A voice faded out to its stop ends when it reaches 0.
  `audio.music` is a voice in group `music` at priority 0, ignoring the
  pause unless told; a crossfade fades the old voice out to its stop and
  the new one in. Golden: crates/fynkel_audio/tests/goldens/crossfade.wav.
- Groups are flat, 15 of them, each under `audio.volume` (no group in a
  group: a game's settings screen doesn't need it).
- Fades and group volumes are passed to the native library through
  memory, as a double passed to a call is an allocation in Dart's JIT;
  the allocation test crossfades streamed tracks, moves sliders, mutes
  and fades with no garbage.
- Hot reload: `World.reloadAssets()` (from `FynkelGame.reassemble`) reads
  again the files of every asset kind that reloads; a clip whose file
  changed takes its new sound from its next play.
- Android's audio focus (left from phase 1): `AudioSession.playback`
  asks for it through JNI, so the player's music stops; `ambient` gives
  it up. A call ringing or on (AudioManager's mode, read every half
  second) holds the device, as the background does. Losing the focus to
  another app isn't heard (it needs a Java listener).
- The lab's "Sound: music and the mix" is the settings screen: master,
  music and effects sliders, mutes, a duck, fades, over two tracks of
  ours (examples/lab/tool/music.py, 24 s and 16 s, streamed), with the
  starved count in the readout.
- Ten minutes streamed, 2026-10-22: the Mac (`cargo test -p fynkel_audio
  --test device -- --ignored a_streamed`, chase looping) 56,251
  callbacks, none starved or underrun; the iPhone 16 Pro Max and the
  Note10+ (the lab's scene with `--dart-define=LAB_SOUND=true`, a
  crossfade every 30 s) 600 s each, none starved or underrun.

### Phase 4: the games, and measuring

- **Sheepdog**: barks, sheep, footsteps on `animator.at`, a whistle,
  music, UI clicks, a settings screen with volume. **Coins**: pickups and
  music. **Bowling**: rolling and pins hit by how hard (from
  `onCollisionEnter`'s speed).
- Sounds and music from CC0 packs (Kenney's audio packs and CC0 music),
  committed with their credits.
- The measurements (§6), the guide's §13b "Sound" and §22's Unity rows,
  the report.

*Done when* the three games have sound and run as fast as before on all
three devices, and §6's bars are met.

## 5. Not in this stage

Effects on the sound (reverb, echo, low-pass for "under water" or a
muffled pause menu), reverb zones, Doppler, HRTF, sound blocked by walls,
mixer snapshots and ducking, the microphone, recording, MIDI, and
synthesising sound in code. They go in the guide's §21. A low-pass on a
group is the first I'd add if a game asks.

## 6. Measurements

On the Mac, the iPhone 16 Pro Max and the Note10+ (profile builds):

- **No fps lost**: Sheepdog with sound at each quality level runs as it
  does now.
- **The frame's cost**: the sound's part of the frame under 0.1 ms with
  32 voices playing, 16 of them 3D and moving.
- **The audio thread**: under 10% of one core at 32 voices on the
  Note10+; no underruns in a 10-minute run.
- **Latency**, a tap to the sound: under 50 ms on the iPhone; measured
  and said on the Note10+ (Android's own floor is often higher).
- **Size**: what the package adds to the app, said per platform.
- **Memory**: decoded clips counted against what the game loads.

## 7. Decisions to make

1. **Our own mixer on `cpal`**, as Fynkel 1 (*recommended*: small, read
   whole, no allocation on the audio thread, counted), rather than
   `kira` (a good Rust mixer, but its audio thread is its own business)
   or `flutter_soloud` (a Flutter plugin over the C++ SoLoud: quick to
   start, but a second native engine with its own clock beside ours, and
   its own idea of 3D).
2. **A package**, `fynkel_audio`, like physics (*recommended*), rather
   than in the core.
3. **WAV, OGG Vorbis and MP3** (*recommended*); not AAC/M4A, which needs
   a decoder we'd rather not carry. A game converts with any tool.
4. **On iPhone, the silent switch mutes the game and the player's music
   keeps playing** by default (*recommended*: what most casual games do,
   and what players expect), with `AudioSession.playback` to change it.
5. **A paused game pauses its sounds**, and `time.scale` doesn't change
   pitch unless a source asks (`followTimeScale: true`) (*recommended*;
   Unity doesn't change pitch with time either).
6. **A music player built in** (`audio.music`) (*recommended*: every game
   writes one), not only sources.
7. **Sounds and music from CC0 packs** for the games, committed with
   credits (*recommended*).
