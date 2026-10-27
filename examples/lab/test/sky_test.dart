// The lab's 'Look: sky and fog' page, driven as a player would: the image
// sky arrives from its file, the buttons switch skies, fog comes and goes,
// and the levels change.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:lab/lab.dart';
import 'package:lab/main.dart';
import 'package:lab/scenes/sky.dart';

void main() {
  // The game's frames go on for ever, so no pumpAndSettle: a while of them.
  Future<void> wait(WidgetTester tester, [int frames = 10]) async {
    for (var i = 0; i < frames; i++) {
      await tester.pump(const Duration(microseconds: 16667));
    }
  }

  testWidgets('Look: sky and fog: the image arrives, the buttons switch skies, fog comes and goes', (tester) async {
    tester.view.physicalSize = const Size(1000, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(home: ScenePage(SkyScene.new)));
    await wait(tester);
    final scene = FynkelGame.of<LabScene>(tester.element(find.byType(FynkelView).first)).game as SkyScene;
    Future<void> tap(String label) async {
      await tester.ensureVisible(find.text(label));
      await tester.pump();
      await tester.tap(find.text(label));
      await wait(tester, 2);
    }

    // The .hdr decodes on a helper isolate: real time, not the test's.
    for (var i = 0; i < 100 && scene.sky != 'image'; i++) {
      await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
      await wait(tester, 1);
    }
    expect(scene.sky, 'image');
    final image = scene.world.sky as ImageSky;
    expect((image.image.width, image.image.height), (1024, 512));
    expect(scene.world.fog, isNotNull);
    expect(scene.readout.value, contains('sky: image with fog'));

    await tap('sky: day');
    expect(scene.world.sky, isA<GradientSky>());
    await tap('sky: night');
    expect((scene.world.sky as GradientSky).zenith, const Color(0xFF04060E));
    await tap('sky: plain');
    expect(scene.world.sky, isA<ColorSky>());
    await tap('sky: none');
    expect(scene.world.sky, isNull);
    await tap('fog');
    expect(scene.world.fog, isNull);
    expect(scene.readout.value, contains('sky: none   '));

    for (final quality in Quality.levels) {
      await tap(quality.name);
      expect(scene.world.quality, quality);
      expect(scene.readout.value, startsWith('drawn at ${quality.name} '));
    }
  });
}
