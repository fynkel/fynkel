// Stage 15: UI in the world. Walkers go round behind a ring of pillars,
// each with a name tag that hides while a pillar is in front of it
// (hideBehind); a flag far off keeps an arrow at the screen's edge while
// it's off the screen (keepOnScreen). "sync check" spins the camera round
// a small white cube at 180°/s with a red cross on it: filmed, the cross
// should never leave the cube. "100 tags among 3,000" is the phase's
// measure: 3,000 cubes, 100 of them tagged, hidden behind the rest.

import 'dart:math' as math;

import 'package:flutter/material.dart' show Alignment;
import 'package:flutter/scheduler.dart' show FrameTiming, SchedulerBinding;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';
import '../widgets/tags.dart';

/// `--dart-define=LAB_TAGS=true` starts with the 100 tags among 3,000 on;
/// `LAB_TAGS=bare`, with the 3,000 and no tags, and `LAB_TAGS=plain`, the
/// tags not hidden behind, to compare. With
/// `LAB_RATES` it prints the UI thread's time a frame, where the widgets
/// are placed and looked for.
const _startTags = String.fromEnvironment('LAB_TAGS');

/// A cube walking round, its name over it, hidden behind the pillars.
class Walker extends GameObject {
  Walker(String name, this.radius, this.speed, this._angle)
    : super(
        name: name,
        components: [MeshRenderer(Mesh.cube(), Material(color: const Color(0xFF4CA6E0)))],
      ) {
    addComponent(
      WorldWidget(
        (context) => NameTag(name),
        offset: Vec3(0, 0.8, 0),
        alignment: Alignment.bottomCenter,
        hideBehind: true,
      ),
    );
  }

  final double radius, speed;
  double _angle;

  @override
  void onUpdate() {
    super.onUpdate();
    _angle += speed * time.delta;
    position = Vec3(radius * math.sin(_angle), 0.5, radius * math.cos(_angle));
  }
}

class WorldWidgetsScene extends LabScene {
  @override
  String get title => 'UI in the world';
  @override
  String get step => 'stage 15';
  @override
  String get about =>
      'The walkers\' tags hide while a pillar is in front of them (WorldWidget hideBehind), fading. The flag '
      'far off keeps an arrow at the screen\'s edge while it\'s off the screen (keepOnScreen). "turn the '
      'camera" goes round; "sync check" spins it round a small white cube with a red cross on it at 180°/s: '
      'film the screen, and the cross should never leave the cube.';

  @override
  Vec3 get eye => Vec3(0, 4, 11);

  late final List<Walker> walkers;
  final pillars = <GameObject>[];
  late final GameObject flag, marker, crowd;
  final tagged = <GameObject>[];
  var turning = false, syncCheck = false;
  var degreesPerSecond = 30.0;
  var _angle = 0.0;

