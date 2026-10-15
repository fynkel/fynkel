// The lab's 'Look: everything' page, driven as a player would: every part
// of stage 8's look is in it, its buttons set the world's quality level,
// and night turns the sun off and the lamps on. With `LAB_SHOT=<dir>`,
// pictures of it at each level, by day and by night, to look at.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show Blending, Camera, HeadlessView, Light, LightKind, MeshRenderer, Quality;
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:lab/lab.dart';
import 'package:lab/main.dart';
import 'package:lab/scenes/look.dart';

void main() {
  // The game's frames go on for ever, so no pumpAndSettle: a while of them.
  Future<void> wait(WidgetTester tester, [int frames = 10]) async {
    for (var i = 0; i < frames; i++) {
      await tester.pump(const Duration(microseconds: 16667));
    }
  }

  testWidgets('Look: everything: all of the look, the levels, and night', (tester) async {
    tester.view.physicalSize = const Size(1000, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(home: ScenePage(LookScene.new)));
    // The sky image loads first.
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 300)));
    await wait(tester);
    final scene = FynkelGame.of<LabScene>(tester.element(find.byType(FynkelView).first)).game as LookScene;
    Future<void> tap(String label) async {
      await tester.ensureVisible(find.text(label));
      await tester.tap(find.text(label));
      await wait(tester, 2);
    }

    // Everything stage 8 brought.
    final world = scene.world;
    final lights = world.all<Light>();
    final sun = lights.firstWhere((l) => l.kind == LightKind.sun && l.shadows);
    final spot = lights.firstWhere((l) => l.kind == LightKind.spot);
    expect(spot.shadows, isTrue, reason: 'a spotlight casting shadows');
    expect(lights.where((l) => l.kind == LightKind.point).length, greaterThanOrEqualTo(4), reason: 'lamps');
    expect(world.sky, isNotNull);
    expect(world.fog, isNotNull);
    final materials = world.all<MeshRenderer>().map((r) => r.material).nonNulls.toList();
    expect(materials.where((m) => m.blending == Blending.transparent), isNotEmpty, reason: 'glass and water');
    expect(materials.where((m) => m.emissiveIntensity > 1), isNotEmpty, reason: 'something that glows');
    final camera = scene.find('camera')!.get<Camera>()!;
    expect((camera.bloom, camera.ambientOcclusion), isNot((null, null)));

    expect(world.quality, isNull);
    expect(scene.readout.value, startsWith('drawn at ${Quality.device.name} (the device picks)   day'));

    await tap('night');
    expect(scene.night, isTrue);
    expect(sun.intensity, 0);
    expect(spot.intensity, greaterThan(0));
    expect(scene.readout.value, contains('   night'));
    await tap('night');
    expect(sun.intensity, greaterThan(0));

    final shots = Platform.environment['LAB_SHOT'];
    for (final quality in Quality.values) {
      await tap(quality.name);
      expect(world.quality, quality);
      expect(scene.readout.value, startsWith('drawn at ${quality.name} '));
      if (shots != null) {
        for (final night in [false, true]) {
          scene.night = night;
          final view = HeadlessView(world, width: 1200, height: 800);
          File('$shots/look_${quality.name}_${night ? 'night' : 'day'}.png').writeAsBytesSync(view.png());
          view.dispose();
        }
        scene.night = false;
      }
    }
    await tap("device's");
    expect(world.quality, isNull);
  });
}
