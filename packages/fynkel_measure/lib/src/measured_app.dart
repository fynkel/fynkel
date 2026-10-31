import 'dart:async';

import 'package:flutter/material.dart';
import 'package:fynkel/fynkel.dart' show AssetSource, GameObject;
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'bench.dart';
import 'measurement.dart';
import 'on_screen.dart';

/// A game, run as bin/measure.dart asks: the game's `tool/measured.dart`
/// is `void main() => runApp(MeasuredApp(Garden.new));`, and the tool runs
/// that in place of `lib/main.dart`, so the game's own code knows nothing
/// of measuring.
///
/// It's shaped as [FynkelApp]: it shows the root [make] makes in a
/// [FynkelView], in a [FynkelGame] (so a HUD's `FynkelGame.of(context)`
/// works as in the game), with the game's [overlay] over it, laid out as
/// `FynkelApp`'s, and the stats line over that. It reports how long the
/// root took to load and to reach the screen. Its lifecycle check,
/// [MeasureSettings.cycles] times each, with a mark after each step:
///
/// 0. if the game has a [change], makes it (`changed`), as the next
///    level: part of the scene freed and another loaded;
/// 1. destroys the root (`destroyed`) and adds a new one (`added`): the
///    whole scene freed and loaded again, in the same world and view;
/// 2. takes the view away and shows a new one, with a new world and root
///    (`created`);
/// 3. resizes the view between 0.6 of the window and all of it
///    (`resized to 0.6`, `resized to full`).
///
/// The bench runs [bench] (this package's timings), then the game's own
/// [timings], if any.
class MeasuredApp<T extends GameObject> extends StatelessWidget {
  /// The game whose root [make] makes, measured as the tool asks, with
  /// [overlay] over it.
  const MeasuredApp(this.make, {super.key, this.source, this.overlay, this.change, this.timings, this.onStats});

  /// Makes a new root, as `Garden.new` does.
  final T Function() make;

  /// Where the game's assets come from: the app's bundle if null.
  final AssetSource? source;

  /// What the game shows over its view, such as a HUD, given its root, as
  /// [FynkelApp.overlay].
  final Widget Function(BuildContext context, T game)? overlay;

  /// A change the game makes again and again, given its root, such as
  /// going to the next level: done when what it adds has loaded.
  final Future<void> Function(T game)? change;

  /// The game's own timings for the bench, given its loaded root: each a
  /// line for [Measurement.report].
  final List<String> Function(T game)? timings;

  /// Told the view's stats each second, given the root, for a game that
  /// works out rates of its own (a tour of scenes, each measured apart).
  final void Function(T game, FynkelViewStats stats)? onStats;

  @override
  Widget build(BuildContext context) => MaterialApp(
    debugShowCheckedModeBanner: false,
    theme: ThemeData.dark(),
    home: Scaffold(body: _Measured<T>(this)),
  );
}

class _Measured<T extends GameObject> extends StatefulWidget {
  const _Measured(this.app);

  final MeasuredApp<T> app;

  @override
  State<_Measured<T>> createState() => _MeasuredState<T>();
}

class _MeasuredState<T extends GameObject> extends State<_Measured<T>> {
  final _measurement = Measurement();
  final _clock = Stopwatch()..start();

  /// The root being measured: made here, so its load is timed from the
  /// moment it's made, and given to the [FynkelGame] as it starts.
  late T _root = widget.app.make();

  /// [FynkelGame.make]: the root made for it, or a new one when that is
  /// in a world, or was (F5 in a debug build).
  T _make() => _root.isAlive || _root.isDestroyed ? _root = widget.app.make() : _root;

  // Each view is new, with a world and an on-screen clock of its own.
  var _view = GlobalKey<FynkelViewState>();
  var _onScreen = OnScreenClock();
  bool _shown = true;
  double _scale = 1;

  /// The first root on screen (or, for the baseline, the app started).
  final _started = Completer<void>();

  /// The last load's times, under the stats line.
  final _loads = ValueNotifier('loading…');

  @override
  void initState() {
    super.initState();
    if (MeasureSettings.baseline) {
      _started.complete();
    } else {
      unawaited(_timeLoad('load', _clock).then(_started.complete));
    }
    if (MeasureSettings.lifecycleCheck) unawaited(_runLifecycleCheck());
    if (MeasureSettings.benchCheck) unawaited(_runBench());
  }

  @override
  void dispose() {
    _measurement.dispose();
    _loads.dispose();
    super.dispose();
  }

  /// Reports how long [_root] took, by [clock], to load (or [done] to
  /// complete) and to reach the screen.
  Future<void> _timeLoad(String what, Stopwatch clock, [Future<void>? done]) async {
    await (done ?? _root.loaded);
    final loaded = clock.elapsed;
    final shown = await _onScreen.next(clock);
    final line = '$what: loaded ${_ms(loaded)}, on screen ${_ms(shown)}';
    Measurement.report(line);
    _loads.value = line;
  }

