// Editor extensions (ADR-0028 step 1): a package's `registerEditor` puts
// menu items in the editor, its log lines reach the Console, a failing one
// fails alone, and a hot reload registers everything again.
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart' show Severity;
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/commands/commands.dart';
import 'package:fynkel_editor/src/extensions/editor_extensions.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import 'support/fakes.dart';

// What the fake packages register; a test changes it before a "reload".
var _gameItems = <String>['Tools/Say Hello'];
var _greeting = 'Hello';
var _canTidy = false;
final _ran = <String>[];

void _tools(EditorExtensions editor) {
  editor.menuItem('Tools/Level/Tidy', id: 'tools.tidy', enabled: (_) => _canTidy, run: (_) => _ran.add('tidy'));
  editor.menuItem('Tools/Broken', id: 'tools.broken', run: (_) => throw StateError('no level'));
}

void _game(EditorExtensions editor) {
  for (final path in _gameItems) {
    editor.menuItem(
      path,
      id: 'game.${path.split('/').last.replaceAll(' ', '')}',
      run: (context) {
        _ran.add(path);
        context.log('$_greeting from ${context.package}');
      },
    );
  }
}

/// Selects every coin and renames them, one undo step (ADR-0028 step 2).
void _coins(EditorExtensions editor) {
  editor.menuItem(
    'Tools/Select Coins',
    id: 'coins.select',
    enabled: (context) => context.scene != null,
    run: (context) {
      final scene = context.scene!;
      final coins = [
        for (final o in scene.objects)
          if (o.name.startsWith('Coin')) o,
      ];
      context.select(coins);
      context.edit('Number coins', (edit) {
        for (final (i, c) in context.selection.indexed) {
          edit.rename(c, 'Coin ${i + 1}');
        }
      });
      context.log('${coins.length} coins in ${scene.name}');
    },
  );
}

void _throws(EditorExtensions editor) => throw UnsupportedError('not today');

void _bad(EditorExtensions editor) {
  editor.menuItem('Nowhere/Item', id: 'bad.nowhere', run: (_) {});
  editor.menuItem('Tools', id: 'bad.short', run: (_) {});
  editor.menuItem('Edit/Undo Twice', id: 'document.undo', run: (_) {});
  editor.menuItem(
    'Tools/Save Again',
    id: 'bad.save',
    shortcut: const SingleActivator(LogicalKeyboardKey.keyS, meta: true),
    run: (_) {},
  );
  editor.menuItem('Edit/Spaced', id: 'bad id', run: (_) {});
  editor.menuItem('Edit/Fine', id: 'bad.fine', run: (_) {});
}

