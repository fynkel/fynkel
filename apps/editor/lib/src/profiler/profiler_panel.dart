// The Profiler panel (roadmap Phase 2): a chart of the kept stats windows
// — CPU, GPU, Rendering or Memory — with the 60 and 120 fps lines on the
// timing charts, a legend with the shown window's values, and that
// window's full breakdown below. Click or drag on the chart (or ← →) to
// look at one window; End or ⎋ follows the newest again.
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../project_browser/project_browser_model.dart' show formatBytes;
import '../theme/icons.dart';
import '../theme/panel_focus.dart';
import '../theme/tokens.dart';
import 'profiler_model.dart';

/// The series colours, by [ProfilerSeries.colorIndex].
List<Color> profilerPalette(FynkelTokens t) => [t.accent, t.ok, t.warning, t.info, t.axisX, t.axisZ];

class ProfilerPanel extends StatefulWidget {
  const ProfilerPanel({super.key, required this.model});

  final ProfilerModel model;

  @override
  State<ProfilerPanel> createState() => _ProfilerPanelState();
}

class _ProfilerPanelState extends State<ProfilerPanel> {
  final _focus = FocusNode(debugLabel: 'profiler.chart');

  ProfilerModel get model => widget.model;

  @override
  void dispose() {
    _focus.dispose();
    super.dispose();
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent e) {
    if (e is! KeyDownEvent && e is! KeyRepeatEvent) return KeyEventResult.ignored;
    final k = e.logicalKey;
    if (k == LogicalKeyboardKey.arrowLeft) {
      model.step(-1);
    } else if (k == LogicalKeyboardKey.arrowRight) {
      model.step(1);
    } else if (k == LogicalKeyboardKey.end || k == LogicalKeyboardKey.escape) {
      model.select(null);
    } else {
      return KeyEventResult.ignored;
    }
    return KeyEventResult.handled;
  }

