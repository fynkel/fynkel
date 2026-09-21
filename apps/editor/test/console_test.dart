import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/console/console_panel.dart';

import 'support/fakes.dart';

void main() {
  late FakeProject project;
  late ConsoleModel model;
  final opened = <(String, int?, int?)>[];

  setUp(() {
    opened.clear();
    project = FakeProject(
      dir: '/g',
      diagnostics: [
        diag(
          'FYN-D0206',
          message: 'unknown field "speeed"',
          document: '/g/assets/scenes/main.scene.json',
          pointer: '/entities/e1/components/c2/data/speeed',
          line: 75,
          column: 13,
          suggestion: 'remove it or check the spelling',
        ),
        diag(
          'FYN-D0206',
          message: 'unknown field "speeed"',
          document: '/g/assets/scenes/main.scene.json',
          pointer: '/x',
          line: 75,
          column: 13,
        ),
        diag(
          'FYN-D0206',
          message: 'unknown field "speeed"',
          document: '/g/assets/scenes/main.scene.json',
          pointer: '/x',
          line: 75,
          column: 13,
        ),
        diag('FYN-D0301', severity: Severity.warning, message: 'unused asset', document: '/g/assets/textures/a.png'),
        diag('FYN-D0900', severity: Severity.info, message: 'metadata is stale', pointer: '/meta'),
      ],
    );
    model = ConsoleModel(
      project: project,
      openInIde: (file, {line, column}) async {
        opened.add((file, line, column));
        return null;
      },
    );
  });

  test('entries collapse repeats, filter by severity and search, and print positions', () {
    expect(model.entries.length, 4);
    final repeated = model.entries.firstWhere((e) => e.diagnostic.pointer == '/x');
    expect(repeated.count, 2);
    expect(model.locationOf(model.entries.first.diagnostic), 'assets/scenes/main.scene.json:75:13');
    expect(model.locationOf(project.diagnostics[3]), 'assets/textures/a.png');
    expect(model.locationOf(project.diagnostics[4]), '/meta');
    expect(
      model.lineOf(model.entries.first),
      'error FYN-D0206: unknown field "speeed" (assets/scenes/main.scene.json:75:13) — remove it or check the spelling',
    );
    expect(model.lineOf(repeated), endsWith('(×2)'));

    model.setCollapse(false);
    expect(model.entries.length, 5);
    model.setCollapse(true);

    model.toggleSeverity(Severity.error);
    expect(model.entries.map((e) => e.diagnostic.code), ['FYN-D0301', 'FYN-D0900']);
    model.toggleSeverity(Severity.error);

    model.setQuery('D0301');
    expect(model.entries.single.diagnostic.code, 'FYN-D0301');
    model.setQuery('scenes speeed');
    expect(model.entries.length, 2);
    model.setQuery('textures/a');
    expect(model.entries.single.diagnostic.code, 'FYN-D0301');
    model.setQuery('');
    expect(model.countOf(Severity.error), 3);
    expect(model.countOf(Severity.warning), 1);
  });

  testWidgets('rows show, a click selects, double-click and ↩ open the IDE at line:column, copy copies', (
    tester,
  ) async {
    String? clipboard;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData') clipboard = (call.arguments as Map)['text'] as String;
      return null;
    });
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(body: ConsolePanel(model: model)),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('unknown field "speeed"'), findsNWidgets(2));
    expect(find.text('assets/scenes/main.scene.json:75:13'), findsNWidgets(2));
    expect(find.text('×2'), findsOneWidget);
    expect(find.textContaining('3 errors, 1 warnings'), findsOneWidget);
    expect(find.byKey(const Key('console.detail')), findsNothing);

    final first = find.text('remove it or check the spelling');
    expect(first, findsNothing); // the suggestion shows in the detail pane only
    final row = find.byKey(Key('console.row.${ConsoleEntry.keyOf(project.diagnostics.first)}'));
    await tester.tap(row);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('console.detail')), findsOneWidget);
    expect(find.textContaining('Suggestion: remove it'), findsOneWidget);

    // Double-click a different row (the ×2 one, same file and position).
    final repeated = find.byKey(Key('console.row.${ConsoleEntry.keyOf(project.diagnostics[1])}'));
    await tester.tap(repeated);
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(repeated);
    await tester.pumpAndSettle();
    expect(opened, [('/g/assets/scenes/main.scene.json', 75, 13)]);

    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(opened.length, 2);

    await tester.tap(find.byKey(const Key('console.copy')));
    await tester.pumpAndSettle();
    expect(clipboard, startsWith('error FYN-D0206: unknown field "speeed" (assets/scenes/main.scene.json:75:13)'));
    expect(clipboard!.split('\n').length, 1);

    model.select(null);
    await tester.tap(find.byKey(const Key('console.copy')));
    await tester.pumpAndSettle();
    expect(clipboard!.split('\n').length, 4);

    await tester.tap(find.byKey(const Key('console.filter.error')));
    await tester.pumpAndSettle();
    expect(find.text('unknown field "speeed"'), findsNothing);
    expect(find.text('unused asset'), findsOneWidget);

    await tester.enterText(find.byKey(const Key('console.search')), 'nothing-like-this');
    await tester.pumpAndSettle();
    expect(find.text('Nothing matches the filter.'), findsOneWidget);

    await tester.tap(find.byKey(const Key('console.revalidate')));
    await tester.pumpAndSettle();
    expect(project.revalidations, 1);
    expect(find.textContaining('validated 12:30:15'), findsOneWidget);
  });

  testWidgets('an IDE failure shows in the detail pane', (tester) async {
    model = ConsoleModel(project: project, openInIde: (file, {line, column}) async => '`zed` was not found on PATH');
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(body: ConsolePanel(model: model)),
      ),
    );
    await tester.pumpAndSettle();
    model.select(ConsoleEntry.keyOf(project.diagnostics.first));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('console.open')));
    await tester.pumpAndSettle();
    expect(find.text('`zed` was not found on PATH'), findsOneWidget);
  });

  testWidgets('a clean project says so', (tester) async {
    project.diagnostics = [];
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(body: ConsolePanel(model: model)),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('No diagnostics. The project validates clean.'), findsOneWidget);
    expect(find.textContaining('0 errors, 0 warnings'), findsOneWidget);
  });
}
