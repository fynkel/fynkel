// Every command is reachable three ways: menu, palette, shortcut.
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/commands/commands.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/shell/menu_bar.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';

import 'support/fakes.dart';

void main() {
  late FakeProject project;
  late Preferences prefs;
  late MemoryLayoutStore store;

  setUp(() {
    project = FakeProject(dir: '/fake/game');
    prefs = Preferences.inMemory();
    store = MemoryLayoutStore();
  });

  Future<EditorShellState> openShell(WidgetTester tester) async {
    await pumpLarge(
      tester,
      shellApp(prefs: prefs, store: store, projects: {'/fake/game': project}, initialProject: '/fake/game'),
    );
    expect(find.byKey(const Key('window.projectName')), findsOneWidget);
    return tester.state<EditorShellState>(find.byType(EditorShell));
  }

  testWidgets('every non-generated command has a menu, a unique shortcut and a stable id', (tester) async {
    final shell = await openShell(tester);
    final registry = shell.registry;
    final commands = registry.all.where((c) => !c.generated).toList();
    expect(commands.length, 100); // 9c: the Asset panel went into the Inspector; 9e: four Window ▸ New <Panel>; 9g: the Transform tool; 9h: Show Gizmos in Play; ADR-0021: Pause, Step, the Game view, Restart to Apply
    for (final c in commands) {
      expect(c.shortcut, isNotNull, reason: '${c.id} has no shortcut');
      expect(c.id, matches(RegExp(r'^[a-z]+(\.[a-zA-Z]+)+$')), reason: '${c.id} is not a dotted id');
      expect(find.byKey(Key('menuitem.${c.id}'), skipOffstage: false), findsNothing); // menus closed
    }
    expect(registry.duplicateShortcuts, isEmpty);
    final ids = commands.map((c) => c.id).toSet();
    expect(
      ids,
      containsAll([
        'project.new',
        'project.open',
        'project.close',
        'project.revalidate',
        'project.migrations',
        'project.reveal',
        'app.quit',
        'app.preferences',
        'window.commandPalette',
        'window.resetLayout',
        'window.panel.console',
        'window.panel.scene',
        'console.focusSearch',
        'console.copy',
        'console.openInIde',
        'console.openInEditor',
        'document.open',
        'document.save',
        'document.saveAs',
        'document.saveAll',
        'document.revert',
        'document.close',
        'document.next',
        'document.previous',
        'document.undo',
        'document.redo',
        'hierarchy.rename',
        'hierarchy.duplicate',
        'hierarchy.delete',
        'hierarchy.createEmpty',
        'hierarchy.createWith',
        'hierarchy.keepWorldTransform',
        'hierarchy.search',
        'hierarchy.expandAll',
        'hierarchy.collapseAll',
        'scene.tool.select',
        'scene.tool.move',
        'scene.tool.rotate',
        'scene.tool.scale',
        'scene.toggleSpace',
        'scene.toggleSnap',
        'scene.toggleHidden',
        'scene.showAll',
        'scene.toggleLocked',
        'scene.alignCameraToView',
        'view.frameSelected',
        'view.frameAll',
        'view.toggleProjection',
        'view.alignToCamera',
        'view.reset',
        'view.toggleGrid',
        'view.toggleOverlays',
        'view.toggleStats',
        'view.reload',
        'view.recook',
        'view.saveFrame',
        'play.start',
        'play.stop',
        'play.showConsole',
        'play.clearConsole',
        'help.about',
        'help.documentation',
      ]),
    );
    expect(CommandRegistry.shortcutLabel(registry.byId('view.reload')!.shortcut!), '⌃⌘R');
    expect(CommandRegistry.shortcutLabel(registry.byId('view.recook')!.shortcut!), '⌃⇧⌘R');
    expect(CommandRegistry.shortcutLabel(registry.byId('view.frameSelected')!.shortcut!), '⌘F');
    expect(CommandRegistry.shortcutLabel(registry.byId('window.commandPalette')!.shortcut!), '⇧⌘P');
    expect(CommandRegistry.shortcutLabel(registry.byId('window.panel.console')!.shortcut!), '⌥⌘5');
    expect(CommandRegistry.shortcutLabel(registry.byId('document.save')!.shortcut!), '⌘S');
    expect(CommandRegistry.shortcutLabel(registry.byId('document.redo')!.shortcut!), '⇧⌘Z');
    expect(CommandRegistry.shortcutLabel(registry.byId('hierarchy.delete')!.shortcut!), '⌘⌫');
  });

  testWidgets('menus show every command of their menu, in groups, with the shortcut label', (tester) async {
    final shell = await openShell(tester);
    for (final menu in EditorMenu.values) {
      await tester.tap(find.byKey(Key('menu.${menu.name}')));
      await tester.pumpAndSettle();
      for (final c in shell.registry.inMenu(menu)) {
        expect(find.byKey(Key('menuitem.${c.id}')), findsOneWidget, reason: '${c.id} missing from ${menu.title}');
      }
      final palette = shell.registry.byId('window.commandPalette')!;
      if (menu == EditorMenu.window) expect(find.text(CommandRegistry.shortcutLabel(palette.shortcut!)), findsWidgets);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
    }
  });

  testWidgets('a menu item, the palette and the shortcut all run the same command', (tester) async {
    final shell = await openShell(tester);
    expect(shell.workspace!.isVisible(PanelRegistry.console), isTrue);

    // Menu.
    await tester.tap(find.byKey(const Key('menu.window')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('menuitem.window.panel.console')));
    await tester.pumpAndSettle();
    expect(shell.workspace!.isVisible(PanelRegistry.console), isFalse);
    expect(shell.registry.executed.last, 'window.panel.console');

    // Shortcut ⌥⌘5.
    await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.altLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.digit5);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.altLeft);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
    await tester.pumpAndSettle();
    expect(shell.workspace!.isVisible(PanelRegistry.console), isTrue);

    // Palette ⇧⌘P → type → Enter.
    await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyP);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('palette.search')), findsOneWidget);
    await tester.enterText(find.byKey(const Key('palette.search')), 'hide console');
    await tester.pumpAndSettle();
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('palette.search')), findsNothing);
    expect(shell.workspace!.isVisible(PanelRegistry.console), isFalse);
    expect(shell.registry.executed.where((id) => id == 'window.panel.console').length, 3);
  });

  testWidgets('the native macOS menus serialise with and without a project', (tester) async {
    // The in-window MenuBar covers the other tests; the native path moves About,
    // Preferences and Quit into the app menu, which once left the Edit menu with
    // an empty group — an assertion at startup on macOS. Since item 3 the Edit
    // menu has Undo/Redo and the Hierarchy commands, so it stays.
    for (final withProject in [false, true]) {
      await pumpLarge(
        tester,
        shellApp(
          key: UniqueKey(),
          prefs: prefs,
          store: store,
          projects: {'/fake/game': project},
          initialProject: withProject ? '/fake/game' : null,
        ),
      );
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      final menus = EditorMenuBar(registry: shell.registry, child: const SizedBox()).platformMenus();
      expect(menus.first.label, 'Fynkel Editor');
      expect(menus.map((m) => m.label), containsAll(['Project', 'Edit', 'Window', 'Help']));
      // Serialising is what asserts on an empty group; the wire form names every item.
      final wire = [
        for (final menu in menus) ...[
          if (menu.menus.isEmpty) fail('${menu.label} is empty'),
          ...menu.toChannelRepresentation(DefaultPlatformMenuDelegate(), getId: (_) => 1).map(jsonEncode),
        ],
      ].join();
      for (final label in [
        'Open Project…',
        'Preferences…',
        'Command Palette…',
        'About Fynkel Editor',
        'Undo',
        'Save Document',
      ]) {
        expect(wire, contains(label));
      }
    }
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS)); // the platform-provided items exist on macOS only

  testWidgets('project-scoped commands are disabled without a project and quit is injectable', (tester) async {
    var quits = 0;
    await pumpLarge(tester, shellApp(prefs: prefs, store: store, quit: () => quits++));
    final shell = tester.state<EditorShellState>(find.byType(EditorShell));
    expect(shell.registry.byId('project.close')!.isEnabled, isFalse);
    expect(shell.registry.byId('window.panel.console')!.isEnabled, isFalse);
    expect(shell.registry.byId('project.open')!.isEnabled, isTrue);
    expect(shell.registry.execute('project.close'), isFalse);
    expect(shell.registry.execute('app.quit'), isTrue);
    expect(quits, 1);
  });

  testWidgets('recent projects become generated commands without shortcuts', (tester) async {
    prefs.addRecent('/fake/game', 'Fake Game');
    prefs.addRecent('/fake/other', 'Other');
    await pumpLarge(tester, shellApp(prefs: prefs, store: store));
    final shell = tester.state<EditorShellState>(find.byType(EditorShell));
    final recents = shell.registry.all.where((c) => c.id.startsWith('project.recent.')).toList();
    expect(recents.length, 2);
    expect(recents.every((c) => c.generated && c.shortcut == null), isTrue);
    expect(recents.first.baseLabel, 'Open Recent: Other');
    prefs.clearRecents();
    await tester.pump();
    expect(shell.registry.all.where((c) => c.id.startsWith('project.recent.')), isEmpty);
  });
}
