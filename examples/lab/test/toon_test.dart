// The lab's "Look: toon" page, driven as a player would: the shader loads
// and lights the Knight, the Rogue and the shapes in bands, outlined; the
// switches put the engine's light back and take the outlines away. With
// `LAB_SHOT=<dir>`, pictures of it at each level, to look at.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show HeadlessView, MeshRenderer, Quality;
import 'package:fynkel_flutter/testing.dart';
import 'package:lab/main.dart';
import 'package:lab/scenes/toon.dart';

void main() {
  testWidgets('Look: toon: it lights in bands, outlined, and the switches carry it', (tester) async {
    tester.view.physicalSize = const Size(1000, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(home: ScenePage(ToonScene.new)));
    final scene = tester.game<ToonScene>();
    for (var i = 0; i < 100 && scene.rogue == null; i++) {
      await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
      await tester.frames(1);
    }
    expect(scene.rogue, isNotNull, reason: 'the shader and the characters loaded');
    expect(scene.log.value.where((l) => l.error), isEmpty);
    expect(scene.toon!.hasOwnLighting, isTrue);
    final ball = scene.find('ball')!.get<MeshRenderer>()!;
    expect(ball.material!.shader, scene.toon);
    expect(ball.material!.outline, isNotNull);

    scene.bands = 4.4;
    expect(ball.material!.value('bands'), 4.0);
    await tester.frames(1);
    expect(scene.readout.value, contains('4 bands'));

    await tester.ensureVisible(find.text('outlines'));

    await tester.pump();
    await tester.tap(find.text('outlines'));
    await tester.frames(2);
    expect(ball.material!.outline, isNull);
    await tester.tap(find.text('toon'));
    await tester.frames(2);
    expect(ball.material!.shader, isNull, reason: "the engine's own light");
    expect(scene.readout.value, contains("the engine's light"));

    final shots = Platform.environment['LAB_SHOT'];
    if (shots != null) {
      File('$shots/toon_plain.png').writeAsBytesSync(_shot(scene));
    }
    scene
      ..styled = true
      ..outlines = true
      ..bands = 3;
    if (shots != null) {
      for (final quality in Quality.levels) {
        scene.world.quality = quality;
        File('$shots/toon_${quality.name}.png').writeAsBytesSync(_shot(scene));
      }
    }
  });
}

List<int> _shot(ToonScene scene) {
  final view = HeadlessView(scene.world, width: 960, height: 640);
  final png = view.png();
  view.dispose();
  return png;
}
