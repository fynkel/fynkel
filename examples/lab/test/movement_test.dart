// The lab's 'Look: movement' page, driven as a player would: its four
// shaders load from the assets and move, the blades share one material,
// a poke wobbles one jelly and not the others, and the wind slider
// carries the grass's reach with it. With `LAB_SHOT=<dir>`, pictures of
// it at each level, to look at.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show Camera, HeadlessView, MeshRenderer, Quality;
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:lab/lab.dart';
import 'package:lab/main.dart';
import 'package:lab/scenes/movement.dart';

void main() {
  // The game's frames go on for ever, so no pumpAndSettle: a while of them.
  Future<void> wait(WidgetTester tester, [int frames = 10]) async {
    for (var i = 0; i < frames; i++) {
      await tester.pump(const Duration(microseconds: 16667));
    }
  }

  testWidgets('Look: movement: they load and move, a jelly wobbles alone, and the wind carries the reach', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1000, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(home: ScenePage(MovementScene.new)));
    final scene = FynkelGame.of<LabScene>(tester.element(find.byType(FynkelView).first)).game as MovementScene;
    // The files and the Knight load off the frame.
    for (var i = 0; i < 100 && scene.cape == null; i++) {
      await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
      await wait(tester, 1);
    }
    expect(scene.cape, isNotNull, reason: 'the shaders and the Knight loaded');
    expect(scene.log.value.where((l) => l.error), isEmpty);
    for (final material in [scene.grass!, scene.water!, scene.cape!]) {
      expect(material.shader!.movesVertices, isTrue, reason: '${material.shader}');
    }
    final blades = [for (var i = 0; i < 20; i++) scene.find('blade $i')!.get<MeshRenderer>()!.material];
    expect(blades.toSet(), hasLength(1), reason: 'the blades share one material');
    expect(scene.find('Knight_Cape')!.get<MeshRenderer>()!.material, same(scene.cape));
    expect(scene.find('Knight_Body')!.get<MeshRenderer>()!.material!.shader, isNull, reason: 'the cape alone');

    Future<void> tap(String label) async {
      await tester.ensureVisible(find.text(label));
      await tester.pump();
      await tester.tap(find.text(label));
      await wait(tester, 2);
    }

    await tap('poke a jelly');
    final poked = [for (final j in scene.jellies) j.get<MeshRenderer>()!.number('poke')];
    expect(poked.where((p) => p > 0), hasLength(1), reason: 'one jelly wobbles: $poked');
    expect(scene.readout.value, contains('pokes 1'));
    await wait(tester, 90);
    expect([for (final j in scene.jellies) j.get<MeshRenderer>()!.number('poke')], everyElement(0.0));

    scene.wind = 0.4;
    expect(scene.grass!.value('wind'), closeTo(0.4, 1e-6));
    expect(scene.grass!.reach, closeTo(0.4, 1e-6));
    scene.waves = 1.5;
    expect(scene.water!.value('height'), closeTo(1.5, 1e-6));
    await wait(tester, 1);
    expect(scene.readout.value, contains('wind 0.40 m'));

    // The pixel look, while the camera drifts.
    await tester.ensureVisible(find.text('pixel look'));
    await tester.pump();
    await tester.tap(find.text('pixel look'));
    await wait(tester, 2);
    expect(scene.find('camera')!.get<Camera>()!.pixels, 180);
    expect(scene.readout.value, contains('180 px high'));
    scene.drift = true;
    final from = scene.find('camera')!.position;
    await wait(tester, 30);
    expect(scene.find('camera')!.position.x, isNot(from.x), reason: 'the camera drifts');
    scene.drift = false;

    final shots = Platform.environment['LAB_SHOT'];
    if (shots != null) {
      for (final quality in Quality.levels) {
        scene.world.quality = quality;
        final view = HeadlessView(scene.world, width: 960, height: 640);
        File('$shots/movement_pixels_${quality.name}.png').writeAsBytesSync(view.png());
        view.dispose();
      }
      scene.pixelLook = false;
      for (final quality in Quality.levels) {
        scene.world.quality = quality;
        final view = HeadlessView(scene.world, width: 960, height: 640);
        File('$shots/movement_${quality.name}.png').writeAsBytesSync(view.png());
        view.dispose();
      }
    }
  });
}