  @override
  Widget build(BuildContext context) {
    PanelFocus.primary(context, _focus);
    return ListenableBuilder(
      listenable: model,
      builder: (context, _) {
        final t = context.tokens;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            _toolbar(context, t),
            Expanded(
              child: model.frames.isEmpty
                  ? _empty(context, t)
                  : Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        Expanded(
                          flex: 3,
                          child: Row(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              SizedBox(width: 190, child: _legend(context, t)),
                              VerticalDivider(width: 1, color: t.line),
                              Expanded(child: _chart(context, t)),
                            ],
                          ),
                        ),
                        Divider(height: 1, color: t.line),
                        Expanded(flex: 2, child: _details(context, t)),
                      ],
                    ),
            ),
          ],
        );
      },
    );
  }

  Widget _toolbar(BuildContext context, FynkelTokens t) {
    final theme = Theme.of(context);
    final timing = model.chart == ProfilerChart.cpu || model.chart == ProfilerChart.gpu;
    final small = theme.textTheme.bodySmall?.copyWith(color: t.textSecondary);
    return Container(
      height: t.toolbar,
      padding: const EdgeInsets.symmetric(horizontal: 3),
      decoration: BoxDecoration(
        color: t.header,
        border: Border(bottom: t.hairline),
      ),
      // A narrow panel scrolls its controls rather than overflowing them.
      child: Row(
        children: [
          Expanded(
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              child: Row(
                children: [
                  IconButton(
                    key: const Key('profiler.record'),
                    tooltip: model.recording
                        ? 'Recording: click to stop keeping windows'
                        : 'Not recording: click to record',
                    isSelected: model.recording,
                    icon: Icon(FIcons.circle, size: 12, color: model.recording ? t.error : t.textDisabled),
                    onPressed: () => model.setRecording(!model.recording),
                  ),
                  IconButton(
                    key: const Key('profiler.clear'),
                    tooltip: 'Clear',
                    icon: const Icon(FIcons.clearAll),
                    onPressed: model.frames.isEmpty ? null : model.clear,
                  ),
                  InkWell(
                    key: const Key('profiler.clearOnPlay'),
                    onTap: () => model.setClearOnPlay(!model.clearOnPlay),
                    child: Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 4),
                      child: Row(
                        children: [
                          Icon(
                            model.clearOnPlay ? FIcons.checkCircle : FIcons.circleOutlined,
                            size: 13,
                            color: model.clearOnPlay ? t.accent : t.textDisabled,
                          ),
                          const SizedBox(width: 4),
                          Text('Clear on Play', style: small),
                        ],
                      ),
                    ),
                  ),
                  const SizedBox(width: 8),
                  SegmentedButton<ProfilerChart>(
                    key: const Key('profiler.chart'),
                    showSelectedIcon: false,
                    style: const ButtonStyle(
                      visualDensity: VisualDensity.compact,
                      padding: WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 8)),
                    ),
                    segments: [
                      for (final c in ProfilerChart.values)
                        ButtonSegment(
                          value: c,
                          label: Text(c.label, key: Key('profiler.chart.${c.name}')),
                        ),
                    ],
                    selected: {model.chart},
                    onSelectionChanged: (s) => model.setChart(s.first),
                  ),
                  if (timing) ...[
                    const SizedBox(width: 6),
                    PopupMenuButton<String>(
                      key: const Key('profiler.percentile'),
                      popUpAnimationStyle: AnimationStyle.noAnimation,
                      tooltip: 'Which percentile of each window the lines show',
                      itemBuilder: (context) => [
                        for (final p in const ['p50', 'p95', 'p99'])
                          CheckedPopupMenuItem(value: p, checked: model.percentile == p, child: Text(p)),
                      ],
                      onSelected: model.setPercentile,
                      child: Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 6),
                        child: Row(
                          children: [
                            Text(model.percentile, style: small),
                            Icon(FIcons.arrowDropDown, size: 14, color: t.textSecondary),
                          ],
                        ),
                      ),
                    ),
                  ],
                ],
              ),
            ),
          ),
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 260),
            child: Padding(
              padding: EdgeInsets.symmetric(horizontal: t.pad),
              child: Text(
                [
                  if (model.live) 'Playing' else if (model.frames.isNotEmpty) 'Stopped',
                  if (model.frames.isNotEmpty) '${model.frames.length} s kept',
                  if (model.selected != null) '← → step, End follows',
                ].join(' · '),
                key: const Key('profiler.status'),
                style: small,
                overflow: TextOverflow.ellipsis,
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _empty(BuildContext context, FynkelTokens t) {
    final theme = Theme.of(context);
    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(FIcons.speed, size: 40, color: theme.disabledColor),
            const SizedBox(height: 12),
            Text(
              model.live
                  ? (model.recording ? 'Waiting for the first stats window…' : 'Not recording.')
                  : 'Press Play to profile the game.',
              key: const Key('profiler.empty'),
              style: theme.textTheme.titleSmall,
            ),
            const SizedBox(height: 4),
            Text(
              'The game reports its frame timings, draw calls and memory once a second,\n'
              'in the Game view and in Build and Run alike.',
              textAlign: TextAlign.center,
              style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
            ),
          ],
        ),
      ),
    );
  }

  String _format(double v, {String? key}) {
    if (model.chart == ProfilerChart.memory || (key != null && _bytes.contains(key))) return formatBytes(v.round());
    if (model.chart == ProfilerChart.cpu || model.chart == ProfilerChart.gpu) return '${v.toStringAsFixed(2)} ms';
    return v.round().toString();
  }

  Widget _legend(BuildContext context, FynkelTokens t) {
    final theme = Theme.of(context);
    final palette = profilerPalette(t);
    final shown = model.shown;
    return ListView(
      key: const Key('profiler.legend'),
      padding: const EdgeInsets.symmetric(vertical: 4),
      children: [
        for (final s in model.visibleSeries)
          Padding(
            padding: EdgeInsets.symmetric(horizontal: t.pad, vertical: 2),
            child: Row(
              children: [
                Container(
                  width: 8,
                  height: 8,
                  decoration: BoxDecoration(color: palette[s.colorIndex % palette.length], shape: BoxShape.circle),
                ),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(s.label, style: theme.textTheme.bodySmall, overflow: TextOverflow.ellipsis),
                ),
                Text(
                  switch (shown?.metric(s.key, pct: model.percentile)) {
                    final v? => _format(v),
                    null => '—',
                  },
                  key: Key('profiler.legend.${s.key}'),
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: t.textSecondary,
                    fontFeatures: const [FontFeature.tabularFigures()],
                  ),
                ),
              ],
            ),
          ),
      ],
    );
  }

  Widget _chart(BuildContext context, FynkelTokens t) {
    return Focus(
      focusNode: _focus,
      onKeyEvent: _onKey,
      child: LayoutBuilder(
        builder: (context, c) {
          final painter = ProfilerChartPainter(
            frames: model.frames,
            series: model.visibleSeries,
            chart: model.chart,
            percentile: model.percentile,
            selected: model.selected,
            palette: profilerPalette(t),
            grid: t.line,
            text: t.textDisabled,
            cursor: t.textSecondary,
          );
          void pick(Offset at) {
            _focus.requestFocus();
            model.select(painter.indexAt(at.dx, c.maxWidth));
          }

          return GestureDetector(
            key: const Key('profiler.plot'),
            behavior: HitTestBehavior.opaque,
            onTapDown: (d) => pick(d.localPosition),
            onHorizontalDragUpdate: (d) => pick(d.localPosition),
            child: CustomPaint(painter: painter, size: Size(c.maxWidth, c.maxHeight)),
          );
        },
      ),
    );
  }

  Widget _details(BuildContext context, FynkelTokens t) {
    final theme = Theme.of(context);
    final f = model.shown!;
    final label = theme.textTheme.bodySmall?.copyWith(color: t.textSecondary);
    final value = theme.textTheme.bodySmall?.copyWith(fontFeatures: const [FontFeature.tabularFigures()]);
    String number(String key) {
      final v = f.values[key]!;
      if (_bytes.contains(key)) return formatBytes(v.round());
      return v == v.roundToDouble() ? v.round().toString() : v.toStringAsFixed(2);
    }

    String timer(Percentiles p) =>
        '${p.p50.toStringAsFixed(2)} / ${p.p95.toStringAsFixed(2)} / ${p.p99.toStringAsFixed(2)} ms';
    final sections = <(String, List<(String, String, String)>)>[];
    for (final (title, keys) in _groups) {
      final rows = <(String, String, String)>[
        for (final k in keys)
          if (f.timers[k] != null)
            (k, _labels[k] ?? k, timer(f.timers[k]!))
          else if (f.values[k] != null)
            (k, _labels[k] ?? k, number(k)),
      ];
      if (rows.isNotEmpty) sections.add((title, rows));
    }
    return ListView(
      key: const Key('profiler.details'),
      padding: EdgeInsets.all(t.pad),
      children: [
        Text(
          [
            'Window ${f.index + 1}',
            if (f.tick != null) 'tick ${f.tick}',
            if (model.selected == null) 'newest',
            'timers p50 / p95 / p99',
          ].join(' · '),
          key: const Key('profiler.window'),
          style: label,
        ),
        const SizedBox(height: 6),
        Wrap(
          spacing: 24,
          runSpacing: 10,
          children: [
            for (final (title, rows) in sections)
              SizedBox(
                width: 300,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(title.toUpperCase(), style: label?.copyWith(fontWeight: FontWeight.w600)),
                    for (final (key, name, text) in rows)
                      Row(
                        children: [
                          Expanded(
                            child: Text(name, style: label, overflow: TextOverflow.ellipsis),
                          ),
                          const SizedBox(width: 8),
                          Flexible(
                            child: Text(
                              text,
                              key: Key('profiler.value.$key'),
                              style: value,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                        ],
                      ),
                  ],
                ),
              ),
          ],
        ),
      ],
    );
  }
}

