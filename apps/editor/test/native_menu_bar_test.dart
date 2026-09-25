// The native menu bar is sent to the system only when what it shows
// changes: sending it again closes an open menu, and during Play the shell
// rebuilds many times a second (the Hierarchy and Inspector follow the
// running world), so a menu opened then would close at once.
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';

import 'support/fakes.dart';

void main() {
  testWidgets('rebuilding the shell does not send the menus again; a change to what they show does', (tester) async {
    var sent = 0;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.menu, (call) async {
      if (call.method == 'Menu.setMenus') sent++;
      return null;
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.menu, null));

    await pumpLarge(
      tester,
      shellApp(
        prefs: Preferences.inMemory(),
        projects: {'/fake/game': FakeProject(dir: '/fake/game')},
        initialProject: '/fake/game',
        nativeMenuBar: true,
      ),
    );
    final shell = tester.state<EditorShellState>(find.byType(EditorShell));
    expect(sent, greaterThan(0));

    final before = sent;
    for (var i = 0; i < 10; i++) {
      shell.registry.refresh();
      // ignore: invalid_use_of_protected_member
      shell.setState(() {});
      await tester.pump(const Duration(milliseconds: 100));
    }
    expect(sent, before, reason: 'nothing the menus show changed');

    // A label that changes (Window › Hide/Show Console) is sent.
    shell.registry.execute('window.panel.console');
    await tester.pumpAndSettle();
    expect(sent, greaterThan(before));
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));
}
