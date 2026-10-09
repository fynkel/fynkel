// Opens and closes every scene again and again, on a device, with the real
// plugin: each page makes a world and a view, and frees them as it goes.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:lab/main.dart';
import 'package:lab/scenes.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('every scene opens and closes, again and again', (tester) async {
    await tester.pumpWidget(const LabApp());
    final titles = [for (final make in scenes) make().title];
    for (var round = 0; round < 5; round++) {
      for (final title in titles) {
        await tester.tap(find.text(title));
        // Real frames, so the view registers and draws.
        for (var i = 0; i < 40; i++) {
          await tester.pump(const Duration(milliseconds: 16));
        }
        await tester.pageBack();
        await tester.pumpAndSettle();
        debugPrint('round $round: $title');
      }
    }
  });
}
