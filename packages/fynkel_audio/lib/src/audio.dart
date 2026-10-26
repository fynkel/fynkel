// A world's sound (docs/stage-11.md): the system that owns the native
// mixer, follows the game's pause and the app's, opens the output when the
// world is shown, and tells each AudioSource when its sound has ended.

import 'dart:ffi';
import 'dart:isolate';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';
import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';

import 'bindings.g.dart';
import 'owned_memory.dart';

part 'clip.dart';
part 'listener.dart';
part 'mix.dart';
part 'source.dart';

/// How the game's sound sits with the phone's, on iOS and Android
/// (elsewhere it changes nothing): [AudioSession.ambient] unless set.
/// Either way the game's sound waits for a phone call.
enum AudioSession {
  /// The silent switch mutes the game (iOS), and the player's own music
  /// keeps playing under it: what most games do.
  ambient,

  /// The game plays with the silent switch on (iOS), and the player's
  /// music stops: on Android, the game takes the audio focus (Unity's
  /// *Mute Other Audio Sources*). For a game whose sound is the point.
  playback,

  /// The game talks and listens at once, like a call: the mic works
  /// alongside the game's sound (iOS's `playAndRecord`), which plays from
  /// the loudspeaker (not the earpiece) and ignores the silent switch. On
  /// Android, as [playback]. Set it before the first sound, and let the
  /// mic's plugin leave the session alone, so the two don't fight.
  voiceChat,
}

/// The sound itself failed: a panic, or sound a panic left in doubt. A
/// game's own mistakes throw Dart's errors instead: an [ArgumentError]
/// for a bad value, a [StateError] for a call at the wrong time, a
/// [FormatException] for a file that isn't sound.
final class AudioException implements Exception {
  /// The sound failed, saying [message].
  AudioException(this.message);

  /// What the sound says happened.
  final String message;

  @override
  String toString() => 'AudioException: $message';
}

/// Checks once, before the first sound, that the library is the one these
/// bindings were generated for.
final bool _library = () {
  final version = fa_abi_version();
  if (version != FA_ABI_VERSION) {
    throw StateError(
      'package:fynkel_audio was built for sound ABI $FA_ABI_VERSION, but the library is ABI $version. '
      'Rebuild: the build hook compiles crates/fynkel_audio_ffi from the same checkout.',
    );
  }
  return true;
}();

String _lastError() => fa_last_error().cast<Utf8>().toDartString();

void _check(int status) {
  if (status == FaStatus.FA_STATUS_OK) return;
  final message = _lastError();
  throw switch (status) {
    FaStatus.FA_STATUS_INVALID_VALUE => ArgumentError(message),
    FaStatus.FA_STATUS_CANT_DECODE => FormatException(message),
    _ => AudioException(message),
  };
}

/// The sound at a glance: whether an output plays it, at what rate, the
/// voices the mixer has now (paused ones too) and counts since the start.
final class AudioStats {
  AudioStats._(FaStats s, this.frameMilliseconds)
    : device = s.device == 1,
      rate = s.rate,
      channels = s.channels,
      voices = s.voices,
      started = s.started,
      stolen = s.stolen,
      refused = s.refused,
      callbacks = s.callbacks,
      underruns = s.underruns,
      reopened = s.reopened,
      starved = s.starved;

  /// Whether a sound output plays it. Not in tests, nor before the world
  /// is shown on screen.
  final bool device;

  /// Frames a second, and channels, of the output (48,000 in stereo
  /// without one).
  final int rate, channels;

  /// The voices playing now, of [Audio.maxVoices].
  final int voices;

  /// Sounds started; voices that gave way to more important sounds;
  /// sounds not played because every voice was more important.
  final int started, stolen, refused;

  /// Times the output asked for sound, and times it wasn't given it in
  /// time (a crackle).
  final int callbacks, underruns;

  /// Times the output was lost (unplugged, or taken by the system) and
  /// opened again: a few over a game, not one a second.
  final int reopened;

  /// Times the output asked for sound and a [AudioClip.streamed] clip
  /// hadn't been decoded far enough (a gap): never, while its decoding
  /// keeps up.
  final int starved;

