// Fast open: an editor started on older code is covered until the hot
// restart that brings it up to date, so nothing done in the old code is
// lost to that restart (it lost a handover from the launcher once).

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/play/updating_code.dart';

void main() {
  late Directory dir;
  late File file;

  setUp(() {
    dir = Directory.systemTemp.createTempSync('fynkel_updating');
    file = File('${dir.path}/fynkel_updating');
  });
  tearDown(() => dir.deleteSync(recursive: true));

  Widget app({Duration timeout = const Duration(seconds: 30)}) => UpdatingCodeBarrier(
    path: file.path,
    timeout: timeout,
    child: MaterialApp(
      home: Scaffold(body: TextButton(onPressed: () => taps++, child: const Text('Open'))),
    ),
  );

  testWidgets('the old code is covered and takes no clicks until the file goes', (tester) async {
    taps = 0;
    file.writeAsStringSync('stale');
    await tester.pumpWidget(app());
    expect(find.byKey(const Key('updatingCode')), findsOneWidget);
    await tester.tap(find.text('Open'), warnIfMissed: false);
    expect(taps, 0);

    file.writeAsStringSync('restarting');
    await tester.pump(const Duration(milliseconds: 200));
    expect(find.byKey(const Key('updatingCode')), findsOneWidget, reason: 'still the old code');

    file.deleteSync();
    await tester.pump(const Duration(milliseconds: 200));
    expect(find.byKey(const Key('updatingCode')), findsNothing);
    await tester.tap(find.text('Open'));
    expect(taps, 1);
  });

  testWidgets('the restarted code, and an editor not started stale, show nothing', (tester) async {
    file.writeAsStringSync('restarting');
    await tester.pumpWidget(app());
    expect(find.byKey(const Key('updatingCode')), findsNothing);

    file.deleteSync();
    await tester.pumpWidget(const SizedBox());
    await tester.pumpWidget(app());
    expect(find.byKey(const Key('updatingCode')), findsNothing);
  });

  testWidgets('a file left behind uncovers after the timeout', (tester) async {
    file.writeAsStringSync('stale');
    await tester.pumpWidget(app(timeout: const Duration(seconds: 1)));
    expect(find.byKey(const Key('updatingCode')), findsOneWidget);
    await tester.pump(const Duration(milliseconds: 1200));
    expect(find.byKey(const Key('updatingCode')), findsNothing);
  });
}

var taps = 0;
