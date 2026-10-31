// Stage 14 GD4: the on-screen stick the guide names, shipped (the lab and
// Ring Ring Deals each wrote their own).

import 'package:flutter/material.dart' hide Transform;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show Input, Vec2;
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() {
  Widget at(Widget stick, {VoidCallback? tapped}) => Directionality(
    textDirection: TextDirection.ltr,
    child: Stack(
      children: [
        Positioned.fill(
          child: GestureDetector(
            onTap: tapped,
            child: const ColoredBox(color: Color(0xFF000000)),
          ),
        ),
        Positioned(left: 0, top: 0, width: 300, height: 300, child: stick),
      ],
    ),
  );

  testWidgets('dragging its knob sets the axis, up is +y, it stops at the rim, and letting go zeroes it', (
    tester,
  ) async {
    final input = Input();
    await tester.pumpWidget(at(Stick(input)));
    // 120 across, knob 52: the rim is 34 from the middle, (60, 60).
    final g = await tester.startGesture(const Offset(60, 60));
    await g.moveBy(const Offset(17, 0));
    await tester.pump();
    expect(input.axis2('move').x, closeTo(0.5, 1e-6));
    expect(input.axis2('move').y, closeTo(0, 1e-6));
    await g.moveBy(const Offset(-17, -200));
    await tester.pump();
    expect(input.axis2('move').x, closeTo(0, 1e-6));
    expect(input.axis2('move').y, closeTo(1, 1e-6), reason: 'up the screen is up the axis, held at the rim');
    await g.up();
    await tester.pump();
    expect(input.axis2('move'), Vec2.zero);
  });

  testWidgets('another axis, by name', (tester) async {
    final input = Input();
    await tester.pumpWidget(at(Stick(input, axis: 'look')));
    final g = await tester.startGesture(const Offset(60, 60));
    await g.moveBy(const Offset(-34, 0));
    await tester.pump();
    expect(input.axis2('look').x, closeTo(-1, 1e-6));
    expect(input.axis2('move'), Vec2.zero);
    await g.up();
  });

  testWidgets('floating: it appears where the thumb comes down, anywhere in its area, and a tap goes through', (
    tester,
  ) async {
    final input = Input();
    var taps = 0;
    await tester.pumpWidget(at(Stick(input, floating: true), tapped: () => taps++));
    expect(find.byKey(const ValueKey('fynkel.stick')), findsNothing, reason: 'hidden until a thumb comes down');
    final g = await tester.startGesture(const Offset(200, 220));
    await g.moveBy(const Offset(0, 25.5));
    await tester.pump();
    expect(input.axis2('move').y, closeTo(-0.75, 1e-6), reason: 'measured from where the thumb came down');
    expect(tester.getCenter(find.byKey(const ValueKey('fynkel.stick'))), const Offset(200, 220));
    await g.up();
    await tester.pump();
    expect(find.byKey(const ValueKey('fynkel.stick')), findsNothing);
    expect(input.axis2('move'), Vec2.zero);
    await tester.tapAt(const Offset(150, 150));
    expect(taps, 1, reason: 'a tap with no drag reaches what is under it');
  });

  testWidgets('taken away mid-drag, it lets go', (tester) async {
    final input = Input();
    await tester.pumpWidget(at(Stick(input)));
    final g = await tester.startGesture(const Offset(60, 60));
    await g.moveBy(const Offset(34, 0));
    await tester.pump();
    expect(input.axis2('move').x, closeTo(1, 1e-6));
    await tester.pumpWidget(at(const SizedBox()));
    expect(input.axis2('move'), Vec2.zero);
    await g.cancel();
  });
}