  /// What sound costs the game's frame, in milliseconds, averaged over
  /// about the last second: reading where the 3D sources and the listener
  /// are, and telling the mixer. Reading them first sends the frame's
  /// moves to the engine, which drawing would do anyway; that's counted
  /// here too. The mixing itself runs on the output's
  /// own thread (or, without an output, in this frame too).
  final double frameMilliseconds;

  @override
  String toString() =>
      'AudioStats(${device ? 'device' : 'no device'} $rate Hz × $channels, $voices voices, started $started, '
      'stolen $stolen, refused $refused, callbacks $callbacks, underruns $underruns, starved $starved, '
      '${frameMilliseconds.toStringAsFixed(3)} ms a frame)';
}

/// A world's sound, as Unity's `AudioListener` and `AudioSettings` are:
/// [volume] for everything, [group]s under it (music, effects), [music],
/// [playOneShot] for a sound that belongs to no object (a click), [playAt]
/// for one at a place in the world (an explosion), and [session]. Every
/// object and behaviour has it as `audio`; it's made the first time it's
/// asked for, or by the first [AudioSource].
///
/// Sounds play through the device once the world is shown on screen, and
/// stop while the app is away. While the game is [World.paused], its
/// sounds hold where they are, but for those that ignore the pause
/// ([AudioSource.ignorePause]). In a test, where no world is shown, sound
/// is made and timed the same way, without a device: sounds still end
/// when they would.
///
/// Once a frame, after the cameras move, it reads where every 3D
/// [AudioSource] that's playing is, and where the [AudioListener] (or the
/// camera) is and faces, in one call, and the mixer moves each sound
/// there over its next few milliseconds, so one that flies past doesn't
/// crackle.
final class Audio extends WorldSystem implements Finalizable {
  Audio._(super.world) {
    _library;
    _audio = fa_audio_new();
    _finalizer.attach(this, _audio.cast(), detach: this);
    world.addAssetKind(AudioClip.kind);
    _checkLayout();
    // Whoever hears stands at the origin, looking along -Z, until there's
    // a listener or a camera.
    for (var i = 0; i < 7; i++) {
      _listenerFloats[i] = i == 6 ? 1 : 0;
    }
    if (world.isOnScreen) viewStarted();
  }

  static final _finalizer = NativeFinalizer(
    Native.addressOf<NativeFunction<Void Function(Pointer<FaAudio>)>>(fa_audio_free).cast(),
  );

  /// How many sounds play at once, as Unity's: past it, the least
  /// important gives way ([AudioSource.priority]).
  static const maxVoices = 32;

  late final Pointer<FaAudio> _audio;
  var _disposed = false;

  // Scratch for calls, kept: one each, allocated once, and freed when this
  // is collected, not by [dispose] (see [WorldSystem.dispose]).
  late final _memory = OwnedMemory(this);
  late final _play = _memory<FaPlay>();
  late final _settings = _memory<FaSettings>();
  late final _voice = _memory<FaVoice>();
  late final _count = _memory<Size>();
  late final _seconds = _memory<Double>();
  late final _handle = _memory<Uint64>();
  late var _stopped = _memory<FaStopped>(8);
  var _stoppedRoom = 8;
  late final _spatial = _memory<FaSpatial>();
  late final _listener = _memory<FaListener>();
  late final _fade = _memory<FaFade>();
  // Room for every voice's place each frame, and the ids and transforms
  // read for them: the listener's object, then the 3D sources'.
  static const _placeRoom = 2 * maxVoices;
  late final _places = _memory<FaPlace>(_placeRoom);
  late var _ids = _memory<Uint64>(8);
  late var _transforms = _memory<Float>(8 * 10);
  var _idRoom = 8;

