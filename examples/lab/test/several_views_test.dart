// The lab's 'Minimap: the basics' and 'Split screen: the basics' pages,
// driven as a player would: two views of one world, which moves once a
// frame, each from its own camera.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' hide Material, Transform, Color;
import 'package:fynkel/fynkel.dart' as fy show Transform;
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_flutter/src/world_widget.dart' show WorldWidgetLayer;
import 'package:lab/lab.dart';
import 'package:lab/main.dart';
import 'package:lab/scenes/minimap_basics.dart';
import 'package:lab/scenes/split_screen_basics.dart';

void main() {
  // The game's frames go on for ever, so no pumpAndSettle: a while of them.
  Future<void> wait(WidgetTester tester, [int frames = 30]) async {
    for (var i = 0; i < frames; i++) {
      await tester.pump(const Duration(microseconds: 16667));
    }
  }

  Future<T> open<T extends LabScene>(WidgetTester tester, T Function() make) async {
    tester.view.physicalSize = const Size(1000, 700);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MaterialApp(home: ScenePage(make)));
    await wait(tester);
    return FynkelGame.of<LabScene>(tester.element(find.byType(FynkelView).first)).game as T;
  }

  testWidgets('Minimap: the basics: a second view follows the player from above, and moves it once a frame', (
    tester,
  ) async {
    final scene = await open(tester, MinimapBasicsScene.new);
    expect(find.byType(FynkelView), findsNWidgets(2));
    expect(find.byType(WorldWidgetLayer), findsOneWidget, reason: 'none over the minimap');
    final from = scene.player.position.x;
    await tester.sendKeyDownEvent(LogicalKeyboardKey.keyD);
    await wait(tester, 30);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyD);
    // Half a second at 5 m/s: once a frame, not twice.
    expect(scene.player.position.x - from, closeTo(2.5, 0.2));
    expect(scene.above.position.x, scene.player.position.x, reason: 'the minimap camera follows');

    // A tap on the minimap isn't the game's.
    final minimap = tester.getCenter(find.byType(FynkelView).last);
    final finger = await tester.startGesture(minimap);
    await wait(tester, 2);
    expect(scene.world.input.fingers, isEmpty);
    await finger.up();
    await wait(tester, 2);
  });

  testWidgets('Split screen: the basics: each half follows its player, and both show both name tags', (tester) async {
    final scene = await open(tester, SplitScreenBasicsScene.new);
    expect(find.byType(FynkelView), findsNWidgets(2));
    expect(find.byType(WorldWidgetLayer), findsNWidgets(2));
    expect(find.text('blue'), findsNWidgets(2), reason: 'its tag, placed in each half');
    final (blue, orange) = (scene.blue.position, scene.orange.position);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.keyW);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.arrowLeft);
    await wait(tester);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyW);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.arrowLeft);
    expect(scene.blue.position.z, lessThan(blue.z - 2), reason: 'W: away from the camera');
    expect(scene.orange.position.x, lessThan(orange.x - 2), reason: 'left');
    expect(scene.world.input.camera, scene.leftCamera, reason: 'the left half takes input');
  });

  testWidgets("views and world widgets don't wait for the physics step running between frames", (tester) async {
    final world = World();
    addTearDown(world.dispose);
    final above = Camera();
    world
      ..add(
        GameObject(
          transform: fy.Transform(position: Vec3(0, 2, 8)),
          components: [Camera()],
        ),
      )
      ..add(
        GameObject(
          transform: fy.Transform(position: Vec3(0, 20, 0)),
          components: [above],
        ),
      )
      ..add(
        GameObject(
          transform: fy.Transform(position: Vec3(0, 5, 0)),
          components: [BoxCollider(), Rigidbody(), WorldWidget((_) => const Text('box'))],
        ),
      );
    await tester.pumpWidget(
      MaterialApp(
        home: Stack(
          children: [
            Positioned.fill(child: FynkelView.world(world)),
            Positioned(
              left: 0,
              top: 0,
              width: 100,
              height: 100,
              child: FynkelView.world(world, camera: above, input: false),
            ),
          ],
        ),
      ),
    );
    var running = 0;
    for (var i = 0; i < 20; i++) {
      // 20 ms frames: a step falls due in each.
      await tester.pump(const Duration(milliseconds: 20));
      if (world.physics.debugStepRunning) running++;
    }
    // After each frame (its update, both views' frames, the widgets placed
    // and painted) the step it started is still running: nothing waited.
    expect(running, greaterThanOrEqualTo(18));
  });
}
