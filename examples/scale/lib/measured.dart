// The scale world for package:fynkel_measure's tool, which runs this in
// place of lib/main.dart. Its bench times the world's update by itself;
// with `--define FRAME_TIMES=true` it prints, once a second, how long each
// `world.update` took and how long from its end to the next frame's start
// (where the view's frame, the extract, runs).

import 'package:flutter/widgets.dart';
import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_measure/fynkel_measure.dart';

import 'main.dart';

const _frameTimes = bool.fromEnvironment('FRAME_TIMES');

void main() => runApp(
  MeasuredApp(() {
    final root = Scale();
    if (_frameTimes) root.addComponent(_StartTimes());
    return root;
  }, timings: (root) => [_update(root.world)]),
);

class _StartTimes extends Behaviour {
  @override
  void onStart() {
    super.onStart();
    world.system(_Times.new);
  }
}

class _Times extends WorldSystem {
  _Times(super.world);

  final _clock = Stopwatch()..start();
  var _start = 0, _end = 0, _mark = 0;
  final _update = <int>[], _between = <int>[], _first = <int>[], _updates = <int>[], _last = <int>[];

  int _since() {
    final now = _clock.elapsedMicroseconds, since = now - _mark;
    _mark = now;
    return since;
  }

  @override
  void beforeUpdate() {
    super.beforeUpdate();
    _first.add(_since());
  }

  @override
  void afterLateUpdate() {
    super.afterLateUpdate();
    _updates.add(_since());
  }

  @override
  void beforeFrame() {
    super.beforeFrame();
    _start = _mark = _clock.elapsedMicroseconds;
    if (_end > 0) _between.add(_start - _end);
  }

  @override
  void afterFrame() {
    super.afterFrame();
    _end = _clock.elapsedMicroseconds;
    _update.add(_end - _start);
    _last.add(_since());
    if (_update.length == 60) {
      String median(List<int> t) => ((t..sort())[t.length ~/ 2] / 1000).toStringAsFixed(2);
      // ignore: avoid_print
      print(
        '[times] update ${median(_update)} ms (to the updates ${median(_first)}, updates ${median(_updates)}, '
        'after ${median(_last)}), between ${median(_between)} ms',
      );
      for (final list in [_update, _between, _first, _updates, _last]) {
        list.clear();
      }
    }
  }
}

String _update(World world) {
  final clock = Stopwatch();
  final times = <int>[];
  for (var i = 0; i < 240; i++) {
    clock
      ..reset()
      ..start();
    world.update(1 / 60);
    times.add(clock.elapsedMicroseconds);
  }
  times.sort();
  return 'scale world.update: median ${times[120]} µs, 95th ${times[228]} µs, max ${times.last} µs';
}
