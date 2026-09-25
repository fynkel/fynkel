// The pickups package (ADR-0029 step 4): its assets are what
// tool/assets.dart writes from lib/src/pickup_spec.dart, with ids of the
// package's own; its editor part's gem preview.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/authoring.dart';
import 'package:pickups/editor/editor.dart' show GemPreview;
import 'package:pickups/generated/assets.g.dart';

import '../tool/assets.dart';

void main() {
  test('the documents in assets/ are what tool/assets.dart writes', () {
    final written = writeAssets(Directory.current.path);
    expect(written.map((d) => d.path), [
      'materials/gem.material.json',
      'data/gem.data.json',
      'prefabs/pickup.prefab.json',
    ]);
    expect(
      [
        for (final d in written)
          if (d.changed) d.path,
      ],
      isEmpty,
      reason: 'a spec changed: run `dart run tool/assets.dart` and commit the documents',
    );
    expect(written.map((d) => d.ref), [Assets.materials.gem, Assets.data.gem, Assets.prefabs.pickup]);
  });

  test("the ids are the package's own: a game's document at the same path gets another", () {
    final game = Directory.systemTemp.createTempSync('pickups_game');
    addTearDown(() => game.deleteSync(recursive: true));
    File('${game.path}/fynkel.project.json').writeAsStringSync('{"assets": "assets"}');
    final ours = ProjectWriter(game.path).writePrefab('prefabs/pickup', ObjectSpec('Pickup'));
    expect(ours.path, 'prefabs/pickup.prefab.json');
    expect(ours.ref, isNot(Assets.prefabs.pickup));
  });

  testWidgets('the gem preview says what a kind gives', (tester) async {
    await tester.pumpWidget(
      const MaterialApp(
        home: Scaffold(body: GemPreview(color: [0.3, 0.9, 0.8], points: 25)),
      ),
    );
    expect(find.text('Gives 25 points'), findsOneWidget);
    expect(find.byKey(const Key('gemPreview.gem')), findsOneWidget);
  });
}
