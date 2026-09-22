// Item 9a's promises that a test can hold: the keyboard focus ring, notices
// in the status bar instead of snackbars, and nothing that opens with an
// animation.
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/theme/focus_ring.dart';
import 'package:fynkel_editor/src/theme/theme.dart';

import 'support/fakes.dart';

Widget _ringApp(Widget home) => MaterialApp(
  theme: fynkelTheme(brightness: Brightness.dark),
  builder: (context, child) => FocusRingLayer(child: child!),
  home: Scaffold(body: home),
);

Rect? _ring(WidgetTester tester) => tester.state<FocusRingLayerState>(find.byType(FocusRingLayer)).ring;

void main() {
  group('focus ring', () {
    Widget buttons() => Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        TextButton(key: const Key('a'), onPressed: () {}, child: const Text('A')),
        TextButton(key: const Key('b'), onPressed: () {}, child: const Text('B')),
        const SizedBox(width: 200, child: TextField(key: Key('field'))),
        IconButton(key: const Key('c'), onPressed: () {}, icon: const Icon(Icons.add)),
      ],
    );

    testWidgets('Tab puts the ring on what it focuses, exactly around it', (tester) async {
      await tester.pumpWidget(_ringApp(Center(child: buttons())));
      expect(_ring(tester), isNull);
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      expect(_ring(tester), tester.getRect(find.byKey(const Key('a'))));
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      expect(_ring(tester), tester.getRect(find.byKey(const Key('b'))));
    });

    testWidgets('a click shows no ring, and a key pressed where the click put focus does not either', (tester) async {
      await tester.pumpWidget(_ringApp(Center(child: buttons())));
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      expect(_ring(tester), isNotNull);
      // The mouse takes over: the ring goes at once.
      await tester.tap(find.byKey(const Key('b')));
      await tester.pump();
      expect(_ring(tester), isNull);
      // A key that does not move focus (W flies the Scene view) keeps it hidden.
      await tester.sendKeyEvent(LogicalKeyboardKey.keyW);
      await tester.pump();
      expect(_ring(tester), isNull);
      // Moving focus by keyboard from there brings it back.
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      expect(_ring(tester), isNotNull);
    });

    testWidgets('a text field is left to its own accent border', (tester) async {
      await tester.pumpWidget(_ringApp(Center(child: buttons())));
      for (var i = 0; i < 3; i++) {
        await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      }
      await tester.pump();
      expect(FocusManager.instance.primaryFocus?.context?.findAncestorWidgetOfExactType<TextField>(), isNotNull);
      expect(_ring(tester), isNull);
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      expect(_ring(tester), tester.getRect(find.byKey(const Key('c'))));
    });

    testWidgets('the ring is clipped to the scroll view and follows a scroll', (tester) async {
      final controller = ScrollController();
      addTearDown(controller.dispose);
      await tester.pumpWidget(
        _ringApp(
          Align(
            alignment: Alignment.topLeft,
            child: SizedBox(
              key: const Key('viewport'),
              width: 200,
              height: 100,
              child: ListView(
                controller: controller,
                children: [
                  for (var i = 0; i < 10; i++)
                    SizedBox(
                      height: 40,
                      child: TextButton(key: Key('row$i'), onPressed: () {}, child: Text('Row $i')),
                    ),
                ],
              ),
            ),
          ),
        ),
      );
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      await tester.pumpAndSettle(); // traversal scrolls row 2 into view
      final viewport = tester.getRect(find.byKey(const Key('viewport')));
      final row2 = tester.getRect(find.byKey(const Key('row2')));
      expect(_ring(tester), row2.intersect(viewport));
      // Scroll so row 2 is half out of the top: the ring is its visible half.
      controller.jumpTo(controller.offset + (row2.top - viewport.top) + 20);
      await tester.pump();
      await tester.pump();
      final moved = tester.getRect(find.byKey(const Key('row2')));
      expect(moved.top, lessThan(viewport.top));
      expect(_ring(tester), moved.intersect(viewport));
      expect(_ring(tester)!.height, closeTo(20, 0.01));
    });
  });

  group('notices', () {
    late Preferences prefs;
    setUp(() => prefs = Preferences.inMemory());

    testWidgets('a failed open is said in the launcher strip until dismissed; opening clears it', (tester) async {
      final project = FakeProject(dir: '/fake/game');
      await pumpLarge(tester, shellApp(prefs: prefs, projects: {'/fake/game': project}));
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      expect(find.byKey(const Key('notice')), findsNothing);
      await shell.openProjectDir('/fake/missing');
      await tester.pump();
      expect(
        find.descendant(of: find.byKey(const Key('shell.error')), matching: find.textContaining('Could not open')),
        findsOneWidget,
      );
      // Still there long after a snackbar would have gone.
      await tester.pump(const Duration(seconds: 30));
      expect(find.byKey(const Key('notice')), findsOneWidget);
      await tester.tap(find.byKey(const Key('notice.dismiss')));
      await tester.pump();
      expect(find.byKey(const Key('notice')), findsNothing);
      // An error, then a project that opens: the error was about the other one.
      await shell.openProjectDir('/fake/missing');
      await tester.pump();
      expect(find.byKey(const Key('notice')), findsOneWidget);
      await shell.openProjectDir('/fake/game');
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('notice')), findsNothing);
    });

    testWidgets('in the main window the notice takes the status line, and a new one replaces it', (tester) async {
      final project = FakeProject(dir: '/fake/game');
      await pumpLarge(tester, shellApp(prefs: prefs, projects: {'/fake/game': project}, initialProject: '/fake/game'));
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      final status = find.byKey(const Key('window.status'));
      expect(find.descendant(of: status, matching: find.textContaining('documents ·')), findsOneWidget);
      shell.notices.show('Reveal in Finder failed: no Finder');
      await tester.pump();
      expect(find.descendant(of: status, matching: find.text('Reveal in Finder failed: no Finder')), findsOneWidget);
      expect(find.descendant(of: status, matching: find.textContaining('documents ·')), findsNothing);
      shell.notices.show('Reimport refused: locked');
      await tester.pump();
      expect(find.textContaining('Reveal in Finder'), findsNothing);
      expect(find.descendant(of: status, matching: find.text('Reimport refused: locked')), findsOneWidget);
      await tester.tap(find.byKey(const Key('notice.dismiss')));
      await tester.pump();
      expect(find.descendant(of: status, matching: find.textContaining('documents ·')), findsOneWidget);
    });
  });

  test('nothing in the editor opens with an animation or pops a snackbar', () {
    // Every dialog and menu is opened with `AnimationStyle.noAnimation`, the
    // About box included (showAboutDialog takes no style, so it is not used);
    // messages go through notices.dart.
    final opener = RegExp(r'\b(showDialog|showMenu|PopupMenuButton|showGeneralDialog)\s*(<[^(]*>)?\(');
    final offenders = <String>[];
    for (final f in Directory(
      'lib',
    ).listSync(recursive: true).whereType<File>().where((f) => f.path.endsWith('.dart'))) {
      final src = f.readAsStringSync();
      for (final m in opener.allMatches(src)) {
        // The call's own arguments: up to the matching close paren.
        var depth = 0, i = m.end - 1;
        for (; i < src.length; i++) {
          if (src[i] == '(') depth++;
          if (src[i] == ')' && --depth == 0) break;
        }
        final args = src.substring(m.end, i);
        if (!args.contains('AnimationStyle.noAnimation')) {
          offenders.add('${f.path}:${'\n'.allMatches(src.substring(0, m.start)).length + 1} ${m.group(1)}');
        }
      }
      for (final banned in ['SnackBar(', 'showSnackBar', 'showAboutDialog', 'showModalBottomSheet']) {
        if (src.contains(banned)) offenders.add('${f.path}: $banned');
      }
    }
    expect(offenders, isEmpty);
  });
}
