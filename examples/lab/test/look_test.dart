// The lab's 'Look: quality levels' page, driven as a player would: its
// buttons set the world's quality level, and the readout says which one
// is drawn.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show Quality;
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

  testWidgets('Look: quality levels: each button sets the level drawn, and the device picks by default', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1000, 700);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(home: ScenePage(LookScene.new)));
    await wait(tester);
    final scene = FynkelGame.of<LabScene>(tester.element(find.byType(FynkelView).first)).game as LookScene;

    expect(scene.world.quality, isNull);
    expect(scene.readout.value, startsWith('drawn at ${Quality.device.name} (the device picks)'));
    for (final quality in Quality.values) {
      await tester.tap(find.text(quality.name));
      await wait(tester, 2);
      expect(scene.world.quality, quality);
      expect(scene.readout.value, startsWith('drawn at ${quality.name} '));
    }
    await tester.tap(find.text("device's"));
    await wait(tester, 2);
    expect(scene.world.quality, isNull);
  });
}