  // The scratch structs as memory, written and read on each frame without
  // `.ref`, which makes an object each time, or the generated accessors,
  // which box a double in the JIT; `Pointer` reads and writes do neither.
  // FaPlay and FaSettings: volume and pitch, floats 0 and 1; looping,
  // word 2; group, word 3; then FaPlay's priority and pausable, words 4
  // and 5, owner at byte 24 and start at byte 32. FaVoice: playing, word
  // 0; time at byte 8.
  // Checked against the accessors when made. The volume and pitch to play
  // or set, and the start, are written by the caller of [_start] and
  // [_set], as a double passed to a call the JIT doesn't inline is boxed.
  late final _playFloats = _play.cast<Float>();
  late final _playWords = _play.cast<Uint32>();
  late final _playOwner = (_play.cast<Uint8>() + 24).cast<Uint64>();
  late final _playFrom = (_play.cast<Uint8>() + 32).cast<Double>();
  late final _settingsFloats = _settings.cast<Float>();
  late final _settingsWords = _settings.cast<Uint32>();
  late final _voicePlaying = _voice.cast<Uint32>();
  late final _voiceTime = (_voice.cast<Uint8>() + 8).cast<Double>();
  // FaPlay's spatial: blend, pan, min and max distance, floats 10 to 13;
  // rolloff, word 14; position, floats 15 to 17; fade in, float 18.
  // FaSpatial the same from
  // 0. FaListener: position, floats 0 to 2; rotation, 3 to 6. FaPlace, 24
  // bytes each: its voice, word (64-bit) 3 × i; its position, floats
  // 6 × i + 2 to 4.
  // FaFade: to and seconds, floats 0 and 1.
  late final _fadeFloats = _fade.cast<Float>();
  late final _spatialFloats = _spatial.cast<Float>();
  late final _spatialWords = _spatial.cast<Uint32>();
  late final _listenerFloats = _listener.cast<Float>();
  late final _placeVoices = _places.cast<Uint64>();
  late final _placeFloats = _places.cast<Float>();

  // The sources whose voices are playing, by their owner id; the enabled
  // ones, in order; the enabled listeners, the last of them hearing.
  final _sources = <int, AudioSource>{};
  final _enabled = <AudioSource>[];
  final _listeners = <AudioListener>[];
  var _nextOwner = 1;
  final _groups = <String, AudioGroup>{};

  var _gamePaused = false;
  var _timeScale = 1.0;
  String? _outputError;

  Pointer<FaAudio> get _native {
    if (_disposed) throw StateError('this Audio belongs to a disposed World');
    return _audio;
  }

  /// The gain over every sound, 0 to 1 (Unity's `AudioListener.volume`):
  /// a settings screen's master volume. 1 unless set. Throws an
  /// [ArgumentError] outside 0 to 1.
  double get volume => _volume;
  set volume(double value) {
    _checkUnit(value, 'volume');
    _check(fa_audio_volume_set(_native, value));
    _volume = value;
  }

  var _volume = 1.0;

  /// The group called [name], made the first time it's asked for: its
  /// sounds' volume set together ([AudioGroup]). There's room for 15.
  AudioGroup group(String name) {
    final known = _groups[name];
    if (known != null) return known;
    if (_groups.length == maxGroups) {
      throw StateError('there are $maxGroups sound groups already (${_groups.keys.join(', ')}): no room for $name');
    }
    return _groups[name] = AudioGroup._(this, name, _groups.length + 1);
  }

  /// How many [group]s there may be.
  static const maxGroups = 15;

  /// The world's music: one track at a time, crossfaded ([AudioMusic]).
  AudioMusic get music => _music ??= AudioMusic._(this);
  AudioMusic? _music;

  /// How the game's sound sits with the phone's, on iOS and Android.
  AudioSession get session => _session;
  set session(AudioSession value) {
    _check(fa_audio_session_set(_native, value.index));
    _session = value;
  }

  var _session = AudioSession.ambient;

  /// Why the sound output couldn't be opened, or null: there's none (a
  /// Mac without speakers), or the system wouldn't give it. Sound goes on
  /// without it, and it's tried again each second.
  String? get outputError => _outputError;

  /// Draws each 3D [AudioSource]'s [AudioSource.minDistance] (the inner
  /// sphere) and [AudioSource.maxDistance] (the outer) with the world's
  /// debug lines, every frame while it's on: to see where sounds can be
  /// heard. Off unless set.
  bool drawSources = false;

  /// The sound at a glance.
  AudioStats get stats {
    final out = calloc<FaStats>();
    try {
      _check(fa_audio_stats(_native, out));
      return AudioStats._(out.ref, _frameMicros / 1000);
    } finally {
      calloc.free(out);
    }
  }

