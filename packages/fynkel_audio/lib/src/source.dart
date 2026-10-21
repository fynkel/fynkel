part of 'audio.dart';

/// Plays sounds from its object, as Unity's `AudioSource`: [clip] with
/// [play], and any clip on top with [playOneShot] (footsteps, hits).
///
/// ```dart
/// final voice = dog.addComponent(AudioSource(clip: await assets.audio('bark.ogg'), playOnAwake: false));
/// voice.play();
/// voice.playOneShot(await assets.audio('step.ogg'), pitch: 1.1);
/// ```
///
/// With [playOnAwake] (the default) it plays [clip] when it's enabled.
/// Disabling it, or destroying its object, stops all its sounds. While the
/// game is [World.paused] its sounds hold where they are, unless it
/// [ignorePause]s (a pause menu's).
final class AudioSource extends Behaviour {
  /// A source of [clip], with Unity's defaults. A bad value throws an
  /// [ArgumentError].
  AudioSource({
    this.clip,
    double volume = 1,
    double pitch = 1,
    this._loop = false,
    this.playOnAwake = true,
    this._mute = false,
    int priority = 128,
    this.ignorePause = false,
    this._followTimeScale = false,
  }) : _volume = volume,
       _pitch = pitch,
       _priority = priority {
    _checkUnit(volume, 'volume');
    _checkPitch(pitch);
    _checkPriority(priority);
  }

  /// What [play] plays. Setting it doesn't change what's playing: the next
  /// [play] plays the new one.
  AudioClip? clip;

  /// Whether it plays [clip] when it's enabled: when its object comes into
  /// the world, and each time it's enabled again.
  bool playOnAwake;

  /// Whether its sounds play on while the game is [World.paused] (Unity's
  /// `ignoreListenerPause`): for a pause menu's sounds. From the next
  /// sound it plays.
  bool ignorePause;

  /// How loud, 0 to 1: its sounds, the one-shots too, follow it at once.
  double get volume => _volume;
  set volume(double value) {
    _checkUnit(value, 'volume');
    _volume = value;
    _sendSettings();
  }

  double _volume;

  /// How fast, and so how high, 0.05 to 8: 1 is the clip's own, 2 an
  /// octave up and twice as fast.
  double get pitch => _pitch;
  set pitch(double value) {
    _checkPitch(value);
    _pitch = value;
    _sendSettings();
  }

  double _pitch;

  /// Whether [clip] starts again when it ends.
  bool get loop => _loop;
  set loop(bool value) {
    _loop = value;
    _sendSettings();
  }

  bool _loop;

  /// Silences it, as a volume of 0 would, while its sounds go on.
  bool get mute => _mute;
  set mute(bool value) {
    _mute = value;
    _sendSettings();
  }

  bool _mute;

  /// How important its sounds are, 0 (most) to 256 (least), as Unity's:
  /// when every voice is busy, the least important gives way. 128 unless
  /// set. From the next sound it plays.
  int get priority => _priority;
  set priority(int value) {
    _checkPriority(value);
    _priority = value;
  }

  int _priority;

  /// Whether its sounds go faster and higher with the game's `time.scale`,
  /// for slow motion, and hold while it's 0. Unity's don't; off unless
  /// set.
  bool get followTimeScale => _followTimeScale;
  set followTimeScale(bool value) {
    _followTimeScale = value;
    _sendSettings();
  }

  bool _followTimeScale;

  /// Called when [clip], played with [play], reaches its end; not when it's
  /// stopped, or gives way to a more important sound.
  void Function(AudioClip clip)? onEnd;

  // The world's sound and this source's id there while it has one; its
  // clip's voice, and what it plays; whether that's paused; and its
  // one-shots, with their own volume and pitch.
  Audio? _audio;
  int _owner = 0;
  int _voice = 0;
  AudioClip? _playing;
  var _paused = false;
  var _held = false;
  final _shots = <int>[];
  final _shotVolumes = <double>[];
  final _shotPitches = <double>[];
  double _from = 0;

  /// Whether [clip] is playing: started with [play], not paused, stopped
  /// or ended. One-shots don't count, as in Unity.
  bool get isPlaying {
    final audio = _audio;
    if (audio == null || _voice == 0 || _paused) return false;
    return audio._read(_voice);
  }

  /// Where [clip] is, in seconds, while it plays or is paused (Unity's
  /// `AudioSource.time`; `time` is the game's here); set it to play on
  /// from there, or, when it isn't playing, to start there next.
  double get playTime {
    final audio = _audio;
    if (audio == null || _voice == 0) return _from;
    return audio._read(_voice) ? audio._voice.ref.time : 0;
  }

  set playTime(double value) {
    if (!(value >= 0 && value.isFinite)) throw ArgumentError.value(value, 'playTime', 'is not 0 or more');
    _from = value;
    if (_voice != 0 && _playing != null) {
      final paused = _paused;
      play();
      if (paused) pause();
    }
  }

