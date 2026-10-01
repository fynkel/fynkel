# Audio

The audio contract: blueprint §9 "Audio" (Fynkel concepts, never backend
types; nothing blocking in the callback), §19.2d item 11 (M3-11) and the
scope rule that decided the backend. `fynkel_audio` is the
`AudioSource`/`AudioListener` components, a voice engine the core steps
inside the fixed tick, a mixer that renders those voices over three buses
inside a real-time callback, a decoder thread per streamed voice (music
decoded as it plays), and two backends: **null** (the mixer is
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
  bus / mute / listener gains                          a voice that ended,
  started / stolen / refused / ended                    a stream's tap)
                                                            ▲ rtrb ring, 1 s
                                              decoder thread per streamed voice
                                              (symphonia over the STRM bytes)
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

**An optional module** (ADR-0029). A project can turn audio off
(`modules.audio: false` in `fynkel.project.json`, Project ▸ Modules… in the
editor). Then the system is skipped, its commands are rejected with
`MODULE_OFF` (`FYN-R0014`), `AudioSource` and `AudioListener` in a scene or prefab are
`FYN-P0018`, and from Dart `world.audio.play`, `setBus` and `GameObject.audio` (`applySettings` keeps the volumes quietly) throw a `StateError` naming the module
(`world.modules.has(Module.audio)` answers first). With `modules.strip: true`
a runtime built from source also leaves out the mixer, rtrb, symphonia and cpal; the types stay, so the
contract is the same in every build.

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
suspended, streams, starved }` (64 B) — the engine at a glance; `streams`
is how many of the clips are streamed, `starved` the callbacks a
streamed voice's decoder fell behind in (§ Streamed music).

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
  `WindowEvent::Occluded`, closes on close and prints the engine's
  counters at the end (voices, clips and streams, callbacks, underruns,
  starved) — the way to hear a cooked scene without a Flutter app.
- **`hosts/headless`** never opens one; its report carries `audio:
  { sources, sounds, streams, started, stolen, refused, ended, diagnostics }` and
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
world.audio.setBus(AudioBus.music, 0.4, muted: false);    // publishes at transaction commit, no tick
world.audio.applySettings(settings);           // all gains + global mute in one transaction
world.audio.busVolume(AudioBus.music); world.audio.muted; // logical configuration
world.audio.stats;                             // AudioStats: device, voices, started, stolen, refused, ended, clips, streams, starved, …
world.audio.open();                            // what FynkelGameView(audio: true) does; false + `audio_device` diagnostic without one
world.audio.suspend(true); world.audio.close();

// Components, like any other:
gameObject.set(gameObject.get<AudioSource>()!.copyWith(looping: true));
gameObject.add(const AudioSource(clip: hum, bus: AudioBus.music, looping: true, playing: true));
```

Presentation `AudioSystem.play` flushes at once outside gameplay edits.
`setBus` and `applySettings` use the native gameplay transaction: successful
commit publishes configuration without a tick, and failure rolls it back.
Mute is global; the last bus write sets it. Device delivery remains asynchronous.
`busVolume` and `muted` read current logical configuration, including staged
writes. Standalone calls enter the usual preceding-queue adoption boundary.

`SettingsStore.apply` calls `applySettings` before input rebinding. Store document
changes and saved files are external state and are not undone by gameplay
rollback. Audio and input changes share one commit when store application runs
inside a callback or `World.edit`; standalone store application performs them
separately. Audio-only `applySettings`
is supported inside callbacks and remains a no-op when audio is disabled. `LoadReport.audio` has `sounds` (registered
from the scene), `clips` (registered in all), `streams` (of those, the
streamed), `device`, `sampleRate`, `channels`, `maxVoices`.
`DiagnosticCode.soundUnavailable` / `audioDevice`.

The starter (`fynkel create`) is the worked example: the Cube's
`fynkel.AudioSource` holds `assets/audio/jump.ogg` and the Mover plays it
on take-off (`audio.play()`), the Camera carries the listener, and
`land.ogg` — an `@Expose`d `AssetRef land` on the Mover, set in the
scene — plays on landing through `world.audio.play(land)`;
`test/game_test.dart` replays `walk.input.json` and counts 4 voices
started and ended and 2 `PlaybackEnded` on the cube, on the null backend.
Both clips are Kenney's (CC0, www.kenney.nl).