  /// Plays [clip] once, belonging to no object: a button's click, say.
  /// [volume] (0 to 1, 1 unless given) and [pitch] (0.05 to 8, 1 unless
  /// given) are its own; it holds while the game is paused unless
  /// [ignorePause], and is under [group]'s volume, if given. Nothing plays
  /// it if every voice is busy with something more important than
  /// [priority], or of priority 0, which never gives way.
  void playOneShot(
    AudioClip clip, {
    double? volume,
    double? pitch,
    bool ignorePause = false,
    int priority = 128,
    AudioGroup? group,
  }) {
    // Nullable, not defaulted, and checked here, the error given the
    // value passed: the JIT boxes a double with a default, one passed to a
    // call it doesn't inline, or one an error is made with, at every call.
    final gain = volume ?? 1.0, speed = pitch ?? 1.0;
    if (!(gain >= 0 && gain <= 1)) throw ArgumentError.value(volume, 'volume', 'is not between 0 and 1');
    if (!(speed >= 0.05 && speed <= 8)) throw ArgumentError.value(pitch, 'pitch', 'is not between 0.05 and 8');
    _checkPriority(priority);
    _playFloats
      ..[0] = gain
      ..[1] = speed
      ..[10] = 0
      ..[11] = 0
      ..[12] = 1
      ..[13] = 500;
    _playWords[14] = 0;
    _playFrom.value = 0;
    _start(clip, looping: false, priority: priority, pausable: !ignorePause, owner: 0, group: _indexOf(group));
  }

  /// Plays [clip] once at [position] in the world, a 3D sound that belongs
  /// to no object, as Unity's `AudioSource.PlayClipAtPoint`: an explosion,
  /// a coin picked up. It stays where it was played, heard from there as
  /// the listener moves, fading between [minDistance] and [maxDistance]
  /// (1 and 500 unless given) as [rolloffMode] says. [volume], [pitch],
  /// [ignorePause], [priority] and [group] are as [playOneShot]'s.
  void playAt(
    AudioClip clip,
    Vec3 position, {
    double? volume,
    double? pitch,
    double? minDistance,
    double? maxDistance,
    AudioRolloffMode rolloffMode = AudioRolloffMode.logarithmic,
    bool ignorePause = false,
    int priority = 128,
    AudioGroup? group,
  }) {
    // Nullable and checked here, for the reason playOneShot's are.
    final gain = volume ?? 1.0, speed = pitch ?? 1.0, near = minDistance ?? 1.0, far = maxDistance ?? 500.0;
    if (!(gain >= 0 && gain <= 1)) throw ArgumentError.value(volume, 'volume', 'is not between 0 and 1');
    if (!(speed >= 0.05 && speed <= 8)) throw ArgumentError.value(pitch, 'pitch', 'is not between 0.05 and 8');
    if (!(near > 0 && near.isFinite)) throw ArgumentError.value(minDistance, 'minDistance', 'is not above 0');
    if (!(far > near && far.isFinite)) {
      throw ArgumentError.value(maxDistance, 'maxDistance', 'is not above minDistance ($near)');
    }
    _checkPriority(priority);
    _playFloats
      ..[0] = gain
      ..[1] = speed
      ..[10] = 1
      ..[11] = 0
      ..[12] = near
      ..[13] = far
      ..[15] = position.x
      ..[16] = position.y
      ..[17] = position.z;
    _playWords[14] = rolloffMode.index;
    _playFrom.value = 0;
    _hear();
    _start(clip, looping: false, priority: priority, pausable: !ignorePause, owner: 0, group: _indexOf(group));
  }

  /// Makes [frames] frames of the sound (48,000 a second, interleaved
  /// left and right) as a speaker would have played them: for a test to
  /// listen to what plays, and compare it with what it should be. The
  /// sound moves on by that much. Throws a [StateError] while a device
  /// plays, as on screen.
  Float32List render(int frames) {
    RangeError.checkNotNegative(frames, 'frames');
    final out = calloc<Float>(frames * 2);
    try {
      final status = fa_audio_render(_native, out, frames);
      if (status == FaStatus.FA_STATUS_NO_DEVICE) throw StateError(_lastError());
      _check(status);
      return Float32List.fromList(out.asTypedList(frames * 2));
    } finally {
      calloc.free(out);
    }
  }