  Audio get _system {
    final audio = _audio;
    if (audio != null) return audio;
    final made = gameObject.audio;
    _audio = made;
    _owner = made._nextOwner++;
    made._sources[_owner] = this;
    return made;
  }

  /// Plays [clip] from the start (or from [playTime], if set while it wasn't
  /// playing), stopping what it played before. Throws a [StateError]
  /// without a clip.
  void play() {
    final clip = this.clip;
    if (clip == null) throw StateError('$this has no clip to play');
    final audio = _system;
    if (_voice != 0) fa_voice_stop(audio._native, _voice);
    _voice = audio._start(
      clip,
      volume: _effectiveVolume,
      pitch: _effectivePitch,
      looping: _loop,
      priority: _priority,
      pausable: !ignorePause,
      owner: _owner,
      from: _from.clamp(0, clip.length),
    );
    _from = 0;
    _playing = _voice == 0 ? null : clip;
    _paused = false;
    _held = false;
    _hold();
  }

  /// Pauses [clip] where it is.
  void pause() {
    if (_voice == 0 || _paused) return;
    _paused = true;
    _check(fa_voice_pause(_system._native, _voice, 1));
  }

  /// Plays a paused [clip] on from where it was.
  void unPause() {
    if (_voice == 0 || !_paused) return;
    _paused = false;
    if (!_held) _check(fa_voice_pause(_system._native, _voice, 0));
  }

  /// Stops [clip], with a short fade, and its one-shots.
  void stop() {
    final audio = _audio;
    if (audio == null || audio._disposed) return;
    _check(fa_owner_stop(audio._native, _owner));
    _voice = 0;
    _playing = null;
    _paused = false;
    _shots.clear();
    _shotVolumes.clear();
    _shotPitches.clear();
  }

  /// Plays [clip] once, over whatever else plays (Unity's `PlayOneShot`):
  /// at [volume] times this source's, and [pitch] times its. It can't be
  /// paused or stopped on its own; [stop] stops it.
  void playOneShot(AudioClip clip, {double volume = 1, double pitch = 1}) {
    _checkUnit(volume, 'volume');
    if (!(pitch > 0 && pitch <= 8)) throw ArgumentError.value(pitch, 'pitch', 'is not above 0 and up to 8');
    final audio = _system;
    final handle = audio._start(
      clip,
      volume: _effectiveVolume * volume,
      pitch: (_effectivePitch * pitch).clamp(0.05, 8),
      looping: false,
      priority: _priority,
      pausable: !ignorePause,
      owner: _owner,
    );
    if (handle == 0) return;
    _shots.add(handle);
    _shotVolumes.add(volume);
    _shotPitches.add(pitch);
    if (_held) _check(fa_voice_pause(audio._native, handle, 1));
  }

  double get _effectiveVolume => _mute ? 0 : _volume;

  double get _scale => _followTimeScale ? (_audio?.world ?? gameObject.world).time.scale : 1.0;

  double get _effectivePitch => (_pitch * _scale).clamp(0.05, 8);

  /// Holds its sounds while it follows a time scale too slow to hear, and
  /// lets them go after.
  void _hold() {
    final held = _followTimeScale && _scale < 0.05;
    if (held == _held) return;
    _held = held;
    final audio = _system;
    if (_voice != 0 && !_paused) _check(fa_voice_pause(audio._native, _voice, held ? 1 : 0));
    for (final shot in _shots) {
      _check(fa_voice_pause(audio._native, shot, held ? 1 : 0));
    }
  }

  void _sendSettings() {
    final audio = _audio;
    if (audio == null) return;
    final volume = _effectiveVolume, pitch = _effectivePitch;
    if (_voice != 0) audio._set(_voice, volume: volume, pitch: pitch, looping: _loop);
    for (var i = 0; i < _shots.length; i++) {
      audio._set(
        _shots[i],
        volume: volume * _shotVolumes[i],
        pitch: (pitch * _shotPitches[i]).clamp(0.05, 8),
        looping: false,
      );
    }
    _hold();
  }

  /// [handle], one of its voices, stopped on its own.
  void _ended(int handle, {required bool stolen}) {
    if (handle == _voice) {
      final clip = _playing;
      _voice = 0;
      _playing = null;
      _paused = false;
      if (!stolen && clip != null) {
        try {
          onEnd?.call(clip);
        } catch (error, stack) {
          world.report(gameObject, error, stack);
        }
      }
      return;
    }
    final i = _shots.indexOf(handle);
    if (i < 0) return;
    _shots.removeAt(i);
    _shotVolumes.removeAt(i);
    _shotPitches.removeAt(i);
  }

  @override
  void onEnable() {
    super.onEnable();
    _system;
    if (playOnAwake && clip != null && _voice == 0) play();
  }

  @override
  void onDisable() {
    super.onDisable();
    stop();
    final audio = _audio;
    if (audio != null && !audio._disposed) audio._sources.remove(_owner);
    _audio = null;
  }

  @override
  String toString() => 'AudioSource(${clip?.name ?? 'no clip'})';
}
