// Step 2: time. Game time against real time, the time scale, and a hitch
// counted as maxDelta.

import 'dart:io' show sleep;
import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class TimeScene extends LabScene {
  @override
  String get title => 'Time';
  @override
  String get step => 'step 2';
  @override
  String get about =>
      'The orange cube turns by time.delta (game time), the blue one by time.unscaledDelta (real time), and the '
      'ball circles by time.sinceStart. Slide the time scale: 0 pauses the game, but the blue cube keeps turning. '
      '"Freeze 1 s" blocks the app for a second: the frame after counts as maxDelta, so nothing jumps.';

  @override
  void build() {
    add(
      shape('game time', Mesh.cube(), const Color(0xFFE0613A), at: Vec3(-2, 0.6, 0), components: [Turn(scaled: true)]),
    );
    add(
      shape('real time', Mesh.cube(), const Color(0xFF4CA6E0), at: Vec3(2, 0.6, 0), components: [Turn(scaled: false)]),
    );
    add(shape('ball', Mesh.sphere(), const Color(0xFFFFD040), size: Vec3(0.4, 0.4, 0.4), components: [Circle()]));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final t = time;
    if (t.unscaledDelta >= t.maxDelta) {
      log.add('frame ${t.frame}: a long frame, counted as maxDelta = ${secs(t.maxDelta)}');
    }
    readout.value = [
      'frame ${t.frame}   scale ${t.scale.toStringAsFixed(2)}   maxDelta ${secs(t.maxDelta)}',
      'delta ${(t.delta * 1000).toStringAsFixed(1)} ms   unscaledDelta ${(t.unscaledDelta * 1000).toStringAsFixed(1)} ms',
      'sinceStart ${secs(t.sinceStart)}   unscaledSinceStart ${secs(t.unscaledSinceStart)}',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Slider('time scale', () => time.scale, (v) => time.scale = v, max: 3),
    Button('pause', () => time.scale = 0),
    Button('play', () => time.scale = 1),
    Slider('maxDelta', () => time.maxDelta, (v) => time.maxDelta = v, min: 0.02, max: 1),
    Button('freeze 1 s', () {
      log.add('freezing the app for 1 s…');
      sleep(const Duration(seconds: 1));
    }),
  ];
}

/// Turns its object, by game time or real time.
class Turn extends Behaviour {
  Turn({required this.scaled});
  final bool scaled;

  @override
  void onUpdate() {
    super.onUpdate();
    gameObject.rotate(Vec3.up, 1.5 * (scaled ? time.delta : time.unscaledDelta));
  }
}

/// Puts its object on a circle, by the game's time since it started.
class Circle extends Behaviour {
  @override
  void onUpdate() {
    super.onUpdate();
    final a = time.sinceStart;
    gameObject.transform = Transform(
      position: Vec3(2.5 * math.cos(a), 0.4, 2.5 * math.sin(a) + 1),
      scale: Vec3(0.4, 0.4, 0.4),
    );
  }
}
