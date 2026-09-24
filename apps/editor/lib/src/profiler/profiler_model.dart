// The Profiler (roadmap Phase 2): the game's frame timings over time, as
// Unity's Profiler shows them. The numbers are the ones the engine already
// measures — `FynkelGameView`'s per-second stats windows, which reach the
// editor as the channel's `stats` records from either Play (the Game view
// in the editor, or Build and Run's process). Nothing is sampled here: a
// window is parsed once (the record's fields and its `summary` line of
// `key=value` pairs, a timer as `p50/p95/p99` ms) and kept in a ring.
import 'package:flutter/foundation.dart';

import '../play/play_session.dart';

/// One timer of a window: its percentiles in milliseconds.
typedef Percentiles = ({double p50, double p95, double p99});

/// One stats window (about a second of the game's loop).
final class ProfilerFrame {
  const ProfilerFrame({required this.index, required this.tick, required this.values, required this.timers});

  /// Parses a `stats` record; null when it has nothing to show.
  static ProfilerFrame? parse(Map<String, Object?> record, {required int index}) {
    final values = <String, double>{};
    final timers = <String, Percentiles>{};
    final summary = record['summary'];
    if (summary is String) {
      for (final pair in summary.split(' ')) {
        final at = pair.indexOf('=');
        if (at <= 0) continue;
        final key = pair.substring(0, at);
        final value = pair.substring(at + 1);
        final parts = value.split('/');
        if (parts.length == 3) {
          final n = parts.map(double.tryParse).toList();
          if (n.every((x) => x != null)) timers[key] = (p50: n[0]!, p95: n[1]!, p99: n[2]!);
        } else if (parts.length == 2) {
          // `shadow=casters/draws`: two counts, not a timer.
          final a = double.tryParse(parts[0]), b = double.tryParse(parts[1]);
          if (a != null && b != null) {
            values['$key.0'] = a;
            values['$key.1'] = b;
          }
        } else {
          final n = double.tryParse(value);
          if (n != null) values[key] = n;
        }
      }
    }
    for (final e in record.entries) {
      final v = e.value;
      if (v is num && e.key != 'seq' && e.key != 'tick') values[e.key] = v.toDouble();
    }
    if (values.isEmpty && timers.isEmpty) return null;
    final tick = record['tick'];
    return ProfilerFrame(index: index, tick: tick is int ? tick : null, values: values, timers: timers);
  }

  /// The window's position since the Profiler started (it survives Clear).
  final int index;
  final int? tick;
  final Map<String, double> values;
  final Map<String, Percentiles> timers;

  /// A timer at [pct] (`p50`, `p95`, `p99`), or a plain value.
  double? metric(String key, {String pct = 'p95'}) {
    final t = timers[key];
    if (t != null) {
      return switch (pct) {
        'p50' => t.p50,
        'p99' => t.p99,
        _ => t.p95,
      };
    }
    return values[key];
  }
}

/// One line of a chart.
final class ProfilerSeries {
  const ProfilerSeries(this.key, this.label, this.colorIndex);

  /// A timer or value key of [ProfilerFrame].
  final String key;
  final String label;

  /// Into the panel's palette.
  final int colorIndex;
}

enum ProfilerChart {
  cpu('CPU', 'ms'),
  gpu('GPU', 'ms'),
  rendering('Rendering', ''),
  memory('Memory', 'MB');

  const ProfilerChart(this.label, this.unit);
  final String label;
  final String unit;

