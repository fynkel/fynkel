// Unity parity: a component card's header drags onto another card's to
// take its place (docs/unity-parity.md § Tier 1, Component header). A
// line shows where it lands; the move is one `moveComponent`, one ⌘Z.
// The Transform neither moves nor lets anything above it, and a prefab's
// parts do not move.
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'harness.dart';

const crateB = 'c0000000-0000-4000-8000-0000000000e5';

void main() {
  testWidgets('a card dragged by its header takes the other card\'s place; ⌘Z puts it back', (tester) async {
    final e = await Editor.open(tester);
    final tab = e.shell.tabs!.focused!;
    await e.click(e.row('Crate B'));
    List<String> order() => [
      for (final c in ((tab.document.root()['entities'] as Map)[crateB]['components'] as Map).values)
        (c as Map)['type'] as String,
    ];
    expect(order(), ['fynkel.Transform', 'fynkel.RigidBody', 'fynkel.Collider', 'fynkel.Layer']);
    // A card's header: what is clicked to fold it and dragged to move it.
    Finder header(String key) =>
        find.descendant(of: find.byKey(ValueKey('card.$key')), matching: find.byType(InkWell)).first;
    Offset top(String key) => tester.getTopLeft(header(key)) + const Offset(60, 4);

    // Folded, as one would, so every header is in view.
    for (final key in ['fynkel.Transform#0', 'fynkel.RigidBody#0']) {
      await e.click(header(key));
    }

    // Drag the Collider up onto the RigidBody: the line above it, then the Collider in its place.
    final g = await tester.startGesture(top('fynkel.Collider#0'));
    await tester.pump(const Duration(milliseconds: 50));
    await g.moveBy(const Offset(0, -10));
    await tester.pump(const Duration(milliseconds: 50));
    await g.moveTo(top('fynkel.RigidBody#0'));
    await tester.pump(const Duration(milliseconds: 50));
    expect(find.byKey(const ValueKey('card.dropLine.fynkel.RigidBody#0')), findsOneWidget);
    await g.up();
    await tester.pumpAndSettle();
    expect(order(), ['fynkel.Transform', 'fynkel.Collider', 'fynkel.RigidBody', 'fynkel.Layer']);
    expect(
      tester.getTopLeft(header('fynkel.Collider#0')).dy,
      lessThan(tester.getTopLeft(header('fynkel.RigidBody#0')).dy),
    );

    // Down: the Collider onto the RigidBody lands after it.
    await e.drag(header('fynkel.Collider#0'), top('fynkel.RigidBody#0'));
    expect(order(), ['fynkel.Transform', 'fynkel.RigidBody', 'fynkel.Collider', 'fynkel.Layer']);

    // Nothing goes above the Transform, and the Transform does not move.
    await e.drag(header('fynkel.Collider#0'), top('fynkel.Transform#0'));
    await e.drag(header('fynkel.Transform#0'), top('fynkel.Collider#0'));
    expect(order(), ['fynkel.Transform', 'fynkel.RigidBody', 'fynkel.Collider', 'fynkel.Layer']);

    // One ⌘Z per move.
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(order(), ['fynkel.Transform', 'fynkel.Collider', 'fynkel.RigidBody', 'fynkel.Layer']);
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(order(), ['fynkel.Transform', 'fynkel.RigidBody', 'fynkel.Collider', 'fynkel.Layer']);
  });

  testWidgets("a prefab instance's inherited components do not move", (tester) async {
    final e = await Editor.open(tester);
    await e.click(e.row('Crate A'));
    final model = e.shell.inspector!;
    expect(model.slots.where(model.canDrag), isEmpty);
  });
}
