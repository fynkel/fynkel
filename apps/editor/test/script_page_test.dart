// A script selected in the Project's Scripts shows in the Inspector (Unity's
// MonoScript page): `player (Behaviour)`, the behaviour's fields, the
// source; a scripts folder shows too — a selection never leaves the
// Inspector on the last asset.
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/shell/editor_selection.dart';

import 'parity/harness.dart';

void main() {
  testWidgets('selecting a script or a scripts folder shows it in the Inspector', (tester) async {
    final e = await Editor.open(tester, tag: 'scriptpage', prepare: writeDemoScripts);
    final browser = e.shell.browser!;
    final selection = e.shell.selection!;

    browser.showScripts('game');
    await tester.pumpAndSettle();
    expect(selection.target, const InspectScript('game', folder: true));
    expect(find.text('game (Folder)'), findsOneWidget);
    expect(find.descendant(of: find.byKey(const Key('script.folder.scripts')), matching: find.text('2')), findsOneWidget);

    browser.selectScript('game/player.dart');
    await tester.pumpAndSettle();
    expect(selection.target, const InspectScript('game/player.dart'));
    expect(find.text('player (Behaviour)'), findsOneWidget);
    expect(find.byKey(const Key('script.behaviour.game.Player')), findsOneWidget);
    expect(find.text('Double = 4'), findsOneWidget);
    expect(find.text('Coin Kind (data asset), or none'), findsOneWidget);
    expect(find.textContaining('class Player extends Behaviour'), findsOneWidget);

    browser.selectScript('game/flow.dart');
    await tester.pumpAndSettle();
    expect(find.text('flow (Script)'), findsOneWidget);
    expect(find.byKey(const Key('script.behaviour.game.Player')), findsNothing);

    // Clearing the script selection empties the page, as for an asset.
    browser.selectScript(null);
    await tester.pumpAndSettle();
    expect(selection.target, isNull);
  });
}
