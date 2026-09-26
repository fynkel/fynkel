// The Inspector's preview stages (authoring ABI v4, ADR-0030 §3): one
// asset alone in a small edit-mode world — a prefab or a model's
// hierarchy, a mesh with its material, a material on a shape — lit by the
// stage's own sun and framed the way the Project's tiles show it. The
// editor draws [PreviewStage.worldAddress] through its own viewport and
// camera; the stage shares the session's cooked store, so an unsaved
// material edit shows in it and a recook reloads it.

part of 'session.dart';

/// A camera fitting the staged asset, from the three-quarter view.
final class StageFraming {
  const StageFraming({
    required this.eye,
    required this.target,
    required this.fovY,
    required this.near,
    required this.far,
    required this.radius,
    required this.world,
  });

  factory StageFraming.fromJson(Map<String, Object?> j) => StageFraming(
    eye: _doubles(j['eye']),
    target: _doubles(j['target']),
    fovY: (j['fovY'] as num?)?.toDouble() ?? 0.5236,
    near: (j['near'] as num?)?.toDouble() ?? 0.01,
    far: (j['far'] as num?)?.toDouble() ?? 100,
    radius: (j['radius'] as num?)?.toDouble() ?? 0.5,
    world: _doubles(j['world']),
  );

  final List<double> eye, target;

  /// Radians.
  final double fovY;
  final double near, far;

  /// The bounding sphere's radius: what an orbit keeps its distance by.
  final double radius;

  /// The camera's world matrix, 16 floats, column-major.
  final List<double> world;
}

/// What the staged meshes add up to (the preview's info line).
final class StageStats {
  const StageStats({
    this.meshes = 0,
    this.vertices = 0,
    this.triangles = 0,
    this.skinned = 0,
    this.boundsMin,
    this.boundsMax,
  });

  factory StageStats.fromJson(Map<String, Object?> j) {
    final b = _mapOf(j['bounds']);
    return StageStats(
      meshes: (j['meshes'] as num?)?.toInt() ?? 0,
      vertices: (j['vertices'] as num?)?.toInt() ?? 0,
      triangles: (j['triangles'] as num?)?.toInt() ?? 0,
      skinned: (j['skinned'] as num?)?.toInt() ?? 0,
      boundsMin: b.isEmpty ? null : _doubles(b['min']),
      boundsMax: b.isEmpty ? null : _doubles(b['max']),
    );
  }

  final int meshes, vertices, triangles, skinned;

  /// Null when nothing measurable is drawn.
  final List<double>? boundsMin, boundsMax;
}

/// A sound's format, as its stage reports it.
final class StageSound {
  const StageSound({
    required this.sampleRate,
    required this.channels,
    required this.frames,
    required this.duration,
    required this.streamed,
    required this.codec,
  });

  factory StageSound.fromJson(Map<String, Object?> j) => StageSound(
    sampleRate: (j['sampleRate'] as num?)?.toInt() ?? 0,
    channels: (j['channels'] as num?)?.toInt() ?? 1,
    frames: (j['frames'] as num?)?.toInt() ?? 0,
    duration: (j['duration'] as num?)?.toDouble() ?? 0,
    streamed: j['streamed'] == true,
    codec: j['codec'] as String? ?? 'pcm',
  );

  final int sampleRate, channels, frames;

  /// Seconds.
  final double duration;

  /// Decoded as it plays (the sidecar's `stream: true`).
  final bool streamed;

  /// `pcm` or `vorbis`.
  final String codec;
}

/// A waveform: per channel, per column, the lowest and highest sample.
final class SoundPeaks {
  const SoundPeaks(this.channels, this.columns, this.values);

  final int channels, columns;

  /// `[channel][column][min, max]` flattened.
  final Float32List values;

  double min(int channel, int column) => values[(channel * columns + column) * 2];
  double max(int channel, int column) => values[(channel * columns + column) * 2 + 1];
}

/// Where a sound stage's playback is.
final class SoundPlayhead {
  const SoundPlayhead({
    required this.playing,
    required this.time,
    required this.duration,
    required this.loop,
    required this.device,
  });

  factory SoundPlayhead.fromJson(Map<String, Object?> j) => SoundPlayhead(
    playing: j['playing'] == true,
    time: (j['time'] as num?)?.toDouble() ?? 0,
    duration: (j['duration'] as num?)?.toDouble() ?? 0,
    loop: j['loop'] == true,
    device: j['device'] == true,
  );

  final bool playing;

  /// Seconds into the sound.
  final double time, duration;
  final bool loop;

  /// Whether it is heard (an output device is open).
  final bool device;
}

/// A stage's state after a call that (re)loaded or reported it.
final class StageState {
  const StageState({
    required this.subject,
    required this.shape,
    required this.framing,
    required this.light,
    required this.stats,
    this.diagnostics = const [],
    this.sound,
  });

  factory StageState.fromJson(Map<String, Object?> j) => StageState(
    sound: j['sound'] is Map<String, Object?> ? StageSound.fromJson(j['sound'] as Map<String, Object?>) : null,
    subject: j['subject'] as String? ?? 'hierarchy',
    shape: j['shape'],
    framing: StageFraming.fromJson(_mapOf(j['framing'])),
    light: _doubles(j['light']),
    stats: StageStats.fromJson(_mapOf(j['stats'])),
    diagnostics: _strings(j['diagnostics']),
  );

