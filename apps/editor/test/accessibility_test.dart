// Item 9's accessibility pass, held by tests: every control a screen reader
// can press has a name, and the whole editor works from the keyboard alone
// — Unity's ⌘1… keys move it between panels, Tab walks one panel, the
// palette (⌘K) runs any command.
import 'package:flutter/rendering.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart' show Severity;
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';

import 'support/fakes.dart';

FakeDocument _scene() => FakeDocument(
  path: '/fake/game/assets/scenes/s.scene.json',
  id: 's',
  kind: 'scene',
  name: 'S',
  relativePath: 'scenes/s.scene.json',
  entities: {
    'crate': FakeDocument.entity(
      'Crate',
      position: [4, 0, 0],
      components: {
        'm': {'type': 'fynkel.MeshRenderer', 'version': 1, 'data': {}},
      },
    ),
    'robot': FakeDocument.entity('Robot', children: ['arm']),
    'arm': FakeDocument.entity('Arm', parent: 'robot'),
  },
);

Future<EditorShellState> _open(WidgetTester tester) async {
  final project = FakeProject(
    dir: '/fake/game',
    diagnostics: [diag('FYN-D0001', severity: Severity.warning, message: 'a warning to walk to')],
  );
  final doc = project.addDocument(_scene());
  await pumpLarge(
    tester,
    shellApp(prefs: Preferences.inMemory(), projects: {'/fake/game': project}, initialProject: '/fake/game'),
  );
  final shell = tester.state<EditorShellState>(find.byType(EditorShell));
  shell.tabs!.openDocument(doc.id);
  await tester.pumpAndSettle();
  return shell;
}

/// Pressable nodes with neither a label nor a tooltip. The splitters
/// between panels are the docking package's (resizable with a screen
/// reader's scroll actions, but not labelable from outside it): skipped.
List<SemanticsNode> _unnamed(WidgetTester tester) {
  final out = <SemanticsNode>[];
  void visit(SemanticsNode n) {
    final d = n.getSemanticsData();
    final pressable = d.hasAction(SemanticsAction.tap) || d.hasAction(SemanticsAction.longPress);
    final scrolls = d.hasAction(SemanticsAction.scrollLeft) || d.hasAction(SemanticsAction.scrollUp);
    final splitter = scrolls && (n.rect.width <= 8 || n.rect.height <= 8);
    // A text field is named by its row's label beside it (as Flutter's own
    // labeledTapTargetGuideline allows).
    final field = d.flagsCollection.isTextField;
    if (!n.isMergedIntoParent && pressable && !splitter && !field && d.label.isEmpty && d.tooltip.isEmpty) {
      out.add(n);
    }
    n.visitChildren((c) {
      visit(c);
      return true;
    });
  }

  visit(tester.binding.renderViews.first.owner!.semanticsOwner!.rootSemanticsNode!);
  return out;
}

Future<void> _press(WidgetTester tester, LogicalKeyboardKey key, {bool meta = false, bool shift = false}) async {
  if (meta) await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
  if (shift) await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
  await tester.sendKeyEvent(key);
  if (shift) await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
  if (meta) await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
  await tester.pumpAndSettle();
}

String? _focused() => FocusManager.instance.primaryFocus?.debugLabel;

void main() {
  testWidgets('every control a screen reader can press has a name: a scene open, an entity selected', (tester) async {
    final semantics = tester.ensureSemantics();
    await _open(tester);
    expect(_unnamed(tester), isEmpty);
    await tester.tap(find.text('Crate'));
    await tester.pumpAndSettle();
    expect(_unnamed(tester), isEmpty);
    // The Hierarchy's rows say whether they are selected and expanded.
    final row = find.descendant(of: find.byKey(const ValueKey('row.crate')), matching: find.text('Crate'));
    expect(tester.getSemantics(row), isSemantics(label: 'Crate', isSelected: true, hasTapAction: true));
    semantics.dispose();
  });

  testWidgets('the keyboard alone: ⌘1… into each panel, arrows and Tab inside, the palette for the rest', (
    tester,
  ) async {
    final shell = await _open(tester);
    final ws = shell.workspace!;

    // ⌘4 Hierarchy: its list, not its search field; ↓ selects.
    await _press(tester, LogicalKeyboardKey.digit4, meta: true);
    expect(_focused(), 'hierarchy.list');
    expect(ws.focusedPanel.value, PanelRegistry.hierarchy);
    await _press(tester, LogicalKeyboardKey.arrowDown);
    expect(shell.tabs!.focused!.selection, isNotEmpty);

    // ⌘3 Inspector: its first control; Tab stays in the panel.
    await _press(tester, LogicalKeyboardKey.digit3, meta: true);
    expect(ws.focusedPanel.value, PanelRegistry.inspector);
    await _press(tester, LogicalKeyboardKey.tab);
    expect(ws.focusedPanel.value, PanelRegistry.inspector);

    // ⌘1 Scene, ⌘5 Project, ⇧⌘C Console, ⌘7 Profiler (shown if hidden).
    await _press(tester, LogicalKeyboardKey.digit1, meta: true);
    expect(_focused(), 'scene.view');
    await _press(tester, LogicalKeyboardKey.digit5, meta: true);
    expect(_focused(), 'project.list');
    await _press(tester, LogicalKeyboardKey.keyC, meta: true, shift: true);
    expect(_focused(), 'console.list');
    // No frames yet, so no chart: the panel's first control.
    await _press(tester, LogicalKeyboardKey.digit7, meta: true);
    expect(ws.focusedPanel.value, PanelRegistry.profiler);
    expect(ws.isShown(PanelRegistry.profiler), isTrue);

    // ⌘K, type, ⏎: any command by name.
    await _press(tester, LogicalKeyboardKey.keyK, meta: true);
    await tester.enterText(find.byType(EditableText).last, 'Go to Hierarchy');
    await tester.pumpAndSettle();
    await _press(tester, LogicalKeyboardKey.enter);
    expect(_focused(), 'hierarchy.list');
  });
}