/// Keys counted in bytes.
const _bytes = {
  'mem_resident',
  'mem_native',
  'mem_assets',
  'mem_gpu',
  'snapshot',
  'snapshot_peak',
  'heap',
  'heap_capacity',
  'heap_external',
  'heap_alloc',
};

/// The details' sections and the keys each shows, in order.
const _groups = <(String, List<String>)>[
  ('Frame', ['fps', 'frames', 'ticks', 'missed', 'dropped', 'interval']),
  (
    'CPU',
    ['work', 'simulate', 'behaviours', 'native', 'render', 'extract', 'prepare', 'encode', 'cpu', 'build', 'raster'],
  ),
  ('GPU', ['gpu_frame', 'gpu_shadow', 'gpu_ao', 'gpu_scene', 'gpu_post', 'gpu_complete', 'gpu_n', 'gpu_untimed']),
  ('Rendering', ['entities', 'draws', 'instances', 'culled', 'shadow.0', 'shadow.1', 'rendered']),
  (
    'Memory',
    [
      'mem_resident',
      'mem_native',
      'mem_assets',
      'mem_gpu',
      'snapshot',
      'snapshot_peak',
      'heap',
      'heap_capacity',
      'heap_external',
      'heap_alloc',
      'gc',
      'gc_pause',
    ],
  ),
];

const _labels = {
  'fps': 'Frames per second',
  'frames': 'Frames',
  'ticks': 'Fixed ticks',
  'missed': 'Missed deadlines',
  'dropped': 'Dropped frames',
  'interval': 'Frame interval',
  'work': 'Frame work',
  'simulate': 'Simulate',
  'behaviours': 'Behaviours (Dart)',
  'native': 'Engine step',
  'render': 'Render call',
  'extract': 'Extract',
  'prepare': 'Prepare',
  'encode': 'Encode',
  'cpu': 'Renderer CPU',
  'build': 'Flutter build',
  'raster': 'Flutter raster',
  'gpu_frame': 'GPU frame',
  'gpu_shadow': 'Shadows',
  'gpu_ao': 'Ambient occlusion',
  'gpu_scene': 'Scene',
  'gpu_post': 'Post',
  'gpu_complete': 'GPU complete',
  'gpu_n': 'Timed frames',
  'gpu_untimed': 'Untimed frames',
  'entities': 'Entities',
  'draws': 'Draw calls',
  'instances': 'Instances',
  'culled': 'Culled',
  'shadow.0': 'Shadow casters',
  'shadow.1': 'Shadow draws',
  'rendered': 'Frames rendered',
  'mem_resident': 'Resident',
  'mem_native': 'Engine (native)',
  'mem_assets': 'Assets',
  'mem_gpu': 'GPU',
  'snapshot': 'Snapshot',
  'snapshot_peak': 'Snapshot peak',
  'heap': 'Dart heap',
  'heap_capacity': 'Heap capacity',
  'heap_external': 'Heap external',
  'heap_alloc': 'Allocated',
  'gc': 'Collections',
  'gc_pause': 'GC pause (ms)',
};