The reference game (`examples/reference_game`, its README § Credits) is
the acceptance: content the implementer did not make — Kenney's
footsteps, pickup and hit, Juhani Junkala's "Level 1" as the streamed
music (CC0 all) — a listener on the Camera, a looping `Music` source on
the music bus from the load (on the Rust host too), `game.Player`
playing a footstep per `step_l`/`step_r` marker (the file by the
marker's name, a little pitch drift) and the hit on a death,
`game.Collectible` its pickup; `test/game_test.dart` counts the voices
against the markers and checks `starved` stays 0.

## Import and the cache

`assets/audio/*.wav|*.ogg` with a `.meta.json` sidecar (`fynkel asset
init`, importer `fynkel.audio` v2): WAV (PCM 8/16/24/32-bit and float)
and Ogg Vorbis through `symphonia`, nothing else (`FYN-A0103`). The cook
writes a `CLIP` chunk — the decoded PCM as the file had it: no
resampling, no channel mixing, no normalisation — which the runtime
registers whole. Mono and stereo only; a clip with more channels is
refused at registration. `max_audio_frames` is 10 minutes at 48 kHz.

## Streamed music

A sidecar with `"settings": { "stream": true }` (the one importer
setting; anything but a boolean is `IMPORT_INVALID_DATA` at
`/settings/stream`) makes the cook keep the **source bytes** instead of
the PCM: the import still decodes the whole file — the validation and
the frame count are the same as a clip's — then drops the samples, and
the cook writes a **`STRM`** chunk (`fynkel_cooked::KIND_STREAM`:
`sample_rate`, `channels`, `frames`, the `codec` name, the file) where
a `CLIP` would go; the `META` chunk info gains `streamed: true` and
`bytes`. A 74 s stereo Vorbis track is 1.4 MB in the cache and in memory
instead of 26 MB of floats. `Assets::sound` registers whichever chunk
the object has; the world tells them apart only by `AudioClip::is_stream`.

Playing one is the same contract as a clip — the source's `play`,
`stop`, `pause`, `looping`, `pitch`, the clock, `PLAYBACK_ENDED` by the
tick, the stats — with the decoding moved off both threads:

- **Start.** When a streamed voice starts, the engine spawns a decoder
  thread (`fynkel-audio-stream`, `fynkel_audio::stream::spawn`) for that
  voice with the clip's bytes (an `Arc<[u8]>`, shared, never copied), the
  start frame and a `StreamShared` of atomics (stop, looping, finished,
  failed, frames produced), and hands the mixer a `StreamTap`: the
  consumer end of an `rtrb` ring holding `STREAM_RING_SECONDS` (1 s) of
  interleaved frames at the clip's rate. The decoder seeks to the start
  frame (`SeekMode::Accurate`, the odd frames before the requested one
  skipped), fills the ring, and sleeps 5 ms whenever it is full.
- **Render.** The mixer pulls frames from the tap instead of the clip
  table, through the same resampler, gain smoother and buses; the voice
  is otherwise an ordinary voice (stealing, priorities, pause). Nothing
  is allocated or locked in the callback (`tests/allocation.rs` renders
  a streamed voice too: 121 callbacks, zero allocations).
- **Loop, end, stop.** `looping` is mirrored into the shared flags: at
  the end of the file the decoder reopens from frame 0 and goes on, so a
  loop has no gap. A stop, the clip running out or a decode error ends
  the decoder, which marks the stream `finished`; the tap plays out
  what is left in the ring and then reports the voice ended (its last
  frame included). The **tick's clock** still decides when a non-looping
  streamed voice is over, from the frame count the cook wrote — so the
  headless run and the device run agree, as for a clip; the decoder's
  end only stops the mixer from waiting.
- **Starvation.** A ring found empty after it was first primed, while
  the decoder has not finished, means the machine could not keep one
  second ahead: the voice holds silent for the rest of that callback and
  `MixerStats::starved` counts one per voice per callback, surfaced as
  `AudioStats.starved` (the count survives a device reopen). A steady
  count is the signal the lead is too short for the platform; 0 on a
  healthy machine.
- **Device open / close.** The tap lives with the mixer, so opening or
  closing the device (`resend_state`) respawns each streamed voice's
  decoder at the voice's clock position; the freed tap comes back on the
  return ring and is dropped on the world thread (`Return::StreamReleased`;
  a full return ring leaks it rather than freeing on the audio thread).
- **Clips.** `AudioStats.streams` counts the streamed clips registered.
  A stream costs a thread and 1 s of ring while it plays (nothing while
  it does not); the clip table holds the bytes only.

`crates/fynkel_audio/src/stream.rs` has the decoder and the tap with
their unit tests (start position, loop wrap, stop, a bad source);
`tests/engine.rs` plays a streamed clip through the engine: ended by the
clock with `starved == 0`, looped past four lengths, stopped to silence,
respawned across a device close.

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
accurate scheduling against the device clock, a streamed clip's decoder
shared between voices (each streamed voice decodes on its own), a
configurable ring length.
