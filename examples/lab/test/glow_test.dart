// The lab's 'Look: glow and contact' page, driven as a player would: bloom
// and ambient occlusion on, their sliders, off, and the levels. With
// `LAB_SHOT=<dir>`, pictures of it at each level, with and without, to
// look at.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show AmbientOcclusion, Bloom, Camera, HeadlessView, Quality;
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:lab/lab.dart';
import 'package:lab/main.dart';
import 'package:lab/scenes/glow.dart';

void main() {
  // The game's frames go on for ever, so no pumpAndSettle: a while of them.
  Future<void> wait(WidgetTester tester, [int frames = 10]) async {
    for (var i = 0; i < frames; i++) {
      await tester.pump(const Duration(microseconds: 16667));
    }
  }

  testWidgets('Look: glow and contact: bloom and ambient occlusion on, changed, and off', (tester) async {
    tester.view.physicalSize = const Size(1000, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(home: ScenePage(GlowScene.new)));
    await wait(tester);
    final scene = FynkelGame.of<LabScene>(tester.element(find.byType(FynkelView).first)).game as GlowScene;
    Future<void> tap(String label) async {
      await tester.ensureVisible(find.text(label));
      await tester.pump();
      await tester.tap(find.text(label));
      await wait(tester, 2);
    }

    final camera = scene.find('camera')!.get<Camera>()!;
    expect(camera.bloom, const Bloom());
    expect(camera.ambientOcclusion, const AmbientOcclusion());
    final turned = scene.find('block 0')!.position;
    await wait(tester, 20);
    expect(scene.find('block 0')!.position, isNot(turned), reason: 'the ring turns');

    scene.strength = 2;
    scene.intensity = 3;
    expect(camera.bloom, const Bloom(strength: 2));
    expect(camera.ambientOcclusion, const AmbientOcclusion(intensity: 3));

    final shots = Platform.environment['LAB_SHOT'];
    for (final quality in Quality.levels) {
      await tap(quality.name);
      expect(scene.world.quality, quality);
      expect(scene.readout.value, startsWith('drawn at ${quality.name} '));
      expect(scene.readout.value, contains(quality == Quality.low ? 'bloom on, not drawn at low' : 'bloom on '));
      if (shots != null) {
        scene.strength = 1;
        scene.intensity = 1;
        for (final on in [true, false]) {
          scene
            ..bloom = on
            ..ambientOcclusion = on;
          final view = HeadlessView(scene.world, width: 960, height: 640);
          File('$shots/glow_${quality.name}${on ? '' : '_off'}.png').writeAsBytesSync(view.png());
          view.dispose();
        }
        scene
          ..bloom = true
          ..ambientOcclusion = true;
      }
    }

    await tap('bloom');
    await tap('ambient occlusion');
    expect((camera.bloom, camera.ambientOcclusion), (null, null));
    expect(scene.readout.value, contains('bloom off   ambient occlusion off'));
  });
}
