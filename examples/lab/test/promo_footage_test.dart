// The promo film's footage of the lab's scenes: each scene alone in a
// world at high quality, a moment to load and settle, then recorded with
// a slow camera move of the film's own.
// FYNKEL_FOOTAGE=<dir> flutter test test/promo_footage_test.dart

import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:lab/lab.dart';
import 'package:lab/scenes/all_together.dart';
import 'package:lab/scenes/behind.dart';
import 'package:lab/scenes/crowd.dart';
import 'package:lab/scenes/glow.dart';
import 'package:lab/scenes/joints_bridge.dart';
import 'package:lab/scenes/lamps.dart';
import 'package:lab/scenes/meshes_change.dart';
import 'package:lab/scenes/particles_world.dart';
import 'package:lab/scenes/rigidbody_stack.dart';
import 'package:lab/scenes/shaders.dart';
import 'package:lab/scenes/skinned.dart';
import 'package:lab/scenes/sky.dart';
import 'package:lab/scenes/splines.dart';
import 'package:lab/scenes/toon.dart';

import 'promo_recorder.dart';

Future<Uint8List> files(String asset) async => File('assets/$asset').readAsBytesSync();

typedef Shot = ({String name, LabScene Function() scene, double seconds, double warm, double degrees, double push, double rise});

final only = Platform.environment['FOOTAGE_ONLY']?.split(',');

void main() {
  final List<Shot> shots = [
    (name: 'lab_fireworks', scene: () => ParticlesWorldScene(), seconds: 7, warm: 1.5, degrees: 24, push: 0.15, rise: 0),
    (name: 'lab_together', scene: () => AllTogetherScene(quality: 'high'), seconds: 8, warm: 2, degrees: 40, push: 0.2, rise: -1),
    (name: 'lab_crowd', scene: () => CrowdScene(count: 60), seconds: 7, warm: 1, degrees: 35, push: 0.25, rise: -2),
    (name: 'lab_skinned', scene: () => SkinnedScene(), seconds: 7, warm: 1, degrees: 50, push: 0.1, rise: 0),
    (name: 'lab_toon', scene: () => ToonScene(), seconds: 6, warm: 1, degrees: 40, push: 0.1, rise: 0),
    (name: 'lab_shaders', scene: () => ShadersScene(), seconds: 6, warm: 1, degrees: 30, push: 0.15, rise: 0),
    (name: 'lab_bridge', scene: () => JointsBridgeScene(), seconds: 7, warm: 0.5, degrees: 30, push: 0.1, rise: 0),
    (name: 'lab_stack', scene: () => RigidbodyStackScene(), seconds: 6, warm: 0.2, degrees: 25, push: 0.1, rise: 0),
    (name: 'lab_glow', scene: () => GlowScene(), seconds: 6, warm: 1, degrees: 30, push: 0.15, rise: 0),
    (name: 'lab_lamps', scene: () => LampsScene(), seconds: 6, warm: 1, degrees: 30, push: 0.15, rise: -1),
    (name: 'lab_sky', scene: () => SkyScene(), seconds: 6, warm: 1, degrees: 30, push: 0.1, rise: 0),
    (name: 'lab_splines', scene: () => SplinesScene(), seconds: 6, warm: 1, degrees: 30, push: 0.1, rise: 0),
    (name: 'lab_meshes', scene: () => MeshesChangeScene(), seconds: 6, warm: 1, degrees: 30, push: 0.1, rise: 0),
    (name: 'lab_behind', scene: () => BehindScene(), seconds: 6, warm: 1, degrees: 30, push: 0.1, rise: 0),
  ];
  for (final shot in shots) {
    if (only != null && !only!.contains(shot.name)) continue;
    test(shot.name, () async {
      final world = World(source: files)..quality = Quality.high;
      addTearDown(world.dispose);
      world.input.simulate.resize(1920, 1080);
      final scene = world.add(shot.scene());
      // Loading and settling: frames, with time for the files to come in.
      for (var i = 0; i < (shot.warm * 60).round(); i++) {
        world.update(1 / 60);
        await Future<void>.delayed(const Duration(milliseconds: 4));
      }
      final camera = scene.find('camera')!;
      await record(
        world,
        shot.name,
        shot.seconds,
        after: orbit(camera, scene.eye, scene.target, shot.seconds, degrees: shot.degrees, push: shot.push, rise: shot.rise),
      );
    }, skip: skipFootage, timeout: const Timeout(Duration(minutes: 10)));
  }
}
