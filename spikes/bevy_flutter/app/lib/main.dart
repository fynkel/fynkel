import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';

import 'bevy.dart';

/// `--dart-define=SPIKE_AUTO=true` runs the README's measurements on its
/// own (steady state, resizes, 10 recreates), prints `[spike]` lines and quits.
const auto = bool.fromEnvironment('SPIKE_AUTO');
const cubes = int.fromEnvironment('SPIKE_CUBES', defaultValue: 1000);

/// Fraction of the window the viewport fills (iOS: 0.75 = the F8 setting).
final scale = double.parse(const String.fromEnvironment('SPIKE_SCALE', defaultValue: '1.0'));

void main() => runApp(const MaterialApp(debugShowCheckedModeBanner: false, home: SpikePage()));

class SpikePage extends StatefulWidget {
  const SpikePage({super.key});

  @override
  State<SpikePage> createState() => _SpikePageState();
}

class _SpikePageState extends State<SpikePage> with SingleTickerProviderStateMixin {
  BevyViewport? _vp;
  late final Ticker _ticker = createTicker(_onTick);
  final _clock = Stopwatch()..start();
  bool _busy = false;
  (int, int)? _want;
  double _fraction = 1.0;

  final List<int> _dartUs = [];
  final List<double> _resizeMs = [];
  final List<double> _freshFps = [];
  bool _collect = false;
  Timer? _secondTimer;
  final _secondWatch = Stopwatch()..start();
  String _hud = 'starting…';
  int _newFramesThisSecond = 0;
  int _ticks = 0;

  @override
  void initState() {
    super.initState();
    _ticker.start();
    _secondTimer = Timer.periodic(const Duration(seconds: 1), (_) => _onSecond());
    if (auto) unawaited(_autoRun());
  }

  @override
  void dispose() {
    _secondTimer?.cancel();
    _ticker.dispose();
    _vp?.dispose();
    super.dispose();
  }

  Future<void> _create() async {
    final want = _want;
    if (want == null) return;
    _busy = true;
    final sw = Stopwatch()..start();
    final vp = await BevyViewport.create(want.$1, want.$2, cubes);
    log('created ${want.$1}x${want.$2} in ${sw.elapsedMilliseconds} ms (texture ${vp.textureId})');
    setState(() => _vp = vp);
    _busy = false;
  }

  Future<void> _destroy() async {
    final vp = _vp;
    if (vp == null) return;
    _busy = true;
    setState(() => _vp = null);
    await vp.dispose();
    _busy = false;
  }

  void _onTick(Duration _) {
    _ticks++;
    final vp = _vp;
    if (vp == null || _busy) {
      if (vp == null && !_busy && _want != null && !auto) unawaited(_create());
      return;
    }
    final want = _want;
    if (want != null && (want.$1 != vp.width || want.$2 != vp.height)) {
      _busy = true;
      vp.resize(want.$1, want.$2).then((ms) {
        _resizeMs.add(ms);
        log('resize -> ${want.$1}x${want.$2}: ${ms.toStringAsFixed(2)} ms');
        _busy = false;
      });
      return;
    }
    final sw = Stopwatch()..start();
    vp.frame(_clock.elapsedMicroseconds / 1e6);
    _dartUs.add(sw.elapsedMicroseconds);
  }

  Future<void> _onSecond() async {
    final vp = _vp;
    final secs = _secondWatch.elapsedMicroseconds / 1e6;
    _secondWatch.reset();
    if (vp == null) return;
    final n = vp.nativeStats();
    final t = await vp.textureStats();
    final fresh = (t['copyWithNewFrame'] ?? 0) / secs;
    _newFramesThisSecond = t['copyWithNewFrame'] ?? 0;
    final dart = [..._dartUs]..sort();
    _dartUs.clear();
    _ticks = 0;
    if (_collect) _freshFps.add(fresh);
    final line = 'fresh ${fresh.toStringAsFixed(1)} fps | bevy frames ${n[0].toInt()} coalesced ${n[2].toInt()} '
        'presented ${n[3].toInt()} dropped ${n[1].toInt()} | ticks $_ticks busy $_busy | update p50 ${n[4].toStringAsFixed(2)} p95 ${n[5].toStringAsFixed(2)} ms | '
        'gpu p50 ${n[6].toStringAsFixed(2)} p95 ${n[7].toStringAsFixed(2)} ms | '
        'dart p50 ${_pct(dart, .5)} p95 ${_pct(dart, .95)} ms | ${vp.width}x${vp.height} | '
        'rss ${(ProcessInfo.currentRss / 1048576).round()} MB | thermal ${t['thermalState']}';
    log(line);
    if (mounted) setState(() => _hud = line.replaceAll(' | ', '\n'));
  }

