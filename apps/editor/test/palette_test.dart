import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/commands/commands.dart';
import 'package:fynkel_editor/src/commands/palette.dart';

void main() {
  CommandRegistry registry() {
    final r = CommandRegistry();
    var enabled = true;
    r.registerAll([
      EditorCommand(id: 'project.open', baseLabel: 'Open Project…', menu: EditorMenu.project, run: () {}),
      EditorCommand(id: 'project.close', baseLabel: 'Close Project', menu: EditorMenu.project, run: () {}),
      EditorCommand(
        id: 'window.panel.console',
        baseLabel: 'Toggle Console',
        labelBuilder: () => 'Show Console',
        menu: EditorMenu.window,
        shortcut: const SingleActivator(LogicalKeyboardKey.digit5, meta: true, alt: true),
        run: () {},
      ),
      EditorCommand(
        id: 'help.about',
        baseLabel: 'About Fynkel Editor',
        menu: EditorMenu.help,
        enabled: () => enabled,
        run: () => enabled = false,
      ),
    ]);
    return r;
  }

  test('scoring ranks exact, prefix, word start, contains, id, subsequence', () {
    final r = registry();
    final open = r.byId('project.open')!;
    expect(paletteScore(open, 'open project…'), 100);
    expect(paletteScore(open, 'open'), 90);
    expect(paletteScore(open, 'proj'), 80);
    expect(paletteScore(open, 'ject'), 70);
    expect(paletteScore(open, 'project.op'), 60);
    expect(paletteScore(open, 'opj'), 40);
    expect(paletteScore(open, 'xyz'), 0);
    expect(paletteScore(open, ''), 1);
    expect(paletteMatches(r, 'console').map((c) => c.id), ['window.panel.console']);
    expect(paletteMatches(r, '').length, 4);
  });

  testWidgets('type, arrow down, enter runs the highlighted command and closes', (tester) async {
    final r = registry();
    await tester.pumpWidget(
      MaterialApp(
        home: Builder(
          builder: (context) => Center(
            child: ElevatedButton(onPressed: () => showCommandPalette(context, r), child: const Text('open')),
          ),
        ),
      ),
    );
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('palette.search')), findsOneWidget);
    expect(find.text('Show Console'), findsOneWidget); // the live label
    expect(find.text('⌥⌘5'), findsOneWidget);
    expect(find.textContaining('4 of 4 commands'), findsOneWidget);

    await tester.enterText(find.byKey(const Key('palette.search')), 'project');
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('palette.item.project.open')), findsOneWidget);
    expect(find.byKey(const Key('palette.item.project.close')), findsOneWidget);
    expect(find.byKey(const Key('palette.item.help.about')), findsNothing);
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.pumpAndSettle();
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('palette.search')), findsNothing);
    expect(r.executed, ['project.open']);
  });

  testWidgets('a disabled command is shown greyed and does not run on tap', (tester) async {
    final r = registry();
    r.execute('help.about'); // disables itself
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(body: CommandPalette(registry: r)),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('palette.item.help.about')));
    await tester.pumpAndSettle();
    expect(r.executed, ['help.about']);
    expect(find.text('No matching command'), findsNothing);
    await tester.enterText(find.byKey(const Key('palette.search')), 'zzzz');
    await tester.pumpAndSettle();
    expect(find.text('No matching command'), findsOneWidget);
  });
}
