// The lab's 'Look: shaders' page, driven as a player would: its four
// shaders load from the assets, a hit flashes one enemy and not the
// others, and the dissolve and the lava follow their sliders. With
// `LAB_SHOT=<dir>`, pictures of it at each level, to look at.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show HeadlessView, MeshRenderer, Quality;
import 'package:fynkel_flutter/testing.dart';
import 'package:lab/main.dart';
import 'package:lab/scenes/shaders.dart';

void main() {
  testWidgets('Look: shaders: they load, an enemy flashes alone, and the sliders change them', (tester) async {
    tester.view.physicalSize = const Size(1000, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(home: ScenePage(ShadersScene.new)));
    final scene = tester.game<ShadersScene>();
    // The files load off the frame.
    for (var i = 0; i < 50 && scene.dissolve == null; i++) {
      await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
      await tester.frames(1);
    }
    expect(scene.dissolve, isNotNull, reason: 'the shaders loaded');
    expect(scene.log.value.where((l) => l.error), isEmpty);
    for (final name in ['lava', 'enemy 0', 'statue', 'hologram']) {
      expect(scene.find(name)!.get<MeshRenderer>()!.material!.shader, isNotNull, reason: name);
    }
    final shared = scene.enemies.map((e) => e.get<MeshRenderer>()!.material).toSet();
    expect(shared, hasLength(1), reason: 'the enemies share one material');

    Future<void> tap(String label) async {
      await tester.ensureVisible(find.text(label));
      await tester.pump();
      await tester.tap(find.text(label));
      await tester.frames(2);
    }

    await tap('hit');
    final flashing = [for (final e in scene.enemies) e.get<MeshRenderer>()!.number('flash')];
    expect(flashing.where((f) => f > 0), hasLength(1), reason: 'one enemy flashes: $flashing');
    expect(scene.readout.value, contains('hits 1'));
    await tester.frames(30);
    expect([for (final e in scene.enemies) e.get<MeshRenderer>()!.number('flash')], everyElement(0.0));

    await tester.frames(20);
    final before = scene.amount;
    await tester.frames(20);
    expect(scene.amount, isNot(before), reason: 'it dissolves by itself');
    scene.amount = 0.3;
    expect(scene.dissolve!.value('amount'), 0.3);
    scene.flow = 1.2;
    expect(scene.lava!.value('flow'), closeTo(1.2, 1e-6));
    await tester.frames(1);
    expect(scene.readout.value, contains('lava flow 1.20'));

    final shots = Platform.environment['LAB_SHOT'];
    if (shots != null) {
      await tap('dissolve by itself');
      scene.amount = 0.45;
      for (final quality in Quality.levels) {
        scene.world.quality = quality;
        final view = HeadlessView(scene.world, width: 960, height: 640);
        File('$shots/shaders_${quality.name}.png').writeAsBytesSync(view.png());
        view.dispose();
      }
    }
  });
}
