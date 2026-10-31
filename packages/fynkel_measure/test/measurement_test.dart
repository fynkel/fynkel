// Stage 14's GT11: of fynkel_measure only the on-screen clock was tested.
// What a measurement reads and says, on the Mac the tests run on.

import 'dart:ffi' show AllocatorAlloc, Uint8, Uint8Pointer;
import 'dart:io';

import 'package:ffi/ffi.dart';
import 'package:flutter/material.dart' hide Transform;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_flutter/testing.dart';
import 'package:fynkel_measure/fynkel_measure.dart';

void main() {
  test('the footprint is the process, and grows by memory it touches', () {
    final before = footprint();
    expect(before.total, greaterThan(10 << 20), reason: 'a Dart test process is tens of MB');
    expect(before.graphics, greaterThanOrEqualTo(0));
    expect(physicalFootprint(), closeTo(before.total, 64 << 20));
    final block = malloc<Uint8>(64 << 20);
    try {
      block.asTypedList(64 << 20).fillRange(0, 64 << 20, 1);
      expect(footprint().total - before.total, greaterThan(48 << 20));
    } finally {
      malloc.free(block);
    }
  });

  test('the thermal state is one of the four Apple says, on a Mac', () {
    if (!Platform.isMacOS) return markTestSkipped('a Mac only');
    expect(thermalState(), isIn([0, 1, 2, 3]));
  });

  testWidgets("a second's stats line says the view's numbers, Flutter's frames and the memory", (tester) async {
    final measurement = Measurement();
    addTearDown(measurement.dispose);
    await tester.pumpWidget(MaterialApp(home: Stack(children: [StatsLine(measurement)])));
    measurement.onStats(
      const FynkelViewStats(
        window: Duration(seconds: 1),
        ticks: 60,
        view: ViewStats(posted: 60, rendered: 60, coalesced: 0, presented: 60),
        size: Size(800, 600),
      ),
    );
    await tester.pump();
    final line = measurement.line.value;
    for (final field in ['ticks=60.0', 'flutter=', 'slow=', 'budget=', 'footprint=', 'mouse=']) {
      expect(line, contains(field));
    }
    expect(int.parse(RegExp(r'footprint=(\d+)').firstMatch(line)!.group(1)!), greaterThan(10 << 20));
    expect(find.text(line), findsOneWidget);
  });

  testWidgets('a measured app shows the game, with the stats line over it', (tester) async {
    await tester.pumpWidget(MeasuredApp(_Spinner.new, overlay: (root) => const Text('the overlay')));
    await tester.run(0.5);
    expect(tester.game<_Spinner>().turned, greaterThan(0));
    expect(find.text('the overlay'), findsOneWidget);
    expect(find.byType(StatsLine), findsOneWidget);
  });
}

class _Spinner extends GameObject {
  var turned = 0.0;

  @override
  void onUpdate() {
    super.onUpdate();
    turned += time.delta;
  }
}
