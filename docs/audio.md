# Audio

The audio contract: blueprint §9 "Audio" (Fynkel concepts, never backend
types; nothing blocking in the callback), §19.2d item 11 (M3-11) and the
scope rule that decided the backend. `fynkel_audio` is the
`AudioSource`/`AudioListener` components, a voice engine the core steps
inside the fixed tick, a mixer that renders those voices over three buses
inside a real-time callback, and two backends: **null** (the mixer is
rendered on demand — a test, the headless host — or not at all) and
**device** (`cpal`, behind the crate's `device` feature). Tier 1 is 2D:
nothing is panned or attenuated by distance (spatial audio is M5).

**Backend decision (M3-11, the scope rule):** `cpal` 0.18 for the device
and Fynkel's own mixer, not `kira`. `kira` would have given a mixer, buses
and streaming for free but owns its callback: what it allocates, decodes
or locks there is its business, and the gate for this item is "no
allocation in the audio callback", counted. The mixer here is ≈400 lines
over fixed tables, and the world side keeps every decision (which voice
plays what, when it ends) so a headless run and a device run agree.

## Two halves, two threads

```
world thread (the tick)                      audio thread (the device callback)
──────────────────────                       ─────────────────────────────────
fynkel_audio::Audio                          fynkel_audio::Mixer
  clip registry (AssetRef → slot)   ──LoadClip──▶  clip table [MAX_CLIPS = 256]
  voices [MAX_VOICES = 32]:         ──Start/Stop/──▶ voice table: position, gain
    priority, seq, position (frames)  Pause/SetGain     smoother, rate ratio × pitch
  the clock: advance(dt) per tick    ◀──Return────   (a released clip's Arc,
  bus / mute / listener gains                          a voice that ended)
  started / stolen / refused / ended
```

Two lock-free rings (`rtrb`) join them: commands go world → mixer, and
what the mixer is done with comes back so that nothing is freed on the
audio thread. The mixer allocates nothing: fixed tables, a one-pole gain
smoother (`GAIN_SMOOTHING_SECONDS` 5 ms) that also declicks starts,
stops and pauses, linear-interpolation resampling at the clip's rate over
the device's, times the voice's pitch. `crates/fynkel_audio/tests/allocation.rs`
wraps a render in a counting global allocator — clips loading, voices
starting, stopping, looping, resampling, being stolen, a full ring pumped
inside the render — and asserts zero.

**The clock is the tick's, not the device's.** `Audio::advance(dt)` moves
every voice by `dt × pitch × sample_rate` frames and reports the
non-looping voices that reached their end; the mixer renders in real time
from the same start positions. The world decides that a voice ended at
tick *n* — deterministic, so `fynkel test --headless` reports the same
`PLAYBACK_ENDED` ticks as the app; the device only renders what the
tick decided. The two drift by at most a callback buffer; nothing depends
on the device's clock.

## Components

Both are ordinary native components (`fynkel_core::schema::ALL`, the
generated codecs, `set`/`get` from Dart), authored in scenes.

**`fynkel.AudioSource`** (id 23) — a clip with its playback settings on
an entity:

| Field | Type | Rule |
| --- | --- | --- |
| `clip` | `AssetRef` | an audio source's asset id (`.wav`/`.ogg`; no sub-asset) |
| `bus` | `master` \| `music` \| `sfx` | default `sfx` |
| `volume` | number | linear gain `0..=1` |
| `pitch` | number | rate multiplier `0.05..=8`; `1` is the clip's own rate |
| `looping` | bool | |
| `playing` | bool | authored `true` starts the clip at the load; the world clears it when a non-looping clip ends |
| `priority` | int | who keeps a voice at the limit (below) |

**`fynkel.AudioListener`** (id 24) — `volume` `0..=1`, a gain over every
voice: the ears, where spatial audio (M5) will measure from. One per
world: the lowest-numbered entity with one is the listener; without one
the world plays at gain 1.

`fynkel validate` checks the values (`FYN-H0010`: a gain outside `0..=1`,
a pitch outside its range); the runtime rejects the same with
`INVALID_PAYLOAD` at the tick. `fynkel.project.json` lists both under
`components` as `native` like every other component a scene uses.

## The tick (§5.1)

The audio phase (`crates/fynkel_core/src/world/audio.rs`, `SystemKind::Audio`)
runs after transforms and before publish — the last phase of the tick, so
it sees every `playing`/`volume` write the tick's commands and behaviours
made. It reconciles every `AudioSource` with the voice the engine plays
for it (an `AudioRun` beside the component):

- `playing: true` without a voice **starts** the clip (from the scene's
  load, a `SetComponent`, or the `Audio` command's `play`); `playing:
  false` with a voice **stops** it;
- a changed `volume`, `pitch` or `looping` reaches the voice; a changed
  `clip` restarts with the new one;
- a removed component or a destroyed entity stops the voice;
- then the clock advances one tick, and every non-looping voice that
  reached its end clears its source's `playing` and raises
  **`PLAYBACK_ENDED`** (event kind 11, `{ entity, clip }`, general lane,
  the tick it happened). A stopped or stolen voice raises nothing.

The listener's gain is read every tick (a behaviour writing
`AudioListener.volume` fades everything). The source's `playing` is
authoritative: a source that lost its voice to a steal reads `false` the
same tick.

**Voice limit.** 32 voices. A new sound past the limit steals the voice
of the lowest-priority sound at or below its own priority (the oldest
among equals) and is refused when every voice outranks it; `started`,
`stolen`, `refused` count in the stats. A source whose voice was stolen
stays authored as it was (`playing` cleared); a `PlaySound` refused makes
no diagnostic (`refused` counts).

**Clips.** The world knows a clip once it was `register_sound`ed: the
loaders register every clip a loaded scene's or prefab's `AudioSource`s
name (`Assets::register_audio`, from the cooked `CLIP` chunk — decoded
interleaved `f32` PCM, `sample_rate`, `channels` 1 or 2), and the native
world registers from the cache, before the batch, the clip a `PlaySound`
or a `SetComponent(AudioSource)` names (`register_animation_assets`, the
same path an `Animator`'s clip takes) — so `world.audio.play(anyClip)`
from Dart works for any cooked audio asset, and the load report's
`audio.sounds` counts only the scene's. A source with a clip the world
has not been given reports `SOUND_UNAVAILABLE` (18) once per clip per
source and keeps `playing`, so a later registration starts it; a
`PlaySound` with one is rejected with `SOUND_UNAVAILABLE` every time. 256
clip slots; the 257th registration fails.

## Commands, the queries, the stats

Simulation lane, on a `fynkel.AudioSource` — **`fynkel.cmd.Audio`**
(kind 10) `{ entity, op, value }`, `op` ∈ `play` (from the beginning; sets
`playing`), `stop` (clears `playing`, no event), `pause`, `resume`,
`setVolume` (`value` `0..=1`), `setPitch` (`value` `0.05..=8`). Rejected
at the tick with `MISSING_COMPONENT` (no source) or `INVALID_PAYLOAD`;
`WRONG_LANE` on the presentation lane.

Either lane — **`fynkel.cmd.PlaySound`** (11) `{ clip, bus, volume,
pitch, priority }`: a fire-and-forget sound on no entity — no handle, no
`PLAYBACK_ENDED`; and **`fynkel.cmd.AudioBus`** (12) `{ bus, volume,
muted }`: a bus gain and the mute. `master` scales the other two, `muted`
silences everything, and both survive scene loads. On the presentation
lane they apply at submit (a menu's volume slider needs no tick).

**`FYNKEL_QUERY_AUDIO`** (12) answers a source's state as the last step
left it, `fynkel.rec.AudioState { entity, playing, paused, time,
duration, volume, pitch, looping, clip, bus, voice }` (`voice` is
`generation << 8 | slot`, 0 without one; `duration` 0 while the clip is
unknown); no record without a source. **`FYNKEL_QUERY_AUDIO_STATS`** (13)
answers `fynkel.rec.AudioStats { device, sample_rate, channels, voices,
max_voices, started, stolen, refused, ended, clips, callbacks, underruns,
suspended }` — the engine at a glance.

## The device and the host lifecycle

A world starts on the null backend: the same clock, the same counters,
nothing rendered unless someone calls `Audio::render`. A host that plays
opens the device once the project is loaded:

- **C ABI** — `fynkel_world_audio_open(world)` → `Ok` (the device's format
  is in the stats) or `Unsupported` (9: the platform has no output, or
  the build carries no device backend; an `AUDIO_DEVICE` (19) diagnostic
  says why and the world keeps the null backend); `fynkel_world_audio_suspend(world,
  bool)`; `fynkel_world_audio_close(world)`. Opening resends the clips and
  the mix state to the new mixer, so voices already playing continue at
  their positions; closing returns to a null mixer the same way.
- **Suspend** pauses the device stream (the app in the background) and
  resumes it; the engine's clock is the tick's, so game time is not
  affected (a suspended app usually stops ticking anyway).
- **`package:fynkel_flutter`** — `FynkelGameView(audio: true)` opens the
  device after the first load, suspends it while the app is hidden or
  paused, closes it on dispose. Default `false`: a game opts in, a test
  never does.
- **`hosts/standalone`** — `--audio` opens the device (`audio: output
  device open at 48000 Hz, 2 channels` or the diagnostic), suspends on
  `WindowEvent::Occluded`, closes on close.
- **`hosts/headless`** never opens one; its report carries `audio:
  { sources, sounds, started, stolen, refused, ended, diagnostics }` and
  `fynkel test --headless` prints the line when the scene has any.

The Apple hosts need nothing new: `cpal` uses CoreAudio, and
`libfynkel_platform_apple.dylib` already links AVFoundation, AudioToolbox
and CoreAudio (the iOS build was verified with the feature on).

## From Dart

```dart
// The object's own source (a fynkel.AudioSource on it), simulation lane:
audio.play();                 // also stop(), pause(), resume(), volume = 0.5, pitch = 1.2
final s = audio.state;        // AudioState? — playing, paused, time, duration, progress, voice
if (audio.isPlaying) …

@override
void onPlaybackEnded(PlaybackEnded e) { … }   // the source's non-looping clip reached its end

// The world:
world.audio.play(clip, bus: AudioBus.sfx, volume: 0.8, pitch: 1.1, priority: 2);   // one-shot, at the next tick
world.audio.play(click, presentation: true);   // now — a menu sound outside the tick
world.audio.setBus(AudioBus.music, 0.4, muted: false);    // at once (presentation lane)
world.audio.applySettings(settings);           // master/music/sfx volumes + mute from a SettingsDocument
world.audio.stats;                             // AudioStats: device, voices, started, stolen, refused, ended, clips, …
world.audio.open();                            // what FynkelGameView(audio: true) does; false + `audio_device` diagnostic without one
world.audio.suspend(true); world.audio.close();

// Components, like any other:
gameObject.set(gameObject.get<AudioSource>()!.copyWith(looping: true));
gameObject.add(const AudioSource(clip: hum, bus: AudioBus.music, looping: true, playing: true));
```

`AudioSystem.play` and `setBus` on the presentation lane flush at once
(the presentation batch is cleared at every `beginFrame`, so a command
staged outside a frame would otherwise be dropped). `SettingsStore.apply`
calls `applySettings` before it rebinds input, so the saved
`master_volume`/`music_volume`/`sfx_volume`/`muted` reach the buses at
launch and after every change. `LoadReport.audio` has `sounds` (registered
from the scene), `clips` (registered in all), `device`, `sampleRate`,
`channels`, `maxVoices`. `DiagnosticCode.soundUnavailable` /
`audioDevice`.

The starter (`fynkel create`) is the worked example: the Cube's
`fynkel.AudioSource` holds `assets/audio/jump.ogg` and the Mover plays it
on take-off (`audio.play()`), the Camera carries the listener, and
`land.ogg` — an `@Expose`d `AssetRef land` on the Mover, set in the
scene — plays on landing through `world.audio.play(land)`;
`test/game_test.dart` replays `walk.input.json` and counts 4 voices
started and ended and 2 `PlaybackEnded` on the cube, on the null backend.
Both clips are Kenney's (CC0, www.kenney.nl).

## Import and the cache

`assets/audio/*.wav|*.ogg` with a `.meta.json` sidecar (`fynkel asset
init`, importer `fynkel.audio`): WAV (PCM 8/16/24/32-bit and float) and
Ogg Vorbis through `symphonia`, nothing else (`FYN-A0103`). The cook
writes a `CLIP` chunk — the decoded PCM as the file had it: no
resampling, no channel mixing, no normalisation — which the runtime
registers whole. Mono and stereo only; a clip with more channels is
refused at registration. `max_audio_frames` is 10 minutes at 48 kHz.

Streamed music — a long clip decoded on its own thread into a ring
instead of held in memory — is part 2 of this item.

## Cost

The phase is one query over the sources (only the changed fields are sent
to the mixer) plus the clock over at most 32 voices: the starter's one
source costs ≈3 µs a tick; the fixture project's, the same. The mixer's
cost is per voice per frame — 32 voices at 48 kHz stereo is well under a
millisecond per 10 ms buffer on this Mac — and it never blocks: the rings
are single-producer/single-consumer, the tables fixed.

## Not in V1

Spatial audio (panning, distance attenuation, the listener's transform),
effects and sends, per-bus limiters, more than three buses, a voice
handle for one-shots, clip pooling beyond the 256 slots, Android's
`oboe`/AAudio path (cpal's Android backend is untested here), sample-
accurate scheduling against the device clock.
