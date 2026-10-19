// The lab's crowd (stage 9, phase 4) for package:fynkel_measure's tool,
// which runs this in place of lib/main.dart. Its settings are
// `--define`s:
//
// - `CROWD`, how many characters (100);
// - `QUALITY=low|medium|high`, the quality drawn, the device's pick
//   otherwise;
// - `ANIMATED=false`, the crowd holding still, to compare with.
//
// Every 10 seconds it reports the animation's time a frame (the median of
// the last second), and its bench times world.update with the crowd.
//
//   caffeinate -du dart run fynkel_measure:measure rate --seconds 60 --define CROWD=100
//
// The tool's own `--define FYNKEL_FRAME_RATE=120` asks for 120 fps.
//
// `--define TOUR=shaders` measures stage 10's scenes in place of the crowd
// (or `TOUR=<title>;<title>`, the scenes whose titles start so): each at
// each quality level in `TOUR_LEVELS` (low,medium,high), for
// `TOUR_SECONDS` (20) each, the first 5 to settle. It reports how long
// each scene took to load and its shaders to be ready after (the cost of
// making their pipelines, which a loading screen hides), and the fresh
// frames a second at each level. Give the tool enough `--seconds` for the
// whole tour: about 5 × 3 × 20 s, and the loads.
//
//   caffeinate -du dart run fynkel_measure:measure rate --seconds 330 --define TOUR=shaders

import 'dart:async';

import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_measure/fynkel_measure.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart' show FynkelViewStats;
import 'package:lab/lab.dart';
import 'package:lab/scenes.dart';
import 'package:lab/scenes/crowd.dart';

const _crowd = int.fromEnvironment('CROWD', defaultValue: 100);
const _quality = String.fromEnvironment('QUALITY');
const _animated = bool.fromEnvironment('ANIMATED', defaultValue: true);

const _tour = String.fromEnvironment('TOUR');
const _tourLevels = String.fromEnvironment('TOUR_LEVELS', defaultValue: 'low,medium,high');
const _tourSeconds = int.fromEnvironment('TOUR_SECONDS', defaultValue: 20);

/// Stage 10's scenes: surfaces, movement, what's behind, the toon look and
/// screen effects.
const _shaderScenes = ['Look: shaders', 'Look: movement', "Look: what's behind", 'Look: toon', 'Look: screen effects'];

void main() {
  if (_tour.isNotEmpty) {
    final titles = _tour == 'shaders' ? _shaderScenes : _tour.split(RegExp('[,;]'));
    runApp(
      MeasuredApp(
        () => Tour(titles, _tourLevels.split(','), seconds: _tourSeconds),
        onStats: (root, stats) => (root as Tour).onStats(stats),
      ),
    );
    return;
  }
  _measureCrowd();
}

void _measureCrowd() => runApp(
  MeasuredApp(() {
    final scene = CrowdScene(count: _crowd, animated: _animated);
    scene.loaded.then((_) {
      scene.world.quality = Quality.levels.where((q) => q.name == _quality).firstOrNull;
      Timer.periodic(const Duration(seconds: 10), (timer) {
        if (scene.isDestroyed) return timer.cancel();
        Measurement.report(
          '${scene.people.length} characters, animation ${scene.animationMs.toStringAsFixed(2)} ms a frame',
        );
      });
    });
    return scene;
  }, timings: (root) => [_update(root.world)]),
);

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
  return 'crowd world.update: median ${times[120]} µs, 95th ${times[228]} µs, max ${times.last} µs';
}

/// Shows each of the lab's scenes whose titles start with [titles], one
/// after the other, at each quality level in [levels], and reports what
/// each cost.
class Tour extends GameObject {
  Tour(this.titles, this.levels, {required this.seconds}) : super(name: 'tour');

  final List<String> titles, levels;
  final int seconds;
  LabScene? _scene;
  var _counting = false, _shown = 0, _window = 0.0;

  void onStats(FynkelViewStats stats) {
    if (!_counting) return;
    _shown += stats.view.presented > 0 ? stats.view.presented : stats.view.rendered;
    _window += stats.window.inMicroseconds / 1e6;
  }

  @override
  void onStart() {
    super.onStart();
    // Hidden (covered, on macOS), the view draws nothing: say so.
    AppLifecycleListener(onHide: () => Measurement.report('hidden: the numbers until it shows again are short'));
    _run();
  }

  Future<void> _run() async {
    for (final title in titles) {
      final make = scenes.firstWhere((make) => make().title.startsWith(title));
      for (final (i, level) in levels.indexed) {
        world.quality = Quality.levels.firstWhere((q) => q.name == level);
        final clock = Stopwatch()..start();
        if (i == 0) {
          _scene?.destroy();
          final scene = _scene = add(make());
          await scene.loaded;
          final loaded = clock.elapsedMilliseconds;
          await world.shadersReady();
          Measurement.report(
            '$title: loaded in $loaded ms, its shaders ready ${clock.elapsedMilliseconds - loaded} ms after (at $level)',
          );
        } else {
          await world.shadersReady();
          Measurement.report('$title at $level: shaders ready in ${clock.elapsedMilliseconds} ms');
        }
        if (isDestroyed) return;
        await Future<void>.delayed(const Duration(seconds: 5));
        _shown = 0;
        _window = 0;
        _counting = true;
        await Future<void>.delayed(Duration(seconds: seconds - 5));
        _counting = false;
        final fps = _window > 0 ? _shown / _window : 0;
        Measurement.report('$title at $level: ${fps.toStringAsFixed(1)} fps over ${_window.toStringAsFixed(0)} s');
      }
    }
    Measurement.report('tour done');
  }
}
