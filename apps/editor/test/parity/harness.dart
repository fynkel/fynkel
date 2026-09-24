// The Unity parity harness: the whole editor shell over the real
// `SessionProject` on a copy of a project, driven the way a person drives
// it — clicks on rows, shortcut keys, typing — and checked by what they
// would look at: the Hierarchy's rows, the Inspector, the saved file.
// (docs/unity-parity.md lists what these tests hold the editor to.)
import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';

import '../project_test.dart' show copyFixture;
import '../support/fakes.dart';

const mainScene = 'c0000000-0000-4000-8000-000000000030';

final class Editor {
  Editor._(this.tester, this.dir, this.shell);

  /// Opens the whole shell over a copy of the fixture project, with
  /// [scene] open in a tab; [prepare] changes the copy first.
  static Future<Editor> open(
    WidgetTester tester, {
    String tag = 'parity',
    String scene = mainScene,
    void Function(Directory dir)? prepare,
    String? codeReloadStatus,
  }) async {
    final dir = copyFixture(tag);
    prepare?.call(dir);
    final project = (await tester.runAsync(() => SessionProject.open(dir.path, watch: false)))!;
    addTearDown(() {
      project.close();
      dir.deleteSync(recursive: true);
    });
    await pumpLarge(
      tester,
      shellApp(
        prefs: Preferences.inMemory(),
        projects: {dir.path: project},
        initialProject: dir.path,
        codeReloadStatus: codeReloadStatus,
      ),
    );
    final shell = tester.state<EditorShellState>(find.byType(EditorShell));
    shell.tabs!.openDocument(scene);
    await tester.pumpAndSettle();
    return Editor._(tester, dir, shell);
  }

  final WidgetTester tester;
  final Directory dir;
  final EditorShellState shell;

  /// The Hierarchy's rows top to bottom, as the names a person reads.
  List<String> get rows => [
    for (final t in tester.widgetList<Text>(
      find.descendant(
        of: find.byKey(const Key('hierarchy.list')),
        matching: find.byWidgetPredicate(
          (w) =>
              w is Text &&
              w.key is ValueKey<String> &&
              (w.key! as ValueKey<String>).value.startsWith('hierarchy.name.'),
        ),
      ),
    ))
      t.data ?? '',
  ];

  /// The selected objects' names, in selection order.
  List<String> get selectedNames {
    final tab = shell.tabs!.focused!;
    return [for (final id in tab.selection) tab.tree[id]?.name ?? '?'];
  }

  Finder row(String name) =>
      find.descendant(of: find.byKey(const Key('hierarchy.list')), matching: find.text(name)).first;

  /// The whole row [name] is drawn in (its drop zones are quarters of it).
  Rect rowRect(String name) => tester.getRect(
    find
        .ancestor(
          of: row(name),
          matching: find.byWidgetPredicate(
            (w) => w.key is ValueKey<String> && (w.key! as ValueKey<String>).value.startsWith('row.'),
          ),
        )
        .first,
  );

  /// Just inside the top edge of row [name]: "insert above it".
  Offset above(String name) {
    final r = rowRect(name);
    return Offset(r.center.dx, r.top + 2);
  }

  Future<void> click(Finder f, {bool command = false, bool shift = false}) async {
    if (command) await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
    if (shift) await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await tester.tap(f);
    if (shift) await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    if (command) await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
    await tester.pumpAndSettle();
  }

  /// Presses [key] with the given modifiers, as a shortcut.
  Future<void> press(LogicalKeyboardKey key, {bool command = false, bool shift = false, bool alt = false}) async {
    final mods = [
      if (command) LogicalKeyboardKey.metaLeft,
      if (shift) LogicalKeyboardKey.shiftLeft,
      if (alt) LogicalKeyboardKey.altLeft,
    ];
    for (final m in mods) {
      await tester.sendKeyDownEvent(m);
    }
    await tester.sendKeyEvent(key);
    for (final m in mods.reversed) {
      await tester.sendKeyUpEvent(m);
    }
    await tester.pumpAndSettle();
  }

  /// Types into whatever has focus, as a person would.
  Future<void> type(String text) async {
    tester.testTextInput.enterText(text);
    await tester.pumpAndSettle();
  }

  /// Presses Return in the focused text field.
  Future<void> submit() async {
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
  }

  /// Drags [from] onto [to] with a mouse, pausing so drag targets see it.
  Future<void> drag(Finder from, Offset to) async {
    final g = await tester.startGesture(tester.getCenter(from));
    await tester.pump(const Duration(milliseconds: 50));
    await g.moveBy(const Offset(20, 0));
    await tester.pump(const Duration(milliseconds: 50));
    await g.moveTo(to);
    await tester.pump(const Duration(milliseconds: 50));
    await g.up();
    await tester.pumpAndSettle();
  }

  /// The saved scene file, as JSON.
  Map<String, dynamic> savedScene([String relative = 'assets/scenes/main.scene.json']) =>
      jsonDecode(File('${dir.path}/$relative').readAsStringSync()) as Map<String, dynamic>;
}
