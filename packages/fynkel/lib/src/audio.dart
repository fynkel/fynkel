// Audio value types (M3-11, docs/audio.md): what a game reads back from a
// `fynkel.AudioSource` and from the engine, and what a load registered.
// The source and the listener themselves are the native components
// `AudioSource`/`AudioListener` (lib/generated/schema.g.dart), attached
// in a scene or through `GameObject.add`/`set`; playback goes through
// `GameObject.audio`, the engine through `World.audio`.

import '../generated/schema.g.dart' as gen;
import 'asset_ref.dart';

export '../generated/schema.g.dart' show AudioBus, AudioListener, AudioOp, AudioSource;

/// A source's playback as the last tick left it (`GameObject.audio.state`,
/// `FYNKEL_QUERY_AUDIO`).
class AudioState {
  const AudioState({
    required this.clip,
    required this.bus,
    required this.playing,
    required this.paused,
    required this.time,
    required this.duration,
    required this.volume,
    required this.pitch,
    required this.looping,
    required this.voice,
  });

  factory AudioState.fromRecord(gen.AudioStateRecord r) => AudioState(
        clip: r.clip,
        bus: r.bus,
        playing: r.playing,
        paused: r.paused,
        time: r.time,
        duration: r.duration,
        volume: r.volume,
        pitch: r.pitch,
        looping: r.looping,
        voice: r.voice,
      );

  final AssetRef clip;
  final gen.AudioBus bus;

  /// The source holds a voice (paused or not); false before a play, after
  /// `stop`, after a non-looping clip's end, or when the engine stole or
  /// refused the voice.
  final bool playing;
  final bool paused;

  /// Seconds into [clip], `0 ≤ time < duration` (wraps while looping).
  final double time;

  /// The clip's length in seconds (0 while the clip is not registered).
  final double duration;
  final double volume;
  final double pitch;
  final bool looping;

  /// The engine's voice handle (0 without one); a stolen voice's handle
  /// stays unique.
  final int voice;

  /// [time] as a fraction of [duration] (0 for an unregistered clip).
  double get progress => duration > 0 ? time / duration : 0;

  @override
  String toString() =>
      'AudioState($clip ${time.toStringAsFixed(3)}/${duration.toStringAsFixed(3)} s ×$pitch vol $volume'
      '${playing ? '' : ', stopped'}${paused ? ', paused' : ''}${looping ? ', looping' : ''})';
}

/// The engine's counters (`World.audio.stats`, `FYNKEL_QUERY_AUDIO_STATS`).
class AudioStats {
  const AudioStats({
    required this.device,
    required this.sampleRate,
    required this.channels,
    required this.voices,
    required this.maxVoices,
    required this.started,
    required this.stolen,
    required this.refused,
    required this.ended,
    required this.clips,
    required this.callbacks,
    required this.underruns,
    required this.suspended,
    required this.streams,
    required this.starved,
  });

  factory AudioStats.fromRecord(gen.AudioStatsRecord r) => AudioStats(
        device: r.device,
        sampleRate: r.sampleRate,
        channels: r.channels,
        voices: r.voices,
        maxVoices: r.maxVoices,
        started: r.started,
        stolen: r.stolen,
        refused: r.refused,
        ended: r.ended,
        clips: r.clips,
        callbacks: r.callbacks,
        underruns: r.underruns,
        suspended: r.suspended,
        streams: r.streams,
        starved: r.starved,
      );

  /// A real output device is open (`World.audio.open`); the null backend
  /// otherwise — the clock runs, nothing is heard.
  final bool device;
  final int sampleRate;
  final int channels;

  /// Voices playing now, and the limit.
  final int voices;
  final int maxVoices;

  /// Voices started, stolen from a lower priority and refused (every
  /// voice outranked the newcomer) since the world was created.
  final int started;
  final int stolen;
  final int refused;

  /// Non-looping voices that reached their end.
  final int ended;

  /// Clips registered with the world.
  final int clips;

  /// Device callbacks rendered and underruns reported (0 without a device).
  final int callbacks;
  final int underruns;

  /// The device is paused (`World.audio.suspend`).
  final bool suspended;

  /// Of the clips, the streamed ones (a sidecar's `stream: true`: music
  /// decoded as it plays, not held in memory).
  final int streams;

  /// Device callbacks in which a streamed voice's decoder had fallen
  /// behind and the voice held silent for the rest of it. A steady
  /// count means the machine is too busy for the stream's one second of
  /// lead.
  final int starved;

  @override
  String toString() => 'AudioStats(${device ? '$sampleRate Hz × $channels' : 'null backend'}, '
      '$voices/$maxVoices voices, $started started, $stolen stolen, $refused refused, $ended ended, '
      '$clips clips${streams > 0 ? ' ($streams streamed, $starved starved)' : ''}${suspended ? ', suspended' : ''})';
}

/// What a load registered for the scene's audio sources and what the
/// engine looked like after it (`LoadReport.audio`).
class AudioAssets {
  const AudioAssets({
    this.sounds = 0,
    this.clips = 0,
    this.streams = 0,
    this.device = false,
    this.sampleRate = 0,
    this.channels = 0,
    this.maxVoices = 0,
  });

  factory AudioAssets.fromJson(Map<String, Object?> j) => AudioAssets(
        sounds: (j['sounds'] as num?)?.toInt() ?? 0,
        clips: (j['clips'] as num?)?.toInt() ?? 0,
        streams: (j['streams'] as num?)?.toInt() ?? 0,
        device: j['device'] == true,
        sampleRate: (j['sampleRate'] as num?)?.toInt() ?? 0,
        channels: (j['channels'] as num?)?.toInt() ?? 0,
        maxVoices: (j['maxVoices'] as num?)?.toInt() ?? 0,
      );

  /// Sounds this load registered for the scene's `AudioSource`s.
  final int sounds;

  /// Clips the world holds after the load, and how many of them stream
  /// (a sidecar's `stream: true`).
  final int clips;
  final int streams;

  /// The output device state after the load ([sampleRate] and [channels]
  /// are the null backend's 48 kHz stereo without one).
  final bool device;
  final int sampleRate;
  final int channels;
  final int maxVoices;

  @override
  String toString() =>
      'audio($sounds sounds, $clips clips${streams > 0 ? ' ($streams streamed)' : ''}, ${device ? '$sampleRate Hz × $channels' : 'no device'})';
}