void main() {
  late FakeProject project;
  late Preferences prefs;

  setUp(() {
    project = FakeProject(dir: '/fake/game');
    prefs = Preferences.inMemory();
    _gameItems = ['Tools/Say Hello'];
    _greeting = 'Hello';
    _canTidy = false;
    _ran.clear();
  });

  Future<EditorShellState> openShell(WidgetTester tester, List<EditorPart> parts) async {
    await pumpLarge(
      tester,
      shellApp(prefs: prefs, projects: {'/fake/game': project}, initialProject: '/fake/game', extensions: parts),
    );
    return tester.state<EditorShellState>(find.byType(EditorShell));
  }

  List<String> consoleLines(EditorShellState shell) => [
    for (final d in shell.console!.diagnostics)
      if (d.code == EditorExtensionHost.logCode) '${d.severity.name}: ${d.message}',
  ];

  testWidgets('menu items land in their menu, run with their package\'s context and log to the Console', (
    tester,
  ) async {
    final shell = await openShell(tester, const [EditorPart('tools', _tools), EditorPart('my_game', _game)]);
    final tools = shell.registry.inMenu(EditorMenu.tools);
    expect(
      [for (final c in tools) (c.id, c.submenu, c.label)],
      [('tools.tidy', 'Level', 'Tidy'), ('tools.broken', null, 'Broken'), ('game.SayHello', null, 'Say Hello')],
    );

    // Through the menu bar, like any command.
    await tester.tap(find.byKey(const Key('menu.tools')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('menuitem.game.SayHello')));
    await tester.pumpAndSettle();
    expect(_ran, ['Tools/Say Hello']);
    expect(consoleLines(shell), ['info: my_game: Hello from my_game']);

    // `enabled` greys it out until it holds.
    expect(shell.registry.execute('tools.tidy'), isFalse);
    _canTidy = true;
    expect(shell.registry.execute('tools.tidy'), isTrue);
    expect(_ran.last, 'tidy');

    // A throwing command is a Console error, and the editor carries on.
    expect(shell.registry.execute('tools.broken'), isTrue);
    expect(consoleLines(shell).last, startsWith('error: tools: "Tools/Broken" failed: Bad state: no level'));
    expect(shell.registry.execute('game.SayHello'), isTrue);
    expect(consoleLines(shell).last, 'info: my_game: Hello from my_game');
  });

  testWidgets('a context sees the scene in front, selects in it and edits it as one undo step', (tester) async {
    project.addDocument(
      FakeDocument(
        path: '/fake/game/assets/scenes/main.scene.json',
        relativePath: 'scenes/main.scene.json',
        id: 'main',
        name: 'Main',
        entities: {
          'cam': FakeDocument.entity('Camera'),
          'a': FakeDocument.entity('Coin'),
          'b': FakeDocument.entity('Coin'),
        },
      ),
    );
    final shell = await openShell(tester, const [EditorPart('coins', _coins)]);
    expect(shell.registry.execute('coins.select'), isFalse, reason: 'no scene open yet');
    final tab = shell.openDocument('main')!;
    await tester.pumpAndSettle();
    expect(shell.registry.execute('coins.select'), isTrue);
    await tester.pumpAndSettle();
    expect(tab.selection.toList(), ['a', 'b']);
    expect(
      [
        for (final id in ['a', 'b']) tab.tree[id]!.name,
      ],
      ['Coin 1', 'Coin 2'],
    );
    expect(consoleLines(shell), ['info: coins: 2 coins in Main']);
    expect(tab.undo(), isTrue);
    expect(
      [
        for (final id in ['a', 'b']) tab.tree[id]!.name,
      ],
      ['Coin', 'Coin'],
    );
  });

  testWidgets('a hot reload registers every package again', (tester) async {
    final shell = await openShell(tester, const [EditorPart('my_game', _game)]);
    expect(shell.registry.byId('game.SayHello'), isNotNull);

    _gameItems = ['Tools/Say Goodbye'];
    _greeting = 'Goodbye';
    // A hot reload's reassemble; it completes at the next frame.
    final reassembled = tester.binding.reassembleApplication();
    await tester.pump();
    await reassembled;
    await tester.pumpAndSettle();
    expect(shell.registry.byId('game.SayHello'), isNull, reason: 'gone from the code, gone from the menu');
    expect(shell.registry.byId('game.SayGoodbye')!.label, 'Say Goodbye');
    shell.registry.execute('game.SayGoodbye');
    expect(consoleLines(shell), ['info: my_game: Goodbye from my_game']);
  });

  testWidgets('a failing registerEditor skips its package alone; bad items are left out with the reason', (
    tester,
  ) async {
    final shell = await openShell(tester, const [
      EditorPart('broken', _throws),
      EditorPart('bad', _bad),
      EditorPart('my_game', _game),
    ]);
    expect(shell.registry.byId('game.SayHello'), isNotNull);
    expect(shell.registry.byId('bad.fine')!.menu, EditorMenu.edit);
    expect(shell.registry.byId('document.undo')!.baseLabel, 'Undo', reason: 'the editor\'s own command stays');
    for (final id in ['bad.nowhere', 'bad.short', 'bad.save', 'bad id']) {
      expect(shell.registry.byId(id), isNull, reason: id);
    }
    final lines = consoleLines(shell);
    expect(lines.first, startsWith('error: broken: registerEditor failed, so none of its extensions are used'));
    expect(lines.skip(1), [
      startsWith('error: bad: menu item "Nowhere/Item" left out: "Nowhere" is not a menu'),
      'error: bad: menu item "Tools" left out: a path is Menu/Item or Menu/Submenu/Item.',
      'error: bad: menu item "Edit/Spaced" left out: its id "bad id" must be one dotted word (game.snapToGrid).',
      'error: bad: menu item "Edit/Undo Twice" left out: the editor already has a command "document.undo".',
      'error: bad: menu item "Tools/Save Again" left out: its shortcut ⌘S is taken.',
    ]);
    expect(shell.console!.diagnostics.where((d) => d.severity == Severity.error), hasLength(6));

    // Preferences › Extensions says who registered what.
    shell.showPreferences();
    await tester.pumpAndSettle();
    final list = find.byKey(const Key('prefs.extensions'));
    await tester.ensureVisible(list);
    expect(
      find.descendant(
        of: list,
        matching: find.textContaining('registerEditor failed: Unsupported operation: not today'),
      ),
      findsOneWidget,
    );
    expect(find.descendant(of: list, matching: find.text('Menu item: Edit/Fine  (bad.fine)')), findsOneWidget);
    expect(
      find.descendant(of: list, matching: find.text('Menu item: Tools/Say Hello  (game.SayHello)')),
      findsOneWidget,
    );
  });

  testWidgets('without editor parts there is no Tools menu and Preferences says how to add one', (tester) async {
    final shell = await openShell(tester, const []);
    expect(shell.registry.inMenu(EditorMenu.tools), isEmpty);
    expect(find.byKey(const Key('menu.tools')), findsNothing);
    shell.showPreferences();
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('prefs.extensions.none')), findsOneWidget);
  });
}
