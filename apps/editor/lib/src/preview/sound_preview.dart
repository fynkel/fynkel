// The Inspector's sound preview (ADR-0030 step 6), as Unity's audio clip
// preview: the waveform, one lane per channel, with a playhead; play and
// loop in the toolbar; a click on the waveform plays from there; the line
// under it says the format — `Vorbis, 44100 Hz, Stereo, 0:03.210, 212 KB`.
// The waveform is peaks the native stage computes from the cooked clip
// (no samples cross into Dart); playback goes through the stage's own
// audio engine on the output device.
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'preview_provider.dart';
import 'stage_backend.dart';
import 'stage_view.dart';

/// Columns the waveform is read in, once: enough for a wide Inspector;
/// narrower pictures fold several into a pixel.
const waveformColumns = 2048;

/// `0:03.210`.
String soundTime(double seconds) {
  final ms = (seconds * 1000).round();
  final m = ms ~/ 60000, s = (ms ~/ 1000) % 60, rest = ms % 1000;
  return '$m:${'$s'.padLeft(2, '0')}.${'$rest'.padLeft(3, '0')}';
}

/// `212 KB`.
String _size(int bytes) => bytes < 1024
    ? '$bytes B'
    : bytes < 1024 * 1024
    ? '${(bytes / 1024).round()} KB'
    : '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';

/// The info line: `Vorbis, 44100 Hz, Stereo, 0:03.210, 212 KB`.
String soundInfo(PreviewSoundInfo i, {int? bytes}) => [
  i.codec == 'vorbis' ? 'Vorbis' : 'PCM',
  '${i.sampleRate} Hz',
  i.channels == 1 ? 'Mono' : 'Stereo',
  soundTime(i.duration),
  ?(bytes == null ? null : _size(bytes)),
  if (i.streamed) 'Streamed',
].join(', ');

final soundPreview = PreviewProvider(
  package: PreviewProvider.editorPackage,
  kind: 'audio',
  name: 'sound',
  build: (context, preview) => preview.sound(
    playing: preview.value('playing', false),
    loop: preview.value('loop', false),
    onPlaying: (p) => preview.set('playing', p),
    onInfo: (i) => preview.set('info', i),
  ),
  toolbar: (context, preview) {
    final t = context.tokens;
    final playing = preview.value('playing', false);
    final loop = preview.value('loop', false);
    Widget button(String key, IconData icon, String tip, bool on, VoidCallback tap) => Tooltip(
      message: tip,
      child: InkWell(
        key: Key(key),
        onTap: tap,
        borderRadius: t.cornerControl,
        child: Container(
          padding: const EdgeInsets.all(3),
          decoration: BoxDecoration(color: on ? t.selectionQuiet : null, borderRadius: t.cornerControl),
          child: Icon(icon, size: 13, color: on ? t.text : t.textSecondary),
        ),
      ),
    );
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        button(
          'preview.play',
          playing ? FIcons.stop : FIcons.playArrow,
          playing ? 'Stop' : 'Play',
          playing,
          () => preview.set('playing', !playing),
        ),
        const SizedBox(width: 2),
        button('preview.loop', FIcons.repeat, 'Loop', loop, () => preview.set('loop', !loop)),
      ],
    );
  },
  info: (preview) {
    final i = preview.value<PreviewSoundInfo?>('info', null);
    if (i == null) return null;
    int? bytes;
    try {
      bytes = File(preview.file).lengthSync();
    } on FileSystemException {
      bytes = null;
    }
    return soundInfo(i, bytes: bytes);
  },
);

class SoundView extends StatefulWidget {
  const SoundView({
    super.key,
    required this.asset,
    this.playing = false,
    this.loop = false,
    this.onPlaying,
    this.onInfo,
  });

  final String asset;
  final bool playing, loop;
  final void Function(bool playing)? onPlaying;
  final void Function(PreviewSoundInfo info)? onInfo;

  @override
  State<SoundView> createState() => SoundViewState();
}

class SoundViewState extends State<SoundView> with SingleTickerProviderStateMixin {
  StageBackend? _stage;
  SoundPeaks? _peaks;
  String? _error;
  late final Ticker _ticker = createTicker(_tick);
  double _time = 0;
  bool _playing = false;
  bool _device = true;
  bool _opened = false;