  /// The object that hears: the last [AudioListener] enabled, or the
  /// camera, or none.
  GameObject? get _hearer {
    final listeners = _listeners;
    if (listeners.isNotEmpty) return listeners[listeners.length - 1].gameObject;
    return world.camera?.gameObject;
  }

  /// Room for [count] ids and their transforms, keeping the ids written.
  void _room(int count) {
    if (count <= _idRoom) return;
    final ids = _memory<Uint64>(count * 2);
    for (var i = 0; i < _idRoom; i++) {
      ids[i] = _ids[i];
    }
    _memory
      ..free(_ids)
      ..free(_transforms);
    _idRoom = count * 2;
    _ids = ids;
    _transforms = _memory<Float>(_idRoom * 10);
  }

  /// Writes [source]'s 2D and 3D settings to the next play, and, for a 3D
  /// one, where its object is, telling the mixer where the listener is
  /// first, so it starts where it should be heard.
  void _writeSpatial(AudioSource source) {
    _playFloats
      ..[10] = source._spatialBlend
      ..[11] = source._panStereo
      ..[12] = source._minDistance
      ..[13] = source._maxDistance;
    _playWords[14] = source._rolloffMode.index;
    if (source._spatialBlend > 0) {
      _ids[0] = source.gameObject.id;
      world.readWorldTransforms(_ids, 1, _transforms);
      _playFloats
        ..[15] = _transforms[0]
        ..[16] = _transforms[1]
        ..[17] = _transforms[2];
      _hear();
    } else {
      _playFloats
        ..[15] = 0
        ..[16] = 0
        ..[17] = 0;
    }
  }

  /// Writes [source]'s 2D and 3D settings to [_spatial].
  void _fillSpatial(AudioSource source) {
    _spatialFloats
      ..[0] = source._spatialBlend
      ..[1] = source._panStereo
      ..[2] = source._minDistance
      ..[3] = source._maxDistance;
    _spatialWords[4] = source._rolloffMode.index;
  }

  /// Tells the mixer where the hearer is now.
  void _hear() {
    final hearer = _hearer;
    if (hearer == null) return;
    _ids[0] = hearer.id;
    world.readWorldTransforms(_ids, 1, _transforms);
    _readListener(0);
    _check(fa_audio_place(_native, _listener, _places, 0));
  }

  /// [_listener] from the transform read at [index].
  void _readListener(int index) {
    final t = index * 10;
    for (var i = 0; i < 7; i++) {
      _listenerFloats[i] = _transforms[t + i];
    }
  }

  /// The frame's moves, in one call: the hearer, and every 3D source that
  /// plays (or, with [drawSources], is enabled), each of its voices.
  void _place() {
    // Asked once a frame: finding the camera is a call into the engine.
    final hearer = _hearer;
    final enabled = _enabled;
    final draw = drawSources;
    var count = 0;
    if (hearer != null) {
      _room(1);
      _ids[count++] = hearer.id;
    }
    final first = count;
    for (var i = 0; i < enabled.length; i++) {
      final source = enabled[i];
      if (source._spatialBlend > 0 && (draw || source._voice != 0 || source._shotCount > 0)) {
        _room(count + 1);
        _ids[count++] = source.gameObject.id;
      }
    }
    if (count == 0) return;
    world.readWorldTransforms(_ids, count, _transforms);
    if (hearer != null) _readListener(0);
    var places = 0;
    var at = first;
    for (var i = 0; i < enabled.length; i++) {
      final source = enabled[i];
      if (!(source._spatialBlend > 0 && (draw || source._voice != 0 || source._shotCount > 0))) continue;
      final t = at++ * 10;
      if (draw) {
        final centre = Vec3(_transforms[t], _transforms[t + 1], _transforms[t + 2]);
        world.debug
          ..sphere(centre, source._minDistance, color: const Color(0xFF66CCFF))
          ..sphere(centre, source._maxDistance, color: const Color(0xFF3366AA));
      }
      for (var v = -1; v < source._shotCount; v++) {
        final voice = v < 0 ? source._voice : source._shots[v];
        if (voice == 0 || places == _placeRoom) continue;
        _placeVoices[places * 3] = voice;
        final p = places * 6 + 2;
        _placeFloats
          ..[p] = _transforms[t]
          ..[p + 1] = _transforms[t + 1]
          ..[p + 2] = _transforms[t + 2];
        places++;
      }
    }
    _check(fa_audio_place(_native, _listener, _places, places));
  }

