# fynkel_audio: a sketch, to test extend.dart as a seam

Stage 6, step 3 (docs/stage-6.md §1, "extend.dart as the way in for
packages"). The question: could a second package plug into a world the way
fynkel_physics does (docs/stage-5.md §3.1), without changing the core?

What is here (`/tmp/s6/audio-sketch`, a throwaway package with a path
dependency on `packages/fynkel`; the repository was not changed):

- `lib/src/audio.dart`: `Audio` (a `WorldSystem`), `AudioSource`,
  `AudioListener`, `AudioClip`, and the `audio` getters on objects,
  behaviours and the world, written against `package:fynkel/fynkel.dart` and
  `package:fynkel/extend.dart` only. Each workaround is marked `GAP n`.
- `lib/src/backend.dart`: the native side behind an interface,
  `AudioBackend`.
- `test/audio_test.dart` + `test/fake_backend.dart`: 7 tests on a headless
  `World` (guide §17) with a fake backend. **They compile, analyze clean, and
  pass** (`dart test`, about 7 s including the build hook, whose cargo build of the engine was already cached).

The tests show: a 3D source on a child of a moving car is placed where the
child's world transform is, every frame; the listener follows the camera
object; nothing is read while no transform changed; `time.scale = 0` pauses
the game's sources but not one with `ignoreListenerPause`, and 1 resumes
them; `audio.paused` (Unity's `AudioListener.pause`); hiding the object
stops it and showing it again plays it (`playOnAwake`); destroying it stops
it; `world.dispose()` frees clips and the backend; two cars load one clip
once. Two tests exist only to pin gaps: the app-background flag has to be set
by hand, and `world.assets.preload(['x.wav'])` throws.

## 0. The design in one page

| Unity | fynkel_audio | How it hangs on the core |
| --- | --- | --- |
| `AudioSource` | `class AudioSource extends Behaviour`: `clip`, `play/stop/pause/unPause`, `isPlaying`, `volume`, `pitch`, `loop`, `playOnAwake`, `spatialBlend` (0 = 2D, 1 = 3D), `minDistance`, `maxDistance`, `ignoreListenerPause` | Behaviour lifecycle: `onEnable` registers (and plays if `playOnAwake`), `onDisable` stops and unregisters, as Unity stops a disabled source. Hidden object, removed component, destroyed object all arrive as `onDisable`. Same pattern as `Collider`. |
| `AudioListener` | `class AudioListener extends Behaviour`, put on the camera | `onEnable`/`onDisable` set the system's listener. With none, `world.camera` (public) is the listener. |
| `AudioClip` | `await audio.clip('engine.wav')`, loaded once per world, shared | Its own cache in the system (GAP 1–3: the core's `Assets` can't take it). |
| `AudioListener.volume` / mixer | `audio.volume` (master); groups/buses later | System state, sent to the backend. |
| `AudioListener.pause` | `audio.paused`, plus `audio.pauseWithTime` (default on: `time.scale == 0` pauses too) | Polled in `afterFrame` (GAP 4). |
| `OnApplicationPause` | `audio.applicationPaused` | Set by hand today (GAP 5). |
| 3D positioning | Every frame, one `world.readWorldTransforms` for every playing 3D voice plus the listener, one `backend.placeVoices` call | `afterFrame`, skipped while `world.transformVersion` is unchanged. |
| Access | `audio` on objects/behaviours, `world.audio` | `world.system(Audio._)`, made on first use, as `physics`. |

The frame: `beforeFrame` notices voices that played to their end;
`afterFrame` applies pausing and places voices and the listener. It is
`afterFrame`, not `afterLateUpdate`, because the camera rigs and virtual
cameras move the camera object after `afterLateUpdate` (world.dart step 7):
the listener must be read where the frame draws from. Nothing runs in the
fixed step; audio is per frame. Playing is fire-and-forget: the mixing runs
on the backend's audio thread; Dart only sends commands.

## 1. The extend.dart hooks it uses

| Hook / API | Used for |
| --- | --- |
| `WorldSystem` + `world.system<T>(make)` | One `Audio` per world, made lazily by the first `AudioSource`/`AudioListener`/`audio.clip`. A game never sets audio up. |
| `beforeFrame` | Polls the backend for voices that ended (a non-looping clip finished), so `isPlaying` is right in this frame's updates. |
| `afterFrame` | Pausing with the game (reads `world.time.scale` and `audio.paused`), then 3D placement of voices and the listener, after the cameras moved. |
| `dispose` | Stops every voice, frees every clip, frees the backend and the ffi buffers. Needed because `World.dispose` gives behaviours no `onDisable`/`onDestroy`. |
| `world.readWorldTransforms(ids, n, out)` | One call for all 3D voices' objects and the listener: world position + rotation (orientation of the listener; cones later). |
| `world.transformVersion` | Skip the read when no transform, parent or layer changed. |
| `world.report(...)` | Not needed yet; for a future "clip finished" callback into game code. |
| Not used: `beforeFixedUpdate`, `afterFixedUpdate`, `beforeUpdate`, `afterLateUpdate`, `componentAdded`, `transformStamp`, `componentCount/componentAt`, `hierarchyVersion` | Audio has no step, no mixins to discover, and few enough sources that per-object stamps aren't worth it. |
| Also used, from the public API | `world.camera` (listener fallback), `world.time.scale`, `Behaviour` lifecycle, `GameObject.id`, `World.isDisposed`. |

So the frame side of the seam held up completely: no core change was needed
to make sources follow objects, stop with their objects, or pause with the
game's clock.

## 2. Gaps

Ordered by how much they hurt a real audio package.

### GAP 1: a package can't read the world's `AssetSource`
`World({source})` stores it in the private `_source`. Audio clips come
from the same `assets/` folder as models, but the package can't get at the
bytes, so the sketch asks the game for it again: `world.audio.source =
files`. With Flutter, the package could reach for `rootBundle` itself, but
then a headless test (or a game reading files or the network, which
`AssetSource` exists for) loads from the wrong place.
- **Physics:** never loads files (meshes come from the engine's own
  `Mesh.positions/indices`), so it never met this.
- **Smallest change:** a getter in extend.dart: `AssetSource? get source`
  on `WorldSystems` (one line). Folded into GAP 2's change below.

### GAP 2: a package can't add an asset kind
`Assets` knows two kinds, in a private `enum _Kind { model, texture }`;
the loader `_Asset.start` switches on it, `_Kind.of` maps extensions, and
`preload` throws `ArgumentError` for anything else (`.wav`: "is neither a
model (.glb) nor a texture"). So the core's good parts (load once, two
askers in one frame share one future, a failed load is retried, the
disposed-world race) have to be copied into the package (the sketch's
`_clips` map copies the first two, badly).
- **Physics:** no assets.
- **Smallest change:** open the kind. In extend.dart:
  ```dart
  abstract class AssetKind<T extends Object> {
    const AssetKind(this.extensions);          // ['.wav', '.ogg', '.mp3']
    final List<String> extensions;
    Future<T> load(World world, String name, Uint8List bytes);
    void free(World world, T value);           // when the last holder goes
  }
  extension AssetsExtend on Assets {
    Future<T> load<T extends Object>(String name, AssetKind<T> kind);
  }
  void registerAssetKind(AssetKind<Object> kind); // so preload knows .wav
  ```
  `_Asset` already holds `Object value`; `_Kind` becomes the two built-in
  `AssetKind`s; `_free` calls `kind.free` for a package's. Roughly 40 lines
  in assets.dart, no behaviour change for models and textures. Then the
  package's API is `await assets.load('engine.wav', AudioClip.kind)`, or an
  extension `assets.audioClip('engine.wav')`, held by the object that asked,
  like `assets.texture`.

### GAP 3: a package can't tie a resource to an object's lifetime
The core frees an asset when the last object that asked for it is
destroyed (`GameObject._uses`, `World._release`). Outside the core there is
no "this object was destroyed" signal for a system, and `_uses` is private.
The sketch therefore keeps every clip until the world is disposed: fine for
a level, a leak for a long session that streams music tracks.
- **Physics:** its native things belong to behaviours (`Collider`,
  `Rigidbody`), and behaviours get `onDestroy`; mesh colliders' meshes are
  ref-counted by the colliders themselves (`_HeldMesh.users`). Audio could do
  the same for clips held by `AudioSource`s, but not for a clip a
  `GameObject` loaded and keeps in a field (`final jump = await
  audio.clip(...)`), which is the common Unity-style use.
- **Smallest change:** GAP 2's `AssetKind` closes it, since `Assets.load`
  records the use on the asking object exactly as `texture` does. (Without
  GAP 2, the alternative is a `WorldSystem.objectDestroyed(GameObject)` hook,
  which is bigger and less useful.)

### GAP 4: pausing is polled, and `world.paused` doesn't exist yet
There's no signal when `time.scale` changes; the sketch reads it once a
frame in `afterFrame` (cheap: one double). It works, with up to one frame
of latency when a Flutter button sets `time.scale = 0` between frames.
Unity keeps audio and the time scale apart (`AudioListener.pause`); the
sketch offers both, `audio.paused` and `pauseWithTime` (default on).
- **Physics:** uses `time.scale` implicitly (0 runs no fixed steps), no
  signal needed.
- **Smallest change:** none now. For stage 7's `world.paused`: make it a
  public, readable flag, and **keep running the systems' `beforeFrame` and
  `afterFrame` while paused** (or add `WorldSystem.pausedChanged(bool)`); if
  a paused world stops calling `update` altogether, the audio never learns it
  should pause. That is a constraint on the stage 7 design, worth writing
  into it.

### GAP 5: the core never hears that the app went to the background
`FynkelView`'s `Ticker` just stops when the app is backgrounded; nothing
calls the world, so no system, object or behaviour can react. Physics
doesn't care (no frames, no steps). Audio does: its mixer runs on its own
thread and keeps playing the last sounds, at the last positions, from the
background. The sketch exposes `audio.applicationPaused` and the test sets
it by hand.
- **Workaround available today:** make fynkel_audio a Flutter package that
  installs its own `AppLifecycleListener`. Works, but each package that
  cares repeats it, and game code (save on background, Unity's
  `OnApplicationPause`) has nothing.
- **Smallest change:** in the core, `World.applicationPaused` (a bool
  fynkel_flutter sets) that calls a new `WorldSystem.applicationPaused(bool)`
  hook, plus `GameObject/Behaviour.onApplicationPause(bool)` as Unity has.
  In fynkel_flutter, `FynkelViewState` adds an `AppLifecycleListener` and
  sets it (paused/hidden → true, resumed → false). About 30 lines in each.
  This is stage 7's "the app going to the background" item; audio is the
  first package that needs it.

### GAP 6: the shared cargo build only builds crates of this repository
`buildRustCrate` takes the workspace as `input.packageRoot.resolve('../../')`
and requires `crates/<crate>/Cargo.toml` there. A package that isn't in
`packages/` of this checkout can't use it, and every package's hook
declares every `.rs` under `crates/` as a dependency, so a change in
`fynkel_render` reruns the physics (and an audio) hook (cargo then no-ops;
the cost is the cargo invocation, about a second, and the hooks serialize on
cargo's lock of the shared `target/`).
- **Physics:** lives in this repository, so it fit.
- **Smallest change:** a `workspace:` (or `manifest:`) parameter to
  `buildRustCrate`, defaulting to today's `../../`, and declare as
  dependencies only the crate's own directory plus its path-dependencies
  (or just `crates/<crate>` and the workspace's `Cargo.lock`). Fine to leave
  until a package outside the repository exists.

### GAP 7: writing many transforms in one call (not needed by audio)
docs/stage-5.md §3.1 promised "transforms written, for many objects in one
call", but extend.dart only reads. Physics writes each awake body with
`object.transform = ...`, which makes a `Transform` per body (the
allocation its contacts alloc test has to work around) and converts world
to local itself. Audio only reads, so it doesn't care; **animation and
particles would** (a skeleton is dozens of objects a frame).
- **Smallest change:** `world.writeTransforms(ids, count, floats, {space:
  local|world})`, the mirror of `readWorldTransforms`, bumping each object's
  stamp and `transformVersion` and going into the same `_unsent` send.

### GAP 8: which objects moved (not needed by audio)
extend.dart gives a global `transformVersion` and a per-object
`transformStamp`, not the moved objects. In a real game something moves
every frame, so audio's "skip if nothing moved" rarely skips; it then
re-reads all its 3D voices, which is fine for tens of sources (one call).
Physics walks every collider's ancestor chain when anything moved (~42 ns
per collider, measured in stage 5 step 3), because a parent's move moves
the children.
- **Smallest change** (for physics and future heavy users, not audio):
  expose the list the core already keeps for its own send (`_unsent`, in
  order) as a per-frame "set this frame" view, or keep a per-object
  `worldStamp` (the max of its ancestors' stamps, updated when a parent is
  set) so a follower checks one int instead of walking the chain.

### Smaller points (no change needed, or not the core's)
- **componentRemoved hook:** not needed: `AudioSource` and `AudioListener`
  are behaviours, and removal arrives as `onDisable`. A system tracking the
  engine's components (e.g. a future animation system following
  `MeshRenderer`s) would need `WorldSystem.componentRemoved`, the mirror of
  `componentAdded`; physics today has none and rechecks `isRemoved` when it
  next re-specs a collider. Cheap to add with GAP 5's change.
- **`Component` is sealed:** packages can only make `Behaviour`s. Fine for
  audio (and Unity's `AudioSource` is a Behaviour too). A behaviour costs a
  `onUpdate` dispatch per frame that does nothing; negligible.
- **World dispose:** `World.dispose` clears objects without
  `onDisable`/`onDestroy`, so the system's `dispose` must free everything
  itself (the sketch and physics both do). Documented on
  `WorldSystem.dispose`, but worth one more sentence there: "behaviours get
  no calls".
- **A world dropped without `dispose`:** the core's `NativeFinalizer` frees
  the engine world, but `WorldSystem.dispose` never runs. Physics' `RawPhysics`
  attaches its own `NativeFinalizer`; audio must too, and its finalizer must
  stop the voices, or a garbage-collected world keeps making sound.
- **Hot reload:** systems survive (Dart objects persist), and
  `restartOnReload` / F5 dispose the world, which stops the sounds: fine.
  **Hot restart (R)** is the audible risk: the Dart isolate is replaced but
  the native library and its audio thread stay loaded. Whether
  `NativeFinalizer`s run then is worth a check on device; either way the
  native side should reset itself on its first `init` from a new isolate
  (an "instance generation" in the library). Physics has the same latent
  issue (a stepping thread, leaked worlds) but nobody hears it.
- **Threading:** none of the core's business. The backend owns a real-time
  audio thread; Dart sends commands without locks and never blocks; clip
  decoding runs off the UI thread (native async, or `Isolate.run`).
- **`readWorldTransforms` takes `Pointer`s:** right for physics; it makes a
  pure-Dart backend (flutter_soloud takes doubles) use `package:ffi` and
  `malloc`. Acceptable; a `Float32List` overload would be friendlier.
- **Testability:** the system's constructor is private and made lazily, so
  the package needs its own seam to swap in a fake (`Audio.makeBackend`
  here). Physics tests use real Rapier, so it never needed one. A pattern
  worth writing down for packages, not a core change.

## 3. The native side

### A third Rust crate, like physics
`crates/fynkel_audio` (mixing, spatialization, decoding) +
`crates/fynkel_audio_ffi` (the C interface, `fa_*`, its own `FA_ABI_VERSION`,
cbindgen header, ffigen bindings), built by `packages/fynkel_audio/hook/build.dart`:

```dart
void main(List<String> args) async {
  await build(args, (input, output) =>
      buildRustCrate(input, output, crate: 'fynkel_audio_ffi', asset: 'fynkel_audio'));
}
```

Candidate crates: `cpal` for output (CoreAudio on macOS/iOS, AAudio on
Android), `symphonia` (wav/ogg/mp3/flac decode), and either `kira` (mixer,
tweens, spatial tracks, clocks) or a small own mixer (`oddio`-style) for 3D:
distance attenuation (Unity's logarithmic rolloff between min and max
distance), panning from the listener's rotation, doppler from position
deltas. The C interface follows physics': one call each way per frame
(`fa_voices_place(ids, transforms, n)`, the ten-float layout
`readWorldTransforms` writes, so no copy), commands into a lock-free queue
the audio thread drains.

What's awkward:
- **Platform audio sessions are not Rust's.** iOS needs an `AVAudioSession`
  category (ambient: mixes with other apps and obeys the silent switch;
  playback: doesn't), interruption handling (a phone call), and route
  changes (headphones out); Android needs audio focus. These are Swift/Kotlin
  APIs. From Rust it's `objc2` + `jni` glue, which is a lot of platform code
  in a crate that otherwise needs none, and which Flutter plugins already
  do well (`audio_session`).
- **Two, then three, native libraries.** Each cdylib carries its own Rust
  standard library and panic machinery (a few hundred KB each), its own ABI
  number, and its own load. They don't share memory with the engine (nor
  does physics), so that's tolerable, but it's a cost per package.
- **Build time and toolchains.** Another crate tree to compile for every
  target Flutter builds (macOS, iOS device + simulator, Android
  arm64 + x64); a cold `cpal + symphonia + kira` build is minutes per triple.
  If `cpal`'s Oboe backend were used on Android it would bring a C++ build
  (oboe-sys) and the NDK's libc++ into the hook; AAudio through `ndk` avoids
  that.
- **GAP 6** applies: only possible inside this repository.

### A Dart package over a Flutter audio plugin instead
`flutter_soloud` (SoLoud, C++, over `dart:ffi`) already has what Unity's
`AudioSource` needs: wav/ogg/mp3/flac, voices with volume, pitch (relative
play speed), looping, pause, 3D sources with min/max distance and an
attenuation model, a listener position and orientation, buses for a mixer,
filters, and support on iOS, Android, macOS, Windows, Linux and web.
fynkel_audio would then be **Dart only**: the `AudioBackend` here,
implemented over it, plus an `audio_session`-style plugin (or soloud's own
handling) for categories and interruptions.

- For: no Rust, no hook, no ABI to keep; platform sessions handled by
  people who do that; weeks less work; the backend interface keeps the
  option open.
- Against: fynkel_audio depends on Flutter (the core is pure Dart; the
  fake backend keeps headless tests working); 3D updates are one FFI call
  per voice, not one batch (fine at tens of voices, the usual count); SoLoud
  is one global engine per process, so two worlds share it (the system maps
  its voices; master volume and "pause all" need care); its hot-restart
  behaviour must be checked.

**Recommendation:** ship fynkel_audio as Dart over `flutter_soloud`, behind
`AudioBackend`. Write a Rust crate only if a measurement says so (hundreds of
3D voices, or audio that must be sample-locked to the simulation). This also
means the "every package is a Rust crate" pattern isn't forced on future
packages; extend.dart is what they share, not the cargo build.

## 4. Verdict

**extend.dart is a sound seam for the frame.** Audio needed nothing from the
core it didn't already give for its main job: one system per world made on
first use, hooks at the right points (including `afterFrame`, after the
cameras), behaviours whose lifecycle handles enable/disable/remove/destroy,
a batched world-transform read, and a version to skip work. The sketch's 3D
following, pausing with the game, stopping with its object and freeing with
the world all run on public + extend.dart APIs, in a headless test. Physics
was not a one-off.

**It is not yet a seam for everything around the frame.** Ranked, the
changes that would make it one:

1. **Open asset kinds** (`AssetKind<T>`, `Assets.load`, `registerAssetKind`,
   `WorldSystems.source`): closes GAPs 1–3, and every content package needs
   it (audio clips, animation clips, particle presets, fonts). ~40 lines in
   assets.dart.
2. **The app lifecycle in the core** (`World.applicationPaused`,
   `WorldSystem.applicationPaused`, `onApplicationPause` on objects and
   behaviours; fynkel_flutter feeds it): GAP 5. Needed by audio, saving, and
   networking (disconnect/reconnect). Fits stage 7.
3. **Systems keep their hooks while the world is paused** (or a
   `pausedChanged` hook), decided when `world.paused` is designed: GAP 4.
4. **`world.writeTransforms`**, the mirror of `readWorldTransforms`: GAP 7.
   Animation can't be fast without it; physics would lose its
   `Transform`-per-body allocation.
5. **`componentRemoved`** (and, for networking's object replication,
   `objectAdded` / `objectDestroyed` system hooks), cheap symmetric
   additions.
6. **Moved-objects for followers** (`worldStamp` or a per-frame moved
   list): GAP 8; a performance change for physics and animation, not a
   correctness one.
7. **`buildRustCrate(workspace:)`** and narrower dependencies: GAP 6, when a
   package outside the repository appears.

For the four packages named in stage 6: **audio** fits now with GAPs 1–5
worked around; **animation** needs 1, 4, 6; **particles** need 1 and
probably 4 (or draw through the engine, a different seam: extend.dart has
nothing for adding render work, and a particle package that renders would
need an engine-side extension point, not a Dart one); **networking** needs
2, 5, and object add/destroy hooks, plus a hook before input
(`beforeFrame` already is).