  String _pct(List<int> sorted, double q) =>
      sorted.isEmpty ? '-' : (sorted[((sorted.length - 1) * q).round()] / 1000).toStringAsFixed(3);

  Future<void> _waitFrames({Duration timeout = const Duration(seconds: 10)}) async {
    _newFramesThisSecond = 0;
    final sw = Stopwatch()..start();
    while (_newFramesThisSecond == 0) {
      if (sw.elapsed > timeout) {
        log('STALL: pid $pid ticker active ${_ticker.isActive} muted ${_ticker.muted} busy $_busy vp ${_vp != null}; waiting 30 s for a sample');
        await Future<void>.delayed(const Duration(seconds: 30));
        throw StateError('no frame within $timeout');
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
  }

  Future<void> _autoRun() async {
    try {
      while (_want == null) {
        await Future<void>.delayed(const Duration(milliseconds: 50));
      }
      final firstFrame = Stopwatch()..start();
      await _create();
      await _waitFrames();
      log('first frame within ${firstFrame.elapsedMilliseconds} ms of create (1 s stats granularity)');

      await Future<void>.delayed(const Duration(seconds: 5));
      _collect = true;
      await Future<void>.delayed(const Duration(seconds: 10, milliseconds: 200));
      _collect = false;
      final f = [..._freshFps]..sort();
      log('STEADY ${_vp!.width}x${_vp!.height}: fresh fps min ${f.first.toStringAsFixed(1)} '
          'median ${f[f.length ~/ 2].toStringAsFixed(1)} max ${f.last.toStringAsFixed(1)} over ${f.length} s');

      for (final frac in [0.6, 1.0, 0.75, 0.5, 0.9, 1.0]) {
        setState(() => _fraction = frac);
        await Future<void>.delayed(const Duration(milliseconds: 1500));
        await _waitFrames(timeout: const Duration(seconds: 3));
      }
      final r = [..._resizeMs]..sort();
      log('RESIZE ${r.length}x: max ${r.last.toStringAsFixed(2)} ms, frames resumed after every one');

      final rss = <int>[];
      final createMs = <int>[];
      for (var i = 0; i < 10; i++) {
        await _destroy();
        final sw = Stopwatch()..start();
        await _create();
        createMs.add(sw.elapsedMilliseconds);
        await _waitFrames();
        await Future<void>.delayed(const Duration(seconds: 1));
        rss.add((ProcessInfo.currentRss / 1048576).round());
      }
      log('RECREATE 10x: create ms $createMs; rss MB $rss');

      _freshFps.clear();
      _collect = true;
      await Future<void>.delayed(const Duration(seconds: 5, milliseconds: 200));
      _collect = false;
      final g = [..._freshFps]..sort();
      log('AFTER fresh fps median ${g[g.length ~/ 2].toStringAsFixed(1)}');
      log('DONE');
    } catch (e, st) {
      log('FAILED $e\n$st');
    }
    await _destroy();
    exit(0);
  }

  @override
  Widget build(BuildContext context) {
    final dpr = MediaQuery.devicePixelRatioOf(context);
    return Scaffold(
      backgroundColor: Colors.black,
      body: LayoutBuilder(builder: (context, c) {
        final w = c.maxWidth * _fraction, h = c.maxHeight * _fraction;
        _want = ((w * dpr * scale).round(), (h * dpr * scale).round());
        final vp = _vp;
        return Stack(children: [
          Center(
            child: SizedBox(
              width: w,
              height: h,
              child: vp == null
                  ? const Center(child: CircularProgressIndicator())
                  : Texture(textureId: vp.textureId, filterQuality: FilterQuality.low),
            ),
          ),
          Positioned(
            left: 12,
            top: 12,
            child: Container(
              padding: const EdgeInsets.all(8),
              color: Colors.black54,
              child: Text(_hud, style: const TextStyle(color: Colors.white, fontSize: 12, fontFamily: 'Menlo')),
            ),
          ),
          if (!auto)
            Positioned(
              right: 12,
              top: 12,
              child: Row(children: [
                FilledButton(
                  onPressed: () async {
                    await _destroy();
                    await _create();
                  },
                  child: const Text('Recreate'),
                ),
              ]),
            ),
        ]);
      }),
    );
  }
}

/// Also appended to `<app tmp>/spike.log`: a release build's `print` is not
/// visible from a wireless iPhone (fetch it with `devicectl device copy from`).
final _logFile = File('${Directory.systemTemp.path}/spike.log')..writeAsStringSync('');

void log(String s) {
  // ignore: avoid_print
  print('[spike] $s');
  _logFile.writeAsStringSync('[spike] $s\n', mode: FileMode.append, flush: true);
}