  /// Fails if the memory views above don't fall on the fields they're for.
  void _checkLayout() {
    _play.ref
      ..settings.volume = 0.25
      ..settings.pitch = 0.5
      ..settings.looping = 3
      ..settings.group = 13
      ..fade_in = 1.5
      ..priority = 4
      ..pausable = 5
      ..owner = 6
      ..from = 0.75
      ..spatial.blend = 0.5
      ..spatial.pan = -0.5
      ..spatial.min_distance = 2
      ..spatial.max_distance = 3
      ..spatial.rolloff = 9
      ..position[0] = 10
      ..position[2] = 12;
    _spatial.ref
      ..blend = 1
      ..max_distance = 40
      ..rolloff = 7;
    _listener.ref
      ..position[1] = 21
      ..rotation[3] = 22;
    (_places + 1).ref
      ..voice = 31
      ..position[2] = 32;
    _settings.ref
      ..volume = 0.125
      ..pitch = 2
      ..looping = 8
      ..group = 14;
    _voice.ref
      ..playing = 7
      ..time = 1.25;
    _fade.ref
      ..to = 0.375
      ..seconds = 4;
    final seen = [
      _playFloats[0],
      _playFloats[1],
      _playWords[2],
      _playWords[4],
      _playWords[5],
      _playOwner.value,
      _playFrom.value,
      _settingsFloats[0],
      _settingsFloats[1],
      _settingsWords[2],
      _playWords[3],
      _playFloats[18],
      _settingsWords[3],
      _voicePlaying.value,
      _voiceTime.value,
      _playFloats[10],
      _playFloats[11],
      _playFloats[12],
      _playFloats[13],
      _playWords[14],
      _playFloats[15],
      _playFloats[17],
      _spatialFloats[0],
      _spatialFloats[3],
      _spatialWords[4],
      _listenerFloats[1],
      _listenerFloats[6],
      _placeVoices[3],
      _placeFloats[6 + 4],
      _fadeFloats[0],
      _fadeFloats[1],
    ];
    if (seen.join(' ') !=
        '0.25 0.5 3 4 5 6 0.75 0.125 2.0 8 13 1.5 14 7 1.25 0.5 -0.5 2.0 3.0 9 10.0 12.0 1.0 40.0 7 21.0 22.0 31 32.0 0.375 4.0') {
      throw StateError('package:fynkel_audio reads its structs at the wrong places: $seen');
    }
  }

  /// Plays [clip] at the volume, pitch and start written to [_playFloats]
  /// and [_playFrom], and the fade in written there too when [fading]; its
  /// voice's handle, or 0 if none was free.
  int _start(
    AudioClip clip, {
    required bool looping,
    required int priority,
    required bool pausable,
    required int owner,
    required int group,
    bool fading = false,
  }) {
    if (!fading) _playFloats[18] = 0;
    _playWords
      ..[2] = looping ? 1 : 0
      ..[3] = group
      ..[4] = priority
      ..[5] = pausable ? 1 : 0;
    _playOwner.value = owner;
    _check(fa_audio_play(_native, clip._native, _play, _handle));
    return _handle.value;
  }

  /// Fades [handle] to [to] over [seconds], stopping it there if [stop].
  void _fadeVoice(int handle, double to, double seconds, {required bool stop}) {
    _fadeFloats
      ..[0] = to
      ..[1] = seconds;
    _check(fa_voice_fade(_native, handle, _fade, stop ? 1 : 0));
  }

  /// Fades [group] to [to] over [seconds].
  void _fadeGroup(int group, double to, double seconds) {
    _fadeFloats
      ..[0] = to
      ..[1] = seconds;
    _check(fa_group_set(_native, group, _fade));
  }

  /// Sets [handle] to the volume and pitch written to [_settingsFloats].
  void _set(int handle, {required bool looping, required int group}) {
    _settingsWords
      ..[2] = looping ? 1 : 0
      ..[3] = group;
    _check(fa_voice_set(_native, handle, _settings));
  }