  Future<void> _runLifecycleCheck() async {
    const settle = Duration(seconds: 2);
    final cycles = MeasureSettings.cycles;
    final baseline = MeasureSettings.baseline;
    await _started.future;
    // The first frames and uploads are over by then.
    await Future<void>.delayed(const Duration(seconds: 5));
    await Measurement.mark('start');
    if (widget.app.change case final change?) {
      for (var i = 1; i <= cycles; i++) {
        if (!baseline) await _timeLoad('changed', Stopwatch()..start(), change(_root));
        await Future<void>.delayed(settle);
        await Measurement.mark('changed $i');
      }
    }
    for (var i = 1; i <= cycles; i++) {
      if (!baseline) _root.destroy();
      await Future<void>.delayed(settle);
      await Measurement.mark('destroyed $i');
      if (!baseline) {
        final clock = Stopwatch()..start();
        final next = _view.currentState!.world.add(widget.app.make());
        setState(() => _root = next);
        await _timeLoad('added', clock);
      }
      await Future<void>.delayed(settle);
      await Measurement.mark('added $i');
    }
    for (var i = 1; i <= cycles; i++) {
      setState(() => _shown = false);
      await Future<void>.delayed(settle);
      final clock = Stopwatch()..start();
      setState(() {
        _shown = true;
        _view = GlobalKey();
        _onScreen = OnScreenClock();
        if (!baseline) _root = widget.app.make();
      });
      if (!baseline) await _timeLoad('created', clock);
      await Future<void>.delayed(settle);
      await Measurement.mark('created $i');
    }
    for (var i = 1; i <= cycles; i++) {
      setState(() => _scale = i.isOdd ? 0.6 : 1);
      await Future<void>.delayed(settle);
      await Measurement.mark(i.isOdd ? 'resized to 0.6 ${(i + 1) ~/ 2}' : 'resized to full ${i ~/ 2}');
    }
    await Measurement.mark('done');
  }

  Future<void> _runBench() async {
    await _started.future;
    await Future<void>.delayed(const Duration(seconds: 5));
    bench().forEach(Measurement.report);
    if (!MeasureSettings.baseline) widget.app.timings?.call(_root).forEach(Measurement.report);
    await Measurement.mark('done');
  }

  // The view makes its engine view (and, the first time, the GPU device)
  // on the UI isolate in its first tick that has a size: the gap from that tick to
  // the next is how long that held the UI isolate, and with it anything
  // else waiting there, such as a load's last steps.
  Duration? _firstTick;
  var _ticks = 0;

  void _onFrame(double dt) {
    final rate = MeasureSettings.frameRate;
    if (rate != null) _view.currentState?.world.targetFrameRate = rate;
    if (++_ticks > 2) return;
    if (_ticks == 1) _firstTick = _clock.elapsed;
    if (_ticks == 2) {
      Measurement.report(
        'view: first tick ${_ms(_firstTick!)} after start, the next ${_ms(_clock.elapsed - _firstTick!)} later',
      );
    }
  }

  static String _ms(Duration d) => '${(d.inMicroseconds / 1000).toStringAsFixed(1)} ms';

  @override
  Widget build(BuildContext context) {
    final onScreen = _onScreen;
    return Stack(
      children: [
        if (_shown)
          Center(
            child: FractionallySizedBox(
              widthFactor: _scale,
              heightFactor: _scale,
              child: MeasureSettings.baseline
                  ? const BaselineSpinner()
                  : FynkelGame<T>(
                      _make,
                      source: widget.app.source,
                      builder: (context, game) {
                        final view = FynkelView(
                          key: _view,
                          root: _root,
                          onFrame: _onFrame,
                          onStats: (stats) {
                            _measurement.onStats(stats);
                            widget.app.onStats?.call(_root, stats);
                          },
                          onViewStats: onScreen.onViewStats,
                          renderScale: MeasureSettings.renderScale,
                        );
                        final overlay = widget.app.overlay;
                        if (overlay == null) return view;
                        // As FynkelApp lays its overlay out.
                        return Stack(
                          fit: StackFit.expand,
                          children: [
                            view,
                            Material(
                              type: MaterialType.transparency,
                              child: Stack(children: [Builder(builder: (context) => overlay(context, _root))]),
                            ),
                          ],
                        );
                      },
                    ),
            ),
          ),
        StatsLine(_measurement),
        Positioned(
          left: 12,
          right: 12,
          bottom: 12,
          child: SafeArea(
            child: ValueListenableBuilder(
              valueListenable: _loads,
              builder: (context, text, _) => Text(
                text,
                style: const TextStyle(fontFamily: 'Menlo', fontSize: 11, color: Colors.white70),
              ),
            ),
          ),
        ),
      ],
    );
  }
}