  List<ProfilerSeries> get series => switch (this) {
    cpu => const [
      ProfilerSeries('work', 'Frame (simulate + render)', 0),
      ProfilerSeries('simulate', 'Simulate', 1),
      ProfilerSeries('behaviours', 'Behaviours (Dart)', 2),
      ProfilerSeries('native', 'Engine step', 3),
      ProfilerSeries('render', 'Render call', 4),
      ProfilerSeries('raster', 'Flutter raster', 5),
    ],
    gpu => const [
      ProfilerSeries('gpu_frame', 'GPU frame', 0),
      ProfilerSeries('gpu_shadow', 'Shadows', 1),
      ProfilerSeries('gpu_ao', 'Ambient occlusion', 2),
      ProfilerSeries('gpu_scene', 'Scene', 3),
      ProfilerSeries('gpu_post', 'Post', 4),
    ],
    rendering => const [
      ProfilerSeries('draws', 'Draw calls', 0),
      ProfilerSeries('instances', 'Instances', 1),
      ProfilerSeries('culled', 'Culled', 2),
      ProfilerSeries('entities', 'Entities', 3),
    ],
    memory => const [
      ProfilerSeries('mem_resident', 'Resident', 0),
      ProfilerSeries('mem_native', 'Engine (native)', 1),
      ProfilerSeries('mem_assets', 'Assets', 2),
      ProfilerSeries('mem_gpu', 'GPU', 3),
      ProfilerSeries('heap', 'Dart heap', 4),
    ],
  };

  /// A raw value in the chart's unit (bytes → MB).
  double scale(double v) => this == memory ? v / (1024 * 1024) : v;
}

/// The Profiler's state: the windows of the runs so far, what is recorded,
/// shown and selected.
final class ProfilerModel extends ChangeNotifier {
  ProfilerModel({required this.session, this.capacity = 600}) {
    session.addListener(_onSession);
  }

  final PlaySession session;

  /// Windows kept (600: ten minutes of a game's life).
  final int capacity;

  final List<ProfilerFrame> frames = [];

  /// Off: windows that arrive are dropped (Unity's Record button).
  bool recording = true;

  /// A new Play starts an empty chart (Unity's Clear on Play).
  bool clearOnPlay = true;

  ProfilerChart chart = ProfilerChart.cpu;

  /// Which percentile the timer lines show.
  String percentile = 'p95';

  /// The window the details show; null follows the newest.
  int? selected;

  Object? _seenStats;
  DateTime? _seenStart;
  bool _wasLive = false;
  int _next = 0;

  void _onSession() {
    if (live != _wasLive) {
      _wasLive = live;
      notifyListeners();
    }
    final start = session.startedAt;
    if (start != null && !identical(start, _seenStart)) {
      _seenStart = start;
      if (clearOnPlay && frames.isNotEmpty) {
        frames.clear();
        selected = null;
        notifyListeners();
      }
    }
    final stats = session.lastStats;
    if (stats == null || identical(stats, _seenStats)) return;
    _seenStats = stats;
    if (!recording) return;
    final frame = ProfilerFrame.parse(stats, index: _next++);
    if (frame == null) return;
    frames.add(frame);
    if (frames.length > capacity) {
      frames.removeAt(0);
      if (selected != null) selected = selected! > 0 ? selected! - 1 : null;
    }
    notifyListeners();
  }

  /// The window the details show.
  ProfilerFrame? get shown {
    if (frames.isEmpty) return null;
    final i = selected;
    return i == null || i >= frames.length ? frames.last : frames[i];
  }

  bool get live => session.isActive;

  void setRecording(bool on) {
    if (recording == on) return;
    recording = on;
    notifyListeners();
  }

  void setClearOnPlay(bool on) {
    if (clearOnPlay == on) return;
    clearOnPlay = on;
    notifyListeners();
  }

  void setChart(ProfilerChart c) {
    if (chart == c) return;
    chart = c;
    notifyListeners();
  }

  void setPercentile(String p) {
    if (percentile == p) return;
    percentile = p;
    notifyListeners();
  }

  /// Selects window [i] (clamped); null follows the newest.
  void select(int? i) {
    final next = i == null || frames.isEmpty ? null : i.clamp(0, frames.length - 1);
    if (next == selected) return;
    selected = next;
    notifyListeners();
  }

  /// Moves the selection by [delta] windows (from the newest when following).
  void step(int delta) {
    if (frames.isEmpty) return;
    select((selected ?? frames.length - 1) + delta);
  }

  void clear() {
    frames.clear();
    selected = null;
    notifyListeners();
  }

  /// The chart's series that any kept window has, in order.
  List<ProfilerSeries> get visibleSeries => [
    for (final s in chart.series)
      if (frames.any((f) => f.metric(s.key) != null)) s,
  ];

  @override
  void dispose() {
    session.removeListener(_onSession);
    super.dispose();
  }
}