/// The chart: one line per series over the kept windows, oldest at the
/// left; the timing charts carry the 60 and 120 fps lines.
class ProfilerChartPainter extends CustomPainter {
  ProfilerChartPainter({
    required this.frames,
    required this.series,
    required this.chart,
    required this.percentile,
    required this.selected,
    required this.palette,
    required this.grid,
    required this.text,
    required this.cursor,
  });

  final List<ProfilerFrame> frames;
  final List<ProfilerSeries> series;
  final ProfilerChart chart;
  final String percentile;
  final int? selected;
  final List<Color> palette;
  final Color grid, text, cursor;

  /// At least this many slots across, so a short run grows from the left.
  static const minSlots = 60;

  static const _pad = EdgeInsets.fromLTRB(8, 10, 44, 8);

  double _step(double width) => (width - _pad.horizontal) / math.max(frames.length - 1, minSlots - 1);

  /// The window under x (in a chart [width] wide).
  int indexAt(double x, double width) {
    final i = ((x - _pad.left) / _step(width)).round();
    return i.clamp(0, math.max(frames.length - 1, 0));
  }

  double get _top {
    var top = 0.0;
    for (final f in frames) {
      for (final s in series) {
        final v = f.metric(s.key, pct: percentile);
        if (v != null) top = math.max(top, chart.scale(v));
      }
    }
    final timing = chart == ProfilerChart.cpu || chart == ProfilerChart.gpu;
    if (timing) top = math.max(top, 1000 / 60 * 1.15);
    return top <= 0 ? 1 : top * 1.08;
  }

  @override
  void paint(Canvas canvas, Size size) {
    final plot = _pad.deflateRect(Offset.zero & size);
    if (plot.width <= 0 || plot.height <= 0) return;
    final top = _top;
    final step = _step(size.width);
    double y(double v) => plot.bottom - v / top * plot.height;
    final gridPaint = Paint()
      ..color = grid
      ..strokeWidth = 1;
    void label(String s, double at) {
      final tp = TextPainter(
        text: TextSpan(
          text: s,
          style: TextStyle(color: text, fontSize: 10),
        ),
        textDirection: TextDirection.ltr,
      )..layout();
      tp.paint(canvas, Offset(plot.right + 4, at - tp.height / 2));
    }

    canvas.drawLine(plot.bottomLeft, plot.bottomRight, gridPaint);
    if (chart == ProfilerChart.cpu || chart == ProfilerChart.gpu) {
      for (final (ms, name) in const [(1000 / 60, '60 fps'), (1000 / 120, '120 fps')]) {
        final at = y(ms);
        for (var x = plot.left; x < plot.right; x += 6) {
          canvas.drawLine(Offset(x, at), Offset(math.min(x + 3, plot.right), at), gridPaint);
        }
        label(name, at);
      }
    } else {
      label(chart == ProfilerChart.memory ? '${top.toStringAsFixed(0)} MB' : top.toStringAsFixed(0), plot.top);
    }
    for (final s in series) {
      final path = Path();
      var started = false;
      for (var i = 0; i < frames.length; i++) {
        final v = frames[i].metric(s.key, pct: percentile);
        if (v == null) {
          started = false;
          continue;
        }
        final p = Offset(plot.left + i * step, y(chart.scale(v)));
        if (started) {
          path.lineTo(p.dx, p.dy);
        } else {
          path.moveTo(p.dx, p.dy);
          started = true;
        }
      }
      canvas.drawPath(
        path,
        Paint()
          ..color = palette[s.colorIndex % palette.length]
          ..style = PaintingStyle.stroke
          ..strokeWidth = s.colorIndex == 0 ? 2 : 1.25
          ..isAntiAlias = true,
      );
    }
    final at = selected ?? frames.length - 1;
    if (frames.isNotEmpty) {
      final x = plot.left + at * step;
      canvas.drawLine(
        Offset(x, plot.top),
        Offset(x, plot.bottom),
        Paint()
          ..color = selected == null ? cursor.withValues(alpha: .35) : cursor
          ..strokeWidth = 1,
      );
    }
  }

  @override
  bool shouldRepaint(ProfilerChartPainter old) => true;
}