  @override
  void build() {
    for (var i = 0; i < 8; i++) {
      final a = i / 8 * 2 * math.pi;
      pillars.add(
        add(
          shape(
            'pillar $i',
            Mesh.cylinder(),
            const Color(0xFFB0A890),
            at: Vec3(3 * math.sin(a), 1.25, 3 * math.cos(a)),
            size: Vec3(0.7, 2.5, 0.7),
          ),
        ),
      );
    }
    walkers = [
      for (var i = 0; i < 4; i++) add(Walker('walker ${i + 1}', 4.5, i.isEven ? 0.35 : -0.25, i * math.pi / 2)),
    ];
    flag = add(
      shape(
        'flag',
        Mesh.cone(),
        const Color(0xFFFFD040),
        at: Vec3(14, 1, -14),
        size: Vec3(0.8, 2, 0.8),
        components: [
          WorldWidget((context) => const EdgeArrow('the flag'), offset: Vec3(0, 1.6, 0), keepOnScreen: true),
        ],
      ),
    );
    // A glowing ball: its middle on the screen is its middle from any side.
    marker = add(
      GameObject(
        name: 'marker',
        transform: Transform(position: Vec3(1.5, 3.2, 0), scale: Vec3(0.3, 0.3, 0.3)),
        components: [
          MeshRenderer(Mesh.sphere(), Material(color: Color.black, emissive: Color.white)),
          WorldWidget((context) => const Crosshair()),
        ],
      ),
    )..active = false;
    // 3,000 cubes in a block beyond the pillars; every 30th tagged, so the
    // tags are often behind other cubes.
    crowd = add(GameObject(name: '3,000'))..active = false;
    final cube = Mesh.cube();
    final grey = Material(color: const Color(0xFF8A8F98));
    for (var i = 0; i < 3000; i++) {
      final object = crowd.add(
        GameObject(
          transform: Transform(
            position: Vec3((i % 60 - 29.5) * 0.7, 0.2 + (i ~/ 60 % 5) * 0.7, -6 - (i ~/ 300) * 1.4),
            scale: Vec3(0.4, 0.4, 0.4),
          ),
          components: [MeshRenderer(cube, grey)],
        ),
      );
      if (i % 30 == 0 && _startTags != 'bare') {
        object.addComponent(
          WorldWidget((context) => NameTag('${i ~/ 30}'), offset: Vec3(0, 0.4, 0), hideBehind: _startTags != 'plain'),
        );
        tagged.add(object);
      }
    }
    if (_startTags.isNotEmpty) crowd.active = true;
    // `--dart-define=LAB_SYNC_CHECK=true` starts with the sync check on.
    if (const bool.fromEnvironment('LAB_SYNC_CHECK')) controls.whereType<Toggle>().last.onChanged(true);
  }

  // The UI thread's time a frame, from Flutter's frame timings.
  final _ui = <int>[];
  late final _timings = _uiTimes;

  void _uiTimes(List<FrameTiming> timings) {
    for (final t in timings) {
      _ui.add(t.buildDuration.inMicroseconds);
    }
    if (_ui.length < 120) return;
    _ui.sort();
    // ignore: avoid_print
    print(
      '[ui] ${crowd.active ? (tagged.isEmpty ? '3,000 bare' : '100 tags among 3,000, $_startTags') : 'the page'}: '
      'median ${(_ui[_ui.length ~/ 2] / 1000).toStringAsFixed(3)} ms, '
      'slowest ${(_ui.last / 1000).toStringAsFixed(3)} ms',
    );
    _ui.clear();
  }

  @override
  void onStart() {
    super.onStart();
    if (printRates) SchedulerBinding.instance.addTimingsCallback(_timings);
  }

  @override
  void onDestroy() {
    if (printRates) SchedulerBinding.instance.removeTimingsCallback(_timings);
    super.onDestroy();
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final camera = find('camera')!;
    if (syncCheck) {
      // Fast round a point beside the marker, so the marker swings across
      // the view: a frame's gap between the two would show.
      _angle += 180 * math.pi / 180 * time.delta;
      final at = Vec3(5 * math.sin(_angle), 3.2, 5 * math.cos(_angle));
      camera
        ..position = at
        ..rotation = Quat.lookRotation(Vec3(0, 3.2, 0) - at);
    } else if (turning) {
      _angle += degreesPerSecond * math.pi / 180 * time.delta;
      final at = Vec3(11 * math.sin(_angle), 4, 11 * math.cos(_angle));
      camera
        ..position = at
        ..rotation = Quat.lookRotation(target - at);
    }
    readout.value = 'camera at ${(_angle * 180 / math.pi % 360).round()}°';
  }

  @override
  List<Control> get controls => [
    Toggle('turn the camera', () => turning, (on) => turning = on),
    Slider('degrees a second', () => degreesPerSecond, (v) => degreesPerSecond = v, min: 10, max: 180),
    Toggle('100 tags among 3,000', () => crowd.active, (on) => crowd.active = on),
    Toggle('sync check', () => syncCheck, (on) {
      syncCheck = on;
      marker.active = on;
      // Every frame the display shows, so each is a step to compare.
      world.targetFrameRate = on ? 0 : 60;
      // The cube alone, nothing in front of it.
      if (on) crowd.active = false;
      for (final o in [...walkers, ...pillars, flag]) {
        o.active = !on;
      }
    }),
  ];
}
