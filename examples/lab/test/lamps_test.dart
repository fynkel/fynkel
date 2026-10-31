// The lab's 'Look: lamps' page, driven as a player would: its buttons set
// how many lights there are and the level they're drawn at, and the torch
// follows the pointer.

import 'package:flutter/gestures.dart' show PointerDeviceKind;
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show Light, LightKind, Quality, Vec3;
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_flutter/testing.dart';
import 'package:lab/main.dart';
import 'package:lab/scenes/lamps.dart';

void main() {
  testWidgets('Look: lamps: the buttons set the lights and the level, and the torch follows the pointer', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1000, 700);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(home: ScenePage(LampsScene.new)));
    await tester.frames(10);
    final scene = tester.game<LampsScene>();

    int lights() =>
        scene.children.where((o) => const [LightKind.point, LightKind.spot].contains(o.get<Light>()?.kind)).length;
    expect(scene.lamps, 31);
    expect(lights(), 32, reason: '31 lamps and the torch');
    expect(scene.readout.value, startsWith('32 lights   drawn at ${Quality.device.name} (the device picks)'));
    expect(scene.find('sun')!.active, isFalse, reason: 'night');

    for (final count in [64, 8, 32]) {
      await tester.tap(find.text('$count lights'));
      await tester.frames(2);
      expect((scene.lamps, lights()), (count - 1, count));
      expect(scene.readout.value, startsWith('$count lights '));
    }
    await tester.tap(find.text('low'));
    await tester.frames(2);
    expect(scene.world.quality, Quality.low);
    expect(scene.readout.value, contains('drawn at low '));

    // The torch turns to where the pointer is over the floor.
    final view = tester.getRect(find.byType(FynkelView).first);
    final torch = scene.find('torch')!;
    final pointer = await tester.createGesture(kind: PointerDeviceKind.mouse);
    addTearDown(pointer.removePointer);
    await pointer.addPointer(location: view.centerLeft + const Offset(40, 0));
    await tester.frames(2);
    final left = torch.forward;
    await pointer.moveTo(view.centerRight - const Offset(40, 0));
    await tester.frames(2);
    final right = torch.forward;
    expect(left.x, lessThan(0), reason: 'shining left: $left');
    expect(right.x, greaterThan(left.x + 0.2), reason: 'then right: $right');
    expect(right.dot(Vec3(0, -1, 0)), greaterThan(0.3), reason: 'down at the floor');
  });
}
