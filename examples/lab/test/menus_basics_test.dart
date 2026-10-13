// The lab's 'Menus: the basics' and 'Rebinding: the basics' pages, driven
// as a player would: the menu and a page over it pause the game, and a
// key pressed in a dialog becomes the jump key.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show KeyCode;
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:lab/lab.dart';
import 'package:lab/main.dart';
import 'package:lab/scenes/menus_basics.dart';
import 'package:lab/scenes/rebinding_basics.dart';

void main() {
  // The game's frames go on for ever, so no pumpAndSettle: a while of them.
  Future<void> wait(WidgetTester tester) async {
    for (var i = 0; i < 30; i++) {
      await tester.pump(const Duration(microseconds: 16667));
    }
  }

  testWidgets('Menus: the basics: the menu and a page over it pause the game; closing both goes on', (tester) async {
    tester.view.physicalSize = const Size(1000, 700);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(const MaterialApp(home: ScenePage(MenusBasicsScene.new)));
    await wait(tester);
    final scene = FynkelGame.of<LabScene>(tester.element(find.byType(FynkelView))).game as MenusBasicsScene;
    final turned = scene.cube.rotation;
    expect(scene.world.paused, isFalse);
    await tester.tap(find.text('Menu'));
    await wait(tester);
    expect(scene.world.paused, isTrue);
    final at = scene.cube.rotation;
    expect(at, isNot(turned));
    await tester.tap(find.text('Open a page'));
    await wait(tester);
    expect(scene.world.paused, isTrue);
    expect(scene.cube.rotation, at, reason: 'stopped');
    await tester.tap(find.byType(BackButton));
    await wait(tester);
    expect(scene.world.paused, isTrue, reason: 'the dialog is still over it');
    await tester.tap(find.text('Resume'));
    await wait(tester);
    expect(scene.world.paused, isFalse);
    expect(scene.cube.rotation, isNot(at));
    expect(scene.log.value.map((l) => l.text), containsAllInOrder(['paused', 'going on']));
  });

  testWidgets('Rebinding: the basics: the button binds jump to the key pressed; the cube hops on it', (tester) async {
    tester.view.physicalSize = const Size(1000, 700);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(const MaterialApp(home: ScenePage(RebindingBasicsScene.new)));
    await wait(tester);
    final scene = FynkelGame.of<LabScene>(tester.element(find.byType(FynkelView))).game as RebindingBasicsScene;
    await tester.tap(find.text('Jump: Space'));
    await wait(tester);
    expect(scene.world.paused, isTrue);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyJ);
    await wait(tester);
    expect(find.text('Jump: J'), findsOneWidget);
    expect(scene.jump.buttons, [KeyCode.j]);
    expect(scene.world.paused, isFalse);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.keyJ);
    await wait(tester);
    expect(scene.log.value.map((l) => l.text), contains('hop (J)'));
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyJ);
    await wait(tester);
  });
}
