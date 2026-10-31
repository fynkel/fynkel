// Hardening G7: the keys a player can change, and the settings list for
// them, which arena and sheepdog each wrote.

import 'package:flutter/material.dart' hide Transform;
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

Controls make() =>
    Controls({'up': KeyCode.w, 'down': KeyCode.s, 'left': KeyCode.a, 'right': KeyCode.d, 'dash': KeyCode.space});

void main() {
  test('set gives a name a key, and what had that key takes the old one', () {
    final controls = make();
    var told = 0;
    controls.addListener(() => told++);
    controls.set('up', KeyCode.d);
    expect((controls['up'], controls['right']), (KeyCode.d, KeyCode.w));
    expect(told, 1);
    controls.set('up', KeyCode.d);
    expect(told, 1, reason: 'no change');
    controls.reset();
    expect((controls['up'], controls['right']), (KeyCode.w, KeyCode.d));
    expect(() => controls['jump'], throwsArgumentError);
    expect(() => controls.set('jump', KeyCode.j), throwsArgumentError);
  });

  test('bindTo: up, down, left and right are the move axis, with the arrows; the rest are actions', () {
    final world = World();
    addTearDown(world.dispose);
    final controls = make()..set('dash', KeyCode.e);
    controls.bindTo(world.input);
    world.input.simulate.keyDown(KeyCode.w);
    world.input.simulate.keyDown(KeyCode.e);
    world.update(1 / 60);
    expect(world.input.axis2('move'), const Vec2(0, 1));
    expect(world.input.action('dash').isPressed, isTrue);
    world.input.simulate
      ..keyUp(KeyCode.w)
      ..keyDown(KeyCode.arrowLeft);
    world.update(1 / 60);
    expect(world.input.axis2('move'), const Vec2(-1, 0), reason: 'the arrows move too');
  });

  test('toJson and Controls.fromJson keep them between runs; what fromJson does not know is left', () {
    final controls = make()..set('dash', KeyCode.e);
    final json = controls.toJson();
    expect(json['dash'], 'e');
    final again = Controls.fromJson({...json, 'jump': 'j', 'up': 'no such key', 'down': 3}, defaults: make().defaults);
    expect(again['dash'], KeyCode.e);
    expect((again['up'], again['down']), (KeyCode.w, KeyCode.s));
    expect(again.defaults, make().defaults, reason: 'reset goes back to the defaults, not to what was saved');
    expect(Controls.fromJson(null, defaults: make().defaults)['dash'], KeyCode.space, reason: 'nothing saved yet');
  });

  testWidgets('ControlsSettings lists them, changes one with the key pressed, and resets', (tester) async {
    final controls = make();
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ControlsSettings(controls, labels: const {'up': 'Up', 'dash': 'Dash'}, note: 'The arrows move too.'),
        ),
      ),
    );
    expect(find.text('Up'), findsOneWidget);
    expect(find.text('Dash'), findsOneWidget);
    expect(find.text('Down'), findsNothing, reason: 'only the labelled ones');
    expect(find.text('The arrows move too.'), findsOneWidget);
    expect(find.text(KeyCode.space.label), findsOneWidget);

    await tester.tap(find.text(KeyCode.space.label));
    await tester.pumpAndSettle();
    expect(find.text('Press a key for Dash'), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyE, physicalKey: PhysicalKeyboardKey.keyE);
    await tester.pumpAndSettle();
    expect(controls['dash'], KeyCode.e);
    expect(find.text(KeyCode.e.label), findsOneWidget);

    await tester.tap(find.text(KeyCode.e.label));
    await tester.pumpAndSettle();
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pumpAndSettle();
    expect(controls['dash'], KeyCode.e, reason: 'Escape keeps it');

    await tester.tap(find.text('Reset'));
    await tester.pump();
    expect(controls['dash'], KeyCode.space);
  });
}