  /// `hierarchy`, `mesh`, `material` or `sound`.
  final String subject;

  /// A sound stage's format (it has no world).
  final StageSound? sound;

  /// A material's: `sphere`, `cube`, `cylinder`, `capsule`, `plane`, or
  /// `{asset, sub}` for a mesh; null for the others.
  final Object? shape;
  final StageFraming framing;

  /// Toward the stage's sun.
  final List<double> light;
  final StageStats stats;
  final List<String> diagnostics;
}

/// One open stage, from [AuthoringSession.openStage]. Dead after [close],
/// the project's close or the session's dispose.
final class PreviewStage {
  PreviewStage._(this.session, this.id, this.state, this.cook);

  final AuthoringSession session;
  final int id;

  /// After the last call that loaded or reported it.
  StageState state;

  /// The cook that opened the cache, when this stage ran it.
  final CookReport? cook;

  bool _closed = false;
  bool get isClosed => _closed;

  /// The native `FynkelWorld*` for the viewport ABI; 0 when closed.
  int get worldAddress => _closed || session._disposed
      ? 0
      : n.fynkel_authoring_stage_world(session._ptr, id).address;

  /// A material's stage on another shape: a built-in's name, or a mesh as
  /// `{'asset': id, 'sub': 'mesh/…'}`. Throws when the mesh does not load
  /// (the stage keeps its shape).
  StageState setShape(Object shape) =>
      state = StageState.fromJson(_call('shape', {'shape': shape}));

  /// Turns the stage's sun like an orbit (radians); the new direction.
  List<double> turnLight(double yaw, double pitch) =>
      _doubles(_call('light', {'yaw': yaw, 'pitch': pitch})['light']);

  /// Points the stage's sun from [toward].
  List<double> setLight(List<double> toward) =>
      _doubles(_call('light', {'toward': toward})['light']);

  /// Loaded again from the store (the asset changed on disk).
  StageState reload() => state = StageState.fromJson(_call('reload'));

  /// The counts now, without a reload.
  StageState refresh() => state = StageState.fromJson(_call('stats'));

  /// A material document's unsaved fields drawn by every world of the
  /// session (the Scene views' and every stage's).
  void setMaterial(String material, Map<String, Object?> data) =>
      _call('setMaterial', {'material': material, 'data': data});

  void restoreMaterial(String material) =>
      _call('restoreMaterial', {'material': material});

  /// A sound stage's waveform in [columns] columns.
  SoundPeaks peaks(int columns) {
    final r = _call('peaks', {'columns': columns});
    return SoundPeaks(
      (r['channels'] as num).toInt(),
      (r['columns'] as num).toInt(),
      Float32List.fromList(_doubles(r['peaks'])),
    );
  }

  /// Plays a sound stage [from] a time (seconds), looping or not; with
  /// [device] false it is not heard (tests), its clock still runs.
  SoundPlayhead play({double from = 0, bool? loop, bool device = true}) =>
      SoundPlayhead.fromJson(_call('play', {'from': from, 'loop': ?loop, 'device': device}));

  SoundPlayhead stop() => SoundPlayhead.fromJson(_call('stop'));

  SoundPlayhead setLoop(bool loop) => SoundPlayhead.fromJson(_call('loop', {'loop': loop}));

  /// Where the playback is now.
  SoundPlayhead playhead() => SoundPlayhead.fromJson(_call('playhead'));

  void close() {
    if (_closed || session._disposed) return;
    session._call(
      'stageClose',
      () => n.fynkel_authoring_stage_close(session._ptr, id),
    );
    _closed = true;
    session._stages.remove(id);
  }

  Map<String, Object?> _call(
    String kind, [
    Map<String, Object?> args = const {},
  ]) {
    if (_closed) throw StateError('stage $id is closed');
    return session._kindAndArgs(
      'stage $kind',
      kind,
      args,
      (ptr, k, kl, a, al) =>
          n.fynkel_authoring_stage_call(ptr, id, k, kl, a, al),
    );
  }
}

extension PreviewStages on AuthoringSession {
  /// Opens a stage for the asset [asset] (a sub-asset by [sub]: `mesh/…`,
  /// `material/…`), a material on [shape]. The project is cooked first
  /// when nothing opened the cache yet. Throws [AuthoringException] with
  /// FYN-T0003 for an asset with nothing to stage (a texture, a sound) or
  /// one whose cook failed.
  PreviewStage openStage(String asset, {String? sub, Object? shape}) {
    final r = _text(
      'stageOpen',
      jsonEncode({'asset': asset, 'sub': ?sub, 'shape': ?shape}),
      (buf, len) => n.fynkel_authoring_stage_open(_ptr, buf, len),
    );
    final cook = r['cook'] is Map<String, Object?>
        ? CookReport.fromJson(r['cook'] as Map<String, Object?>)
        : null;
    final stage = PreviewStage._(
      this,
      (r['stage'] as num).toInt(),
      StageState.fromJson(r),
      cook,
    );
    _stages[stage.id] = stage;
    return stage;
  }
}

List<double> _doubles(Object? v) => v is List
    ? [
        for (final x in v)
          if (x is num) x.toDouble(),
      ]
    : const [];
