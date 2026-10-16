// The lab's 'Look: shadows' page, driven as a player would: the sun and
// the spotlight cast shadows, its switches turn them off and on, and its
// buttons set the level they're drawn at.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show Light, Quality;
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:lab/lab.dart';
import 'package:lab/main.dart';
import 'package:lab/scenes/shadows.dart';

void main() {
  // The game's frames go on for ever, so no pumpAndSettle: a while of them.
  Future<void> wait(WidgetTester tester, [int frames = 10]) async {
    for (var i = 0; i < frames; i++) {
      await tester.pump(const Duration(microseconds: 16667));
    }
  }

  testWidgets('Look: shadows: the sun and the spotlight cast them, the switches and the levels change them', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1000, 700);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(home: ScenePage(ShadowsScene.new)));
    await wait(tester);
    final scene = FynkelGame.of<LabScene>(tester.element(find.byType(FynkelView).first)).game as ShadowsScene;
    Future<void> tap(String label) async {
      await tester.ensureVisible(find.text(label));
      await tester.tap(find.text(label));
      await wait(tester, 2);
    }

    final sun = scene.find('sun')!.get<Light>()!, torch = scene.find('torch')!.get<Light>()!;
    expect((sun.shadows, torch.shadows), (true, true));
    expect(scene.readout.value, contains('shadows: sun and spotlight'));
    final from = scene.find('camera')!.position;
    await wait(tester, 30);
    expect(scene.find('camera')!.position, isNot(from), reason: 'the camera drifts');

    await tap('sun shadows');
    expect(sun.shadows, isFalse);
    expect(scene.readout.value, contains('shadows: spotlight '));
    await tap('spotlight shadows');
    expect(torch.shadows, isFalse);
    expect(scene.readout.value, contains('shadows: none'));

    await tap('night');
    expect((scene.night, sun.intensity), (true, 0));

    for (final quality in Quality.levels) {
      await tap(quality.name);
      expect(scene.world.quality, quality);
      expect(scene.readout.value, startsWith('drawn at ${quality.name} '));
    }
  });
}
