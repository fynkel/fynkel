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

part 'clip.dart';
part 'source.dart';

/// How the game's sound sits with the phone's, on iOS (elsewhere it
/// changes nothing): [AudioSession.ambient] unless set.
enum AudioSession {
  /// The silent switch mutes the game, and the player's own music keeps
  /// playing under it: what most games do.
  ambient,

  /// The game plays with the silent switch on, and the player's music
  /// stops (Unity's *Mute Other Audio Sources*): for a game whose sound is
  /// the point.
  playback,
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
  AudioStats._(FaStats s)
    : device = s.device == 1,
      rate = s.rate,
      channels = s.channels,
      voices = s.voices,
      started = s.started,
      stolen = s.stolen,
      refused = s.refused,
      callbacks = s.callbacks,
      underruns = s.underruns;

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

  @override
  String toString() =>
      'AudioStats(${device ? 'device' : 'no device'} $rate Hz × $channels, $voices voices, started $started, '
      'stolen $stolen, refused $refused, callbacks $callbacks, underruns $underruns)';
}

/// A world's sound, as Unity's `AudioListener` and `AudioSettings` are:
/// [volume] for everything, [playOneShot] for a sound that belongs to no
/// object (a click), and [session]. Every object and behaviour has it as
/// `audio`; it's made the first time it's asked for, or by the first
/// [AudioSource].
///
/// Sounds play through the device once the world is shown on screen, and
/// stop while the app is away. While the game is [World.paused], its
/// sounds hold where they are, but for those that ignore the pause
/// ([AudioSource.ignorePause]). In a test, where no world is shown, sound
/// is made and timed the same way, without a device: sounds still end
/// when they would.
final class Audio extends WorldSystem implements Finalizable {
  Audio._(super.world) {
    _library;
    _audio = fa_audio_new();
    _finalizer.attach(this, _audio.cast(), detach: this);
    world.addAssetKind(AudioClip.kind);
    _checkLayout();
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

  // Scratch for calls, kept: one each, allocated once.
  final _play = calloc<FaPlay>();
  final _settings = calloc<FaSettings>();
  final _voice = calloc<FaVoice>();
  final _count = calloc<Size>();
  final _seconds = calloc<Double>();
  final _handle = calloc<Uint64>();
  var _stopped = calloc<FaStopped>(8);
  var _stoppedRoom = 8;

  // The scratch structs as memory, written and read on each frame without
  // `.ref`, which makes an object each time, or the generated accessors,
  // which box a double in the JIT; `Pointer` reads and writes do neither.
  // FaPlay and FaSettings: volume and pitch, floats 0 and 1; looping,
  // word 2; then FaPlay's priority and pausable, words 3 and 4, owner at
  // byte 24 and start at byte 32. FaVoice: playing, word 0; time at byte 8.
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

  // The sources whose voices are playing, by their owner id.
  final _sources = <int, AudioSource>{};
  var _nextOwner = 1;

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

  /// How the game's sound sits with the phone's, on iOS.
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

  /// The sound at a glance.
  AudioStats get stats {
    final out = calloc<FaStats>();
    try {
      _check(fa_audio_stats(_native, out));
      return AudioStats._(out.ref);
    } finally {
      calloc.free(out);
    }
  }

  /// Plays [clip] once, belonging to no object: a button's click, say.
  /// [volume] (0 to 1, 1 unless given) and [pitch] (0.05 to 8, 1 unless
  /// given) are its own; it holds while the game is paused unless
  /// [ignorePause]. Nothing plays it if every voice is busy with something
  /// more important than [priority].
  void playOneShot(AudioClip clip, {double? volume, double? pitch, bool ignorePause = false, int priority = 128}) {
    // Nullable, not defaulted, and checked here, the error given the
    // value passed: the JIT boxes a double with a default, one passed to a
    // call it doesn't inline, or one an error is made with, at every call.
    final gain = volume ?? 1.0, speed = pitch ?? 1.0;
    if (!(gain >= 0 && gain <= 1)) throw ArgumentError.value(volume, 'volume', 'is not between 0 and 1');
    if (!(speed >= 0.05 && speed <= 8)) throw ArgumentError.value(pitch, 'pitch', 'is not between 0.05 and 8');
    _checkPriority(priority);
    _playFloats
      ..[0] = gain
      ..[1] = speed;
    _playFrom.value = 0;
    _start(clip, looping: false, priority: priority, pausable: !ignorePause, owner: 0);
  }

  /// Fails if the memory views above don't fall on the fields they're for.
  void _checkLayout() {
    _play.ref
      ..settings.volume = 0.25
      ..settings.pitch = 0.5
      ..settings.looping = 3
      ..priority = 4
      ..pausable = 5
      ..owner = 6
      ..from = 0.75;
    _settings.ref
      ..volume = 0.125
      ..pitch = 2
      ..looping = 8;
    _voice.ref
      ..playing = 7
      ..time = 1.25;
    final seen = [
      _playFloats[0],
      _playFloats[1],
      _playWords[2],
      _playWords[3],
      _playWords[4],
      _playOwner.value,
      _playFrom.value,
      _settingsFloats[0],
      _settingsFloats[1],
      _settingsWords[2],
      _voicePlaying.value,
      _voiceTime.value,
    ];
    if (seen.join(' ') != '0.25 0.5 3 4 5 6 0.75 0.125 2.0 8 7 1.25') {
      throw StateError('package:fynkel_audio reads its structs at the wrong places: $seen');
    }
  }

  /// Plays [clip] at the volume, pitch and start written to [_playFloats]
  /// and [_playFrom]; its voice's handle, or 0 if none was free.
  int _start(
    AudioClip clip, {
    required bool looping,
    required int priority,
    required bool pausable,
    required int owner,
  }) {
    _playWords
      ..[2] = looping ? 1 : 0
      ..[3] = priority
      ..[4] = pausable ? 1 : 0;
    _playOwner.value = owner;
    _check(fa_audio_play(_native, clip._native, _play, _handle));
    return _handle.value;
  }

  /// Sets [handle] to the volume and pitch written to [_settingsFloats].
  void _set(int handle, {required bool looping}) {
    _settingsWords[2] = looping ? 1 : 0;
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

  @override
  void afterFrame() {
    super.afterFrame();
    // Through memory: a double passed to a native call is boxed in the JIT.
    _seconds.value = world.time.unscaledDelta;
    _check(fa_audio_update(_native, _seconds, _count));
    final count = _count.value;
    if (count == 0) return;
    if (count > _stoppedRoom) {
      calloc.free(_stopped);
      _stoppedRoom = count;
      _stopped = calloc<FaStopped>(count);
    }
    _check(fa_stopped_read(_native, _stopped, count));
    for (var i = 0; i < count; i++) {
      final stopped = _stopped[i];
      final source = _sources[stopped.owner];
      if (source != null) source._ended(stopped.handle, stolen: stopped.stolen == 1);
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
    calloc
      ..free(_play)
      ..free(_settings)
      ..free(_voice)
      ..free(_count)
      ..free(_seconds)
      ..free(_handle)
      ..free(_stopped);
  }
}

void _checkUnit(double value, String name) {
  if (!(value >= 0 && value <= 1)) throw ArgumentError.value(value, name, 'is not between 0 and 1');
}

void _checkPitch(double value) {
  if (!(value >= 0.05 && value <= 8)) throw ArgumentError.value(value, 'pitch', 'is not between 0.05 and 8');
}

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
