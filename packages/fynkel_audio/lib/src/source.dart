part of 'audio.dart';

/// How a 3D sound fades with distance, as Unity's `AudioRolloffMode`.
enum AudioRolloffMode {
  /// Full within [AudioSource.minDistance], then falling as 1 over the
  /// distance (twice as far, half as loud), until
  /// [AudioSource.maxDistance], where it stops falling: as sound does.
  logarithmic,

  /// Full within [AudioSource.minDistance], falling in a straight line to
  /// silence at [AudioSource.maxDistance]: easier to place.
  linear,
}

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
///
/// It's 2D unless given a [spatialBlend]: the same in both ears wherever
/// it is, as music and a menu's clicks should be. At a [spatialBlend] of
/// 1 it's 3D, heard from where its object is: louder in the ear it's on
/// the side of, fading with distance ([minDistance], [maxDistance],
/// [rolloffMode]), a little quieter and duller behind, as the
/// [AudioListener] (or the camera) hears it, as it moves.
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
    double spatialBlend = 0,
    double panStereo = 0,
    double minDistance = 1,
    double maxDistance = 500,
    this._rolloffMode = AudioRolloffMode.logarithmic,
  }) : _volume = volume,
       _pitch = pitch,
       _priority = priority,
       _spatialBlend = spatialBlend,
       _panStereo = panStereo,
       _minDistance = minDistance,
       _maxDistance = maxDistance {
    _checkUnit(volume, 'volume');
    _checkPitch(pitch);
    _checkPriority(priority);
    _checkUnit(spatialBlend, 'spatialBlend');
    _checkPan(panStereo);
    _checkDistances(minDistance, maxDistance);
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
  /// when every voice is busy, the least important gives way. A sound of
  /// priority 0 never does, as [Audio.music]'s, so one is for music
  /// alone. 128 unless set. From the next sound it plays.
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

  /// 0 for 2D (the same wherever it is), 1 for 3D (heard from where its
  /// object is), or between for a blend, as Unity's. Its sounds follow it
  /// at once.
  double get spatialBlend => _spatialBlend;
  set spatialBlend(double value) {
    _checkUnit(value, 'spatialBlend');
    _spatialBlend = value;
    _sendSpatial();
  }

  double _spatialBlend;

  /// Left (-1) to right (1), for the 2D part of its sound: Unity's
  /// `panStereo`. 0 unless set.
  double get panStereo => _panStereo;
  set panStereo(double value) {
    _checkPan(value);
    _panStereo = value;
    _sendSpatial();
  }

  double _panStereo;

  /// Within this many metres a 3D sound is at full volume; 1 unless set.
  /// Above 0, and below [maxDistance].
  double get minDistance => _minDistance;
  set minDistance(double value) {
    _checkDistances(value, _maxDistance);
    _minDistance = value;
    _sendSpatial();
  }

  double _minDistance;

  /// Where a 3D sound stops fading ([AudioRolloffMode.logarithmic]) or
  /// falls silent ([AudioRolloffMode.linear]); 500 unless set.
  double get maxDistance => _maxDistance;
  set maxDistance(double value) {
    _checkDistances(_minDistance, value);
    _maxDistance = value;
    _sendSpatial();
  }

  double _maxDistance;

  /// How a 3D sound fades between [minDistance] and [maxDistance].
  AudioRolloffMode get rolloffMode => _rolloffMode;
  set rolloffMode(AudioRolloffMode value) {
    _rolloffMode = value;
    _sendSpatial();
  }

  AudioRolloffMode _rolloffMode;

  /// The group whose volume its sounds are under ([AudioGroup]), such as
  /// `audio.group('effects')`, or none: [Audio.volume]'s alone (Unity's
  /// `outputAudioMixerGroup`). Its sounds follow it at once.
  AudioGroup? get group => _group;
  set group(AudioGroup? value) {
    _group = value;
    _sendSettings();
  }

  AudioGroup? _group;

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
  // Fixed room for as many as the mixer has voices: a list that grows and
  // shrinks as shots come and go would make garbage each time.
  final _shots = Int64List(Audio.maxVoices);
  final _shotVolumes = Float64List(Audio.maxVoices);
  final _shotPitches = Float64List(Audio.maxVoices);
  var _shotCount = 0;
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
    return audio._timeOf(_voice);
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
    audio._playFloats
      ..[0] = _effectiveVolume
      ..[1] = _effectivePitch;
    audio._playFrom.value = _from < clip.length ? _from : clip.length;
    audio._writeSpatial(this);
    _voice = audio._start(
      clip,
      looping: _loop,
      priority: _priority,
      pausable: !ignorePause,
      owner: _owner,
      group: _indexOf(_group),
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
    _shotCount = 0;
  }

  /// Plays [clip] once, over whatever else plays (Unity's `PlayOneShot`):
  /// at [volume] times this source's, and [pitch] times its (each 1
  /// unless given). It can't be paused or stopped on its own; [stop] stops
  /// it.
  void playOneShot(AudioClip clip, {double? volume, double? pitch}) {
    // Nullable, not defaulted, and checked here, the error given the
    // value passed: the JIT boxes a double with a default, one passed to a
    // call it doesn't inline, or one an error is made with, at every call.
    final gain = volume ?? 1.0, speed = pitch ?? 1.0;
    if (!(gain >= 0 && gain <= 1)) throw ArgumentError.value(volume, 'volume', 'is not between 0 and 1');
    if (!(speed > 0 && speed <= 8)) throw ArgumentError.value(pitch, 'pitch', 'is not above 0 and up to 8');
    final audio = _system;
    audio._playFloats
      ..[0] = _effectiveVolume * gain
      ..[1] = _clampPitch(_effectivePitch * speed);
    audio._playFrom.value = 0;
    audio._writeSpatial(this);
    final handle = audio._start(
      clip,
      looping: false,
      priority: _priority,
      pausable: !ignorePause,
      owner: _owner,
      group: _indexOf(_group),
    );
    if (handle == 0) return;
    // Full only when it took a voice of its own whose loss it hears of
    // after the frame: this one plays on, at the volume it started with.
    if (_shotCount == Audio.maxVoices) return;
    _shots[_shotCount] = handle;
    _shotVolumes[_shotCount] = gain;
    _shotPitches[_shotCount] = speed;
    _shotCount++;
    if (_held) _check(fa_voice_pause(audio._native, handle, 1));
  }

  double get _effectiveVolume => _mute ? 0 : _volume;

  double get _scale => _followTimeScale ? (_audio?.world ?? gameObject.world).time.scale : 1.0;

  // Inlined: the JIT boxes a double returned from a call it doesn't
  // inline.
  @pragma('vm:prefer-inline')
  double get _effectivePitch => _clampPitch(_pitch * _scale);

  /// Holds its sounds while it follows a time scale too slow to hear, and
  /// lets them go after.
  void _hold() {
    final held = _followTimeScale && _scale < 0.05;
    if (held == _held) return;
    _held = held;
    final audio = _system;
    if (_voice != 0 && !_paused) _check(fa_voice_pause(audio._native, _voice, held ? 1 : 0));
    for (var i = 0; i < _shotCount; i++) {
      _check(fa_voice_pause(audio._native, _shots[i], held ? 1 : 0));
    }
  }

  void _sendSettings() {
    final audio = _audio;
    if (audio == null) return;
    final volume = _effectiveVolume, pitch = _effectivePitch;
    final settings = audio._settingsFloats;
    final group = _indexOf(_group);
    if (_voice != 0) {
      settings
        ..[0] = volume
        ..[1] = pitch;
      audio._set(_voice, looping: _loop, group: group);
    }
    for (var i = 0; i < _shotCount; i++) {
      settings
        ..[0] = volume * _shotVolumes[i]
        ..[1] = _clampPitch(pitch * _shotPitches[i]);
      audio._set(_shots[i], looping: false, group: group);
    }
    _hold();
  }

  /// Sends its 2D and 3D settings to its sounds.
  void _sendSpatial() {
    final audio = _audio;
    if (audio == null || (_voice == 0 && _shotCount == 0)) return;
    audio._fillSpatial(this);
    if (_voice != 0) _check(fa_voice_spatial_set(audio._native, _voice, audio._spatial));
    for (var i = 0; i < _shotCount; i++) {
      _check(fa_voice_spatial_set(audio._native, _shots[i], audio._spatial));
    }
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
    for (var i = 0; i < _shotCount; i++) {
      if (_shots[i] != handle) continue;
      // The last takes its place: their order doesn't matter.
      _shotCount--;
      _shots[i] = _shots[_shotCount];
      _shotVolumes[i] = _shotVolumes[_shotCount];
      _shotPitches[i] = _shotPitches[_shotCount];
      return;
    }
  }

  @override
  void onEnable() {
    super.onEnable();
    _system._enabled.add(this);
    if (playOnAwake && clip != null && _voice == 0) play();
  }

  @override
  void onDisable() {
    super.onDisable();
    stop();
    final audio = _audio;
    if (audio != null && !audio._disposed) {
      audio._sources.remove(_owner);
      audio._enabled.remove(this);
    }
    _audio = null;
  }

  @override
  String toString() => 'AudioSource(${clip?.name ?? 'no clip'})';
}

/// [pitch] within what the mixer plays, without boxing (`num.clamp` does).
@pragma('vm:prefer-inline')
double _clampPitch(double pitch) => pitch < 0.05 ? 0.05 : (pitch > 8 ? 8 : pitch);

void _checkPan(double value) {
  if (!(value >= -1 && value <= 1)) throw ArgumentError.value(value, 'panStereo', 'is not between -1 and 1');
}

void _checkDistances(double min, double max) {
  if (!(min > 0 && min.isFinite)) throw ArgumentError.value(min, 'minDistance', 'is not above 0');
  if (!(max > min && max.isFinite)) throw ArgumentError.value(max, 'maxDistance', 'is not above minDistance ($min)');
}
