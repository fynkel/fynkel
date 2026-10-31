// The lab's 'Look: glass, water and glows' page, driven as a player would:
// the glass turns, its opacity slider changes all three panes, the crowd
// comes and goes, and the levels change.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show Blending, MeshRenderer, Quality;
import 'package:fynkel_flutter/testing.dart';
import 'package:lab/main.dart';
import 'package:lab/scenes/see_through.dart';

void main() {
  testWidgets('Look: glass, water and glows: the glass turns, the slider and the crowd change it', (tester) async {
    tester.view.physicalSize = const Size(1000, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(home: ScenePage(SeeThroughScene.new)));
    await tester.frames(10);
    final scene = tester.game<SeeThroughScene>();
    Future<void> tap(String label) async {
      await tester.ensureVisible(find.text(label));
      await tester.pump();
      await tester.tap(find.text(label));
      await tester.frames(2);
    }

    Blending blending(String name) => scene.find(name)!.get<MeshRenderer>()!.material!.blending;
    expect(blending('pane 0'), Blending.transparent);
    expect(blending('water'), Blending.transparent);
    expect(blending('spark 0'), Blending.additive);
    expect(blending('cube 0'), Blending.opaque);
    expect(scene.opacity, closeTo(0.35, 1e-9));
    final turned = scene.find('pane 0')!.rotation;
    await tester.frames(20);
    expect(scene.find('pane 0')!.rotation, isNot(turned), reason: 'the glass turns');

    scene.opacity = 0.8;
    for (final name in ['pane 0', 'pane 1', 'pane 2']) {
      expect(scene.find(name)!.get<MeshRenderer>()!.material!.opacity, closeTo(0.8, 1e-9));
    }
    await tester.frames(2);
    expect(scene.readout.value, contains('glass 80 % opaque'));

    await tap('crowd of $crowdSize panes');
    expect(scene.crowd, isTrue);
    expect(scene.find('crowd 0'), isNotNull);
    expect(scene.readout.value, contains('crowd of $crowdSize'));
    await tap('crowd of $crowdSize panes');
    expect(scene.find('crowd 0'), isNull);

    for (final quality in Quality.levels) {
      await tap(quality.name);
      expect(scene.world.quality, quality);
      expect(scene.readout.value, startsWith('drawn at ${quality.name} '));
    }
  });
}
