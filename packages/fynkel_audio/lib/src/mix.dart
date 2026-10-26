part of 'audio.dart';

/// Sounds whose volume is set together, as a group of Unity's
/// `AudioMixer`: a settings screen's "music" and "effects". Made by name
/// with `audio.group('music')`, under [Audio.volume]. Put a source in one
/// with [AudioSource.group]; [Audio.music] plays in `audio.group('music')`.
///
/// ```dart
/// final effects = audio.group('effects');
/// dog.get<AudioSource>()!.group = effects;
/// effects.volume = 0.5; // from a slider
/// audio.group('music').fade(0.2, 1); // quieter under a cutscene's voices
/// ```
///
/// A change is heard within a few milliseconds, without a click.
final class AudioGroup {
  AudioGroup._(this._audio, this.name, this._index);

  final Audio _audio;
  final int _index;

  /// What it was made as.
  final String name;

  /// Its volume, 0 to 1, over its sounds' own: 1 unless set. Throws an
  /// [ArgumentError] outside 0 to 1.
  double get volume => _volume;
  set volume(double value) {
    _checkUnit(value, 'volume');
    _volume = value;
    _send(0);
  }

  var _volume = 1.0;

  /// Silences its sounds while they go on, keeping its [volume].
  bool get mute => _mute;
  set mute(bool value) {
    _mute = value;
    _send(0);
  }

  var _mute = false;

  /// Fades its [volume] to [to] over [seconds] (up to an hour): [volume]
  /// is [to] from now.
  void fade(double to, double seconds) {
    _checkUnit(to, 'to');
    _checkFade(seconds, 'seconds');
    _volume = to;
    _send(seconds);
  }

  void _send(double seconds) => _audio._fadeGroup(_index, _mute ? 0.0 : _volume, seconds);

  @override
  String toString() => "AudioGroup('$name', ${_mute ? 'muted' : _volume.toStringAsFixed(2)})";
}

/// The world's music (`audio.music`): one track at a time, looping, in
/// `audio.group('music')`, crossfaded from one to the next. Long tracks
/// stream ([AudioClip.streamed]), so a three-minute song takes its file's
/// memory, not 30 MB.
///
/// ```dart
/// final calm = await assets.audio('music/calm.ogg');
/// audio.music.play(calm, fadeIn: 1);
/// // Later, a chase:
/// audio.music.crossFade(await assets.audio('music/chase.ogg'), 2);
/// ```
///
/// It plays on while the game is [World.paused] (a pause menu's music)
/// unless it doesn't [ignorePause], and is never given up for other
/// sounds: it plays at priority 0 ([AudioSource.priority]), even while it
/// fades out.
final class AudioMusic {
  AudioMusic._(this._audio) : _owner = _audio._nextOwner++;

  final Audio _audio;
  final int _owner;
  var _voice = 0;
  AudioClip? _clip;

  /// What it plays, or null.
  AudioClip? get clip => _clip;

  /// Whether a track plays: not stopped, nor stopping, nor ended.
  bool get isPlaying => _voice != 0 && _audio._read(_voice);

  /// Where the track is, in seconds; 0 when none plays.
  double get time => _voice == 0 ? 0 : _audio._timeOf(_voice);

  /// The group it plays in: `audio.group('music')` unless set. From the
  /// next track.
  late AudioGroup group = _audio.group('music');

  /// Whether a track starts again at its end; true unless set. From the
  /// next track.
  bool loop = true;

  /// Whether it plays on while the game is [World.paused]; true unless
  /// set. From the next track.
  bool ignorePause = true;

  /// Its own volume, 0 to 1, under its [group]'s: 1 unless set.
  double get volume => _volume;
  set volume(double value) {
    _checkUnit(value, 'volume');
    _volume = value;
    if (_voice == 0) return;
    _audio._settingsFloats
      ..[0] = value
      ..[1] = 1;
    _audio._set(_voice, looping: loop, group: group._index);
  }

  var _volume = 1.0;

  /// Called when a track that doesn't [loop] reaches its end.
  void Function(AudioClip clip)? onEnd;

  /// Plays [clip], fading it in over [fadeIn] seconds (0 unless given),
  /// and stops what played before. If [clip] is playing already, it plays
  /// on: a level that starts its music again doesn't restart it.
  void play(AudioClip clip, {double fadeIn = 0}) {
    if (identical(clip, _clip) && isPlaying) return;
    stop();
    _start(clip, fadeIn);
  }

  /// Fades what plays out over [seconds] while [clip] fades in over the
  /// same. If [clip] is playing already, it plays on.
  void crossFade(AudioClip clip, double seconds) {
    if (identical(clip, _clip) && isPlaying) return;
    _checkFade(seconds, 'seconds');
    if (_voice != 0) _audio._fadeVoice(_voice, 0, seconds, stop: true);
    _voice = 0;
    _start(clip, seconds);
  }

  /// Stops the track, fading it out over [fadeOut] seconds (or a few
  /// milliseconds, unless given).
  void stop({double fadeOut = 0}) {
    _checkFade(fadeOut, 'fadeOut');
    final voice = _voice;
    _voice = 0;
    _clip = null;
    if (voice == 0 || _audio._disposed) return;
    if (fadeOut > 0) {
      _audio._fadeVoice(voice, 0, fadeOut, stop: true);
    } else {
      _check(fa_voice_stop(_audio._native, voice));
    }
  }

  void _start(AudioClip clip, double fadeIn) {
    _checkFade(fadeIn, 'fadeIn');
    final audio = _audio;
    audio._playFloats
      ..[0] = _volume
      ..[1] = 1
      ..[10] = 0
      ..[11] = 0
      ..[12] = 1
      ..[13] = 500
      ..[15] = 0
      ..[16] = 0
      ..[17] = 0
      ..[18] = fadeIn;
    audio._playWords[14] = 0;
    audio._playFrom.value = 0;
    _voice = audio._start(
      clip,
      looping: loop,
      priority: 0,
      pausable: !ignorePause,
      owner: _owner,
      group: group._index,
      fading: true,
    );
    _clip = _voice == 0 ? null : clip;
  }

  void _ended(int handle) {
    if (handle != _voice) return;
    final clip = _clip;
    _voice = 0;
    _clip = null;
    if (clip == null) return;
    try {
      onEnd?.call(clip);
    } catch (error, stack) {
      _audio.world.report(null, error, stack);
    }
  }

  @override
  String toString() => 'AudioMusic(${_clip?.name ?? 'nothing'})';
}

void _checkFade(double seconds, String name) {
  if (!(seconds >= 0 && seconds <= 3600)) throw ArgumentError.value(seconds, name, 'is not 0 to 3600');
}
