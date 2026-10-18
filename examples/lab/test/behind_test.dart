// The lab's "Look: what's behind" page, driven as a player would: its
// three shaders load and read what's behind them, the sliders carry their
// values, and the glass ball and force field come and go. With
// `LAB_SHOT=<dir>`, pictures of it at each level, to look at.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show HeadlessView, Quality;
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:lab/lab.dart';
import 'package:lab/main.dart';
import 'package:lab/scenes/behind.dart';

void main() {
  // The game's frames go on for ever, so no pumpAndSettle: a while of them.
  Future<void> wait(WidgetTester tester, [int frames = 10]) async {
    for (var i = 0; i < frames; i++) {
      await tester.pump(const Duration(microseconds: 16667));
    }
  }

  testWidgets("Look: what's behind: they load and read it, and the sliders carry their values", (tester) async {
    tester.view.physicalSize = const Size(1000, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(home: ScenePage(BehindScene.new)));
    final scene = FynkelGame.of<LabScene>(tester.element(find.byType(FynkelView).first)).game as BehindScene;
    // The files and the Knight load off the frame.
    for (var i = 0; i < 100 && scene.knight == null; i++) {
      await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
      await wait(tester, 1);
    }
    expect(scene.knight, isNotNull, reason: 'the shaders and the Knight loaded');
    expect(scene.log.value.where((l) => l.error), isEmpty);
    for (final material in [scene.pond!, scene.glass!, scene.bubble!]) {
      expect(material.shader!.readsBehind, isTrue, reason: '${material.shader}');
    }
    expect(scene.pond!.shader!.movesVertices, isTrue);

    scene.foam = 0.2;
    scene.bend = 0.05;
    scene.waves = 1.5;
    expect(scene.pond!.value('foam'), closeTo(0.2, 1e-6));
    expect(scene.pond!.value('bend'), closeTo(0.05, 1e-6));
    expect(scene.pond!.value('height'), closeTo(1.5, 1e-6));
    await wait(tester, 1);
    expect(scene.readout.value, contains('foam 20 cm'));

    await tester.ensureVisible(find.text('glass ball and force field'));
    await tester.tap(find.text('glass ball and force field'));
    await wait(tester, 2);
    expect(scene.find('glass ball')!.active, isFalse);
    expect(scene.find('force field')!.active, isFalse);
    scene.extras = true;
    scene.foam = 0.12;
    scene.bend = 0.03;
    scene.waves = 1;

    final shots = Platform.environment['LAB_SHOT'];
    if (shots != null) {
      for (final quality in Quality.levels) {
        scene.world.quality = quality;
        final view = HeadlessView(scene.world, width: 960, height: 640);
        File('$shots/behind_${quality.name}.png').writeAsBytesSync(view.png());
        view.dispose();
      }
    }
  });
}