  /// The stage shown (tests).
  StageBackend? get stage => _stage;
  double get time => _time;
  bool get isPlaying => _playing;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_opened) return;
    _opened = true;
    final scope = PreviewStageScope.maybeOf(context);
    _device = scope?.viewports ?? false;
    if (scope == null) {
      _error = 'No project to read it from.';
      return;
    }
    try {
      final stage = scope.project.openStage(widget.asset);
      _stage = stage;
      _peaks = stage.peaks(waveformColumns);
      final s = stage.state.sound;
      if (s != null && widget.onInfo != null) {
        final info = PreviewSoundInfo(
          sampleRate: s.sampleRate,
          channels: s.channels,
          duration: s.duration,
          codec: s.codec,
          streamed: s.streamed,
        );
        SchedulerBinding.instance.addPostFrameCallback((_) {
          if (mounted) widget.onInfo!(info);
        });
      }
      if (widget.playing) _play(0);
    } on AuthoringException catch (e) {
      _error = e.diagnostics.firstOrNull?.message ?? '$e';
    } on UnsupportedError {
      _error = 'This project plays no sounds here.';
    }
  }

  @override
  void didUpdateWidget(covariant SoundView oldWidget) {
    super.didUpdateWidget(oldWidget);
    final stage = _stage;
    if (stage == null) return;
    if (widget.loop != oldWidget.loop) _apply(() => stage.setLoop(widget.loop));
    if (widget.playing != _playing) {
      if (widget.playing) {
        _play(0);
      } else {
        // Stop, as Unity's: the next Play starts over.
        _apply(stage.stop);
        setState(() => _time = 0);
      }
    }
  }

  void _play(double from) {
    final stage = _stage;
    if (stage == null) return;
    _apply(() => stage.play(from: from, loop: widget.loop, device: _device));
  }

  void _apply(SoundPlayhead Function() call) {
    final SoundPlayhead head;
    try {
      head = call();
    } on AuthoringException catch (e) {
      setState(() => _error = e.diagnostics.firstOrNull?.message ?? '$e');
      return;
    }
    _show(head);
  }

  void _show(SoundPlayhead head) {
    final was = _playing;
    setState(() {
      _playing = head.playing;
      if (head.playing || !was) _time = head.time;
    });
    if (head.playing && !_ticker.isActive) _ticker.start();
    if (!head.playing && _ticker.isActive) _ticker.stop();
    if (was && !head.playing && widget.playing) {
      // Ran out by itself: the toolbar's button goes back to Play.
      _time = 0;
      widget.onPlaying?.call(false);
    }
  }

  void _tick(Duration _) {
    final stage = _stage;
    if (stage == null || stage.isClosed) return;
    try {
      _show(stage.playhead());
    } on AuthoringException {
      _ticker.stop();
    }
  }

  /// A click on the waveform: plays from there.
  void _seek(double fraction) {
    final duration = _stage?.state.sound?.duration ?? 0;
    _play(fraction.clamp(0.0, 1.0) * duration);
    if (!widget.playing) widget.onPlaying?.call(true);
  }

  @override
  void dispose() {
    _ticker.dispose();
    _stage?.close();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final small = Theme.of(context).textTheme.bodySmall;
    if (_error case final error?) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Text(
            error,
            key: const Key('preview.sound.error'),
            textAlign: TextAlign.center,
            style: small?.copyWith(color: t.textSecondary),
          ),
        ),
      );
    }
    final duration = _stage?.state.sound?.duration ?? 0;
    return LayoutBuilder(
      builder: (context, box) => GestureDetector(
        key: const Key('preview.waveform'),
        behavior: HitTestBehavior.opaque,
        onTapDown: (d) => _seek(d.localPosition.dx / math.max(box.maxWidth, 1)),
        child: Stack(
          fit: StackFit.expand,
          children: [
            CustomPaint(
              painter: _Waveform(
                peaks: _peaks,
                progress: duration > 0 ? _time / duration : 0,
                showHead: _playing || _time > 0,
                wave: t.warning,
                played: Color.lerp(t.warning, t.text, 0.35)!,
                head: t.text,
                lane: t.line,
              ),
            ),
            if (_playing || _time > 0)
              Positioned(
                left: 6,
                bottom: 3,
                child: Text(
                  '${soundTime(_time)} / ${soundTime(duration)}',
                  key: const Key('preview.sound.time'),
                  style: small?.copyWith(color: t.textSecondary, fontFeatures: const [FontFeature.tabularFigures()]),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _Waveform extends CustomPainter {
  _Waveform({
    required this.peaks,
    required this.progress,
    required this.showHead,
    required this.wave,
    required this.played,
    required this.head,
    required this.lane,
  });

  final SoundPeaks? peaks;
  final double progress;
  final bool showHead;
  final Color wave, played, head, lane;

  @override
  void paint(Canvas canvas, Size size) {
    final p = peaks;
    if (p == null || p.columns == 0 || size.width <= 0) return;
    final lanes = p.channels;
    final laneHeight = size.height / lanes;
    final pixels = size.width.floor();
    final headX = progress.clamp(0.0, 1.0) * size.width;
    final stroke = Paint()..strokeWidth = 1;
    for (var c = 0; c < lanes; c++) {
      final mid = laneHeight * (c + 0.5);
      final half = laneHeight * 0.45;
      canvas.drawLine(Offset(0, mid), Offset(size.width, mid), Paint()..color = lane);
      for (var x = 0; x < pixels; x++) {
        // The columns under this pixel, folded.
        final from = (x * p.columns / pixels).floor();
        final to = math.max(from + 1, ((x + 1) * p.columns / pixels).floor());
        var lo = 0.0, hi = 0.0;
        for (var k = from; k < to && k < p.columns; k++) {
          lo = math.min(lo, p.min(c, k));
          hi = math.max(hi, p.max(c, k));
        }
        stroke.color = showHead && x < headX ? played : wave;
        final dx = x + 0.5;
        canvas.drawLine(Offset(dx, mid - hi * half), Offset(dx, mid - lo * half + 0.5), stroke);
      }
    }
    if (showHead) {
      canvas.drawLine(Offset(headX, 0), Offset(headX, size.height), Paint()
        ..color = head
        ..strokeWidth = 1);
    }
  }

  @override
  bool shouldRepaint(_Waveform old) =>
      old.peaks != peaks || old.progress != progress || old.showHead != showHead || old.wave != wave;
}
