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

  /// Where the track is, in seconds, as [AudioSource.playTime]: set it to
  /// play on from there, or, when none plays, for the next track to start
  /// there. Past the track's end throws an [ArgumentError].
  double get playTime => _voice == 0 ? _from : _audio._timeOf(_voice);
  set playTime(double value) {
    if (!(value >= 0 && value.isFinite)) throw ArgumentError.value(value, 'playTime', 'is not 0 or more');
    final clip = _clip;
    if (clip != null && value > clip.length) {
      throw ArgumentError.value(value, 'playTime', 'is past the end of $clip');
    }
    _from = value;
    if (_voice == 0 || clip == null || _audio._disposed) return;
    // Started again from there, as the track it is.
    _check(fa_voice_stop(_audio._native, _voice));
    _voice = 0;
    _start(clip, 0, looping: _trackLoop, pausable: _trackPausable, group: _trackGroup);
  }

  double _from = 0;

  /// The group it plays in: `audio.group('music')` unless set. From the
  /// next track. Another world's group throws an [ArgumentError].
  AudioGroup get group => _group ??= _audio.group('music');
  set group(AudioGroup value) {
    _audio._indexOf(value);
    _group = value;
  }

  AudioGroup? _group;

  // What the playing track started with: [loop], [ignorePause] and
  // [group] are from the next track.
  var _trackLoop = true;
  var _trackPausable = false;
  var _trackGroup = 0;

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
    _audio._set(_voice, looping: _trackLoop, group: _trackGroup);
  }

  var _volume = 1.0;

  /// Called when a track that doesn't [loop] reaches its end.
  void Function(AudioClip clip)? onEnd;

  /// Plays [clip], fading it in over [fadeIn] seconds (0 unless given),
  /// and stops what played before. If [clip] is playing already, it plays
  /// on: a level that starts its music again doesn't restart it.
  void play(AudioClip clip, {double fadeIn = 0}) {
    _checkFade(fadeIn, 'fadeIn');
    if (identical(clip, _clip) && isPlaying) return;
    stop();
    _startNext(clip, fadeIn);
  }

  /// Fades what plays out over [seconds] while [clip] fades in over the
  /// same. If [clip] is playing already, it plays on.
  void crossFade(AudioClip clip, double seconds) {
    if (identical(clip, _clip) && isPlaying) return;
    _checkFade(seconds, 'seconds');
    if (_voice != 0) _audio._fadeVoice(_voice, 0, seconds, stop: true);
    _voice = 0;
    _startNext(clip, seconds);
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

  /// Starts [clip] as the next track, with [loop], [ignorePause] and
  /// [group] as they are now.
  void _startNext(AudioClip clip, double fadeIn) =>
      _start(clip, fadeIn, looping: loop, pausable: !ignorePause, group: _audio._indexOf(group));

  void _start(AudioClip clip, double fadeIn, {required bool looping, required bool pausable, required int group}) {
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
    audio._playFrom.value = _from < clip.length ? _from : clip.length;
    _voice = audio._start(
      clip,
      looping: looping,
      priority: 0,
      pausable: pausable,
      owner: _owner,
      group: group,
      fading: true,
    );
    _from = 0;
    _clip = _voice == 0 ? null : clip;
    _trackLoop = looping;
    _trackPausable = pausable;
    _trackGroup = group;
  }

  /// [handle] stopped on its own: at its end, or [stolen], which music
  /// never is (it plays at priority 0), and which isn't an end.
  void _ended(int handle, {required bool stolen}) {
    if (handle != _voice) return;
    final clip = _clip;
    _voice = 0;
    _clip = null;
    if (clip == null || stolen) return;
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
