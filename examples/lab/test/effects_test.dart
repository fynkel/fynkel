// The lab's "Look: screen effects" page, driven as a player would: the
// effects load and go on the camera in the order the switches say, a hit
// flashes and goes, and the pixel look comes and goes under them. With
// `LAB_SHOT=<dir>`, pictures of it at each level, to look at.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show HeadlessView, Quality;
import 'package:fynkel_flutter/testing.dart';
import 'package:lab/main.dart';
import 'package:lab/scenes/effects.dart';

void main() {
  testWidgets('Look: screen effects: they run in the order asked, and a hit comes and goes', (tester) async {
    tester.view.physicalSize = const Size(1000, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(home: ScenePage(EffectsScene.new)));
    final scene = tester.game<EffectsScene>();
    for (var i = 0; i < 100 && scene.rogue == null; i++) {
      await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
      await tester.frames(1);
    }
    expect(scene.rogue, isNotNull, reason: 'the shaders and the Rogue loaded');
    expect(scene.log.value.where((l) => l.error), isEmpty);
    expect(scene.camera.effects, [scene.tv]);

    await tester.ensureVisible(find.text('four colours'));

    await tester.pump();
    await tester.tap(find.text('four colours'));
    await tester.frames(2);
    expect(scene.camera.effects, [scene.palette, scene.tv]);
    expect(scene.readout.value, contains('palette.wgsl → old_tv.wgsl'));
    await tester.ensureVisible(find.text('TV before the palette'));
    await tester.pump();
    await tester.tap(find.text('TV before the palette'));
    await tester.frames(2);
    expect(scene.camera.effects, [scene.tv, scene.palette]);

    await tester.ensureVisible(find.text('hit'));

    await tester.pump();
    await tester.tap(find.text('hit'));
    await tester.frames(2);
    expect(scene.camera.effects.last, scene.hit);
    await tester.frames(40);
    expect(scene.camera.effects, isNot(contains(scene.hit)), reason: 'the flash is over');
    expect(scene.readout.value, contains('hits 1'));

    await tester.ensureVisible(find.text('pixel look'));

    await tester.pump();
    await tester.tap(find.text('pixel look'));
    await tester.frames(2);
    expect(scene.camera.pixels, 160);

    // The colour grade and the scanner go first, over the picture as drawn.
    await tester.ensureVisible(find.text('colour grade'));
    await tester.pump();
    await tester.tap(find.text('colour grade'));
    await tester.ensureVisible(find.text('scanner'));
    await tester.pump();
    await tester.tap(find.text('scanner'));
    await tester.frames(2);
    expect(scene.camera.effects.take(2), [scene.scanner, scene.grade]);
    expect(scene.grade!.shader.textures, ['table']);
    expect(scene.scanner!.shader.readsBehind, isTrue, reason: 'it reads the scene\'s depth');
    expect(scene.log.value.where((l) => l.error), isEmpty);
    scene
      ..colourGrade = false
      ..scanning = false;

    final shots = Platform.environment['LAB_SHOT'];
    if (shots != null) {
      void shot(String name) {
        for (final quality in Quality.levels) {
          scene.world.quality = quality;
          final view = HeadlessView(scene.world, width: 960, height: 640);
          File('$shots/effects_${name}_${quality.name}.png').writeAsBytesSync(view.png());
          view.dispose();
        }
      }

      shot('tv_palette_pixels');
      scene
        ..pixelLook = false
        ..fourColours = false;
      shot('tv');
      scene
        ..oldTv = false
        ..fourColours = true;
      shot('palette');
      scene
        ..fourColours = false
        ..colourGrade = true;
      shot('grade');
      scene
        ..colourGrade = false
        ..scanning = true;
      shot('scanner');
    }
  });
}
