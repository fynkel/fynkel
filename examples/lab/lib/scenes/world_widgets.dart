// Stage 15: UI in the world. Walkers go round behind a ring of pillars,
// each with a name tag that hides while a pillar is in front of it
// (hideBehind); a flag far off keeps an arrow at the screen's edge while
// it's off the screen (keepOnScreen). "sync check" spins the camera round
// a small white cube at 180°/s with a red cross on it: filmed, the cross
// should never leave the cube.

import 'dart:math' as math;

import 'package:flutter/material.dart' show Alignment;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';
import '../widgets/tags.dart';

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
  late final GameObject flag, marker;
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
    // `--dart-define=LAB_SYNC_CHECK=true` starts with the sync check on.
    if (const bool.fromEnvironment('LAB_SYNC_CHECK')) controls.whereType<Toggle>().last.onChanged(true);
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
    Toggle('sync check', () => syncCheck, (on) {
      syncCheck = on;
      marker.active = on;
      // Every frame the display shows, so each is a step to compare.
      world.targetFrameRate = on ? 0 : 60;
      // The cube alone, nothing in front of it.
      for (final o in [...walkers, ...pillars, flag]) {
        o.active = !on;
      }
    }),
  ];
}
