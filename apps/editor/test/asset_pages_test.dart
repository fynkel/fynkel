// The asset pages as Unity draws them (docs/ui-refresh): `name (Kind)`,
// Open and a ⋮ menu in the header, a prefab's root object, and the file's
// facts and uses folded under Info with each use named by its object.
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/inspector/inspector_chrome.dart';

import 'parity/harness.dart';

void main() {
  testWidgets('a prefab shows its root; Info names the objects that use a material', (tester) async {
    InspectorSection.setOpen('Info', null);
    addTearDown(() => InspectorSection.setOpen('Info', null));
    final e = await Editor.open(tester, tag: 'assetpages');
    final browser = e.shell.browser!;
    String id(String path) => browser.entries.firstWhere((a) => a.path == path).id;

    browser.select(id('prefabs/coin.prefab.json'));
    await tester.pumpAndSettle();
    expect(find.text('coin (Prefab)'), findsOneWidget);
    expect(find.byKey(const Key('asset.open')), findsOneWidget);
    expect(tester.widget<Text>(find.byKey(const Key('asset.prefabRoot.name'))).data, 'Coin');
    expect(find.byKey(const ValueKey('asset.prefabRoot.fynkel.Transform')), findsOneWidget);
    expect(find.byKey(const ValueKey('asset.prefabRoot.fynkel.Collider')), findsOneWidget);

    browser.select(id('scenes/main.scene.json'));
    await tester.pumpAndSettle();
    expect(find.text('main (Scene)'), findsOneWidget);
    expect(find.byKey(const Key('asset.prefabRoot')), findsNothing);

    browser.select(id('materials/rusted.material.json'));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('asset.open')), findsNothing);
    expect(find.textContaining('Used by'), findsNothing, reason: 'folded under Info');
    await tester.ensureVisible(find.byKey(const Key('section.Info')));
    await tester.tap(find.byKey(const Key('section.Info')));
    await tester.pumpAndSettle();
    expect(find.textContaining('Used by · '), findsOneWidget);
    // The use in main.scene.json is the Robot's renderer, not an id.
    final robot = find.byWidgetPredicate(
      (w) => w is RichText && w.text.toPlainText() == 'Robot  ·  MeshRenderer',
    );
    expect(robot, findsOneWidget);
  });
}