  /// Where [handle] is, in seconds; 0 once it doesn't play.
  double _timeOf(int handle) {
    _check(fa_voice_get(_native, handle, _voice));
    return _voicePlaying.value == 1 ? _voiceTime.value : 0.0;
  }

  /// Whether [handle] plays, and where: its time in [_voice].
  bool _read(int handle) {
    _check(fa_voice_get(_native, handle, _voice));
    return _voicePlaying.value == 1;
  }

  @override
  void beforeFrame() {
    super.beforeFrame();
    final paused = world.paused;
    if (paused != _gamePaused) {
      _check(fa_audio_game_paused_set(_native, paused ? 1 : 0));
      _gamePaused = paused;
    }
    final scale = world.time.scale;
    if (scale != _timeScale) {
      _timeScale = scale;
      for (final source in _sources.values) {
        if (source.followTimeScale) source._sendSettings();
      }
    }
  }

  // What the last frames' sound cost, in microseconds, smoothed.
  final _clock = Stopwatch()..start();
  var _frameMicros = 0.0;

  @override
  void afterFrame() {
    super.afterFrame();
    final start = _clock.elapsedMicroseconds;
    _frame();
    _frameMicros += ((_clock.elapsedMicroseconds - start) - _frameMicros) * 0.02;
  }

  void _frame() {
    _place();
    // Through memory: a double passed to a native call is boxed in the JIT.
    _seconds.value = world.time.unscaledDelta;
    _check(fa_audio_update(_native, _seconds, _count));
    final count = _count.value;
    if (count == 0) return;
    if (count > _stoppedRoom) {
      _memory.free(_stopped);
      _stoppedRoom = count;
      _stopped = _memory<FaStopped>(count);
    }
    _check(fa_stopped_read(_native, _stopped, count));
    final music = _music;
    for (var i = 0; i < count; i++) {
      final stopped = _stopped[i];
      final source = _sources[stopped.owner];
      if (source != null) source._ended(stopped.handle, stolen: stopped.stolen == 1);
      if (music != null && stopped.owner == music._owner) music._ended(stopped.handle);
    }
  }

  @override
  void viewStarted() {
    super.viewStarted();
    final status = fa_audio_open_device(_native);
    if (status == FaStatus.FA_STATUS_NO_DEVICE) {
      _outputError = _lastError();
    } else {
      _check(status);
      _outputError = null;
    }
  }

  @override
  void applicationPause(bool paused) {
    super.applicationPause(paused);
    final status = fa_audio_suspend(_native, paused ? 1 : 0);
    if (status != FaStatus.FA_STATUS_NO_DEVICE) _check(status);
  }

  @override
  void dispose() {
    super.dispose();
    if (_disposed) return;
    _disposed = true;
    _sources.clear();
    _finalizer.detach(this);
    fa_audio_free(_audio);
    // The scratch stays until this is collected: a call after this one
    // writes into it, then throws.
  }
}

void _checkUnit(double value, String name) {
  if (!(value >= 0 && value <= 1)) throw ArgumentError.value(value, name, 'is not between 0 and 1');
}

void _checkPitch(double value) {
  if (!(value >= 0.05 && value <= 8)) throw ArgumentError.value(value, 'pitch', 'is not between 0.05 and 8');
}

/// [group]'s number for the mixer: 0 for none.
int _indexOf(AudioGroup? group) => group == null ? 0 : group._index;

void _checkPriority(int value) {
  if (value < 0 || value > 256) throw ArgumentError.value(value, 'priority', 'is not between 0 and 256');
}

/// The world's sound, from any object.
extension GameObjectAudio on GameObject {
  /// The world's sound: made the first time it's asked for.
  Audio get audio => world.system(Audio._);
}

/// The world's sound, from any behaviour.
extension BehaviourAudio on Behaviour {
  /// The world's sound: made the first time it's asked for.
  Audio get audio => world.system(Audio._);
}

/// The world's sound, from the world.
extension WorldAudio on World {
  /// The world's sound: made the first time it's asked for.
  Audio get audio => system(Audio._);
}
