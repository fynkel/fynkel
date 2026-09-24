// The command registry: every user action has one stable id, one label, one
// menu placement and (unless it is generated, like a recent project) one
// shortcut — so it is reachable three ways: menu, palette, keyboard.
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

import '../l10n/strings.dart';

/// The menus, in menu-bar order.
enum EditorMenu {
  project(EditorStrings.menuProject),
  edit(EditorStrings.menuEdit),
  gameObject(EditorStrings.menuGameObject),
  view(EditorStrings.menuView),
  play(EditorStrings.menuPlay),
  window(EditorStrings.menuWindow),
  help(EditorStrings.menuHelp);

  const EditorMenu(this.title);

  final String title;
}

final class EditorCommand {
  /// [baseLabel] and [description] default to the command's words in
  /// [EditorStrings.commands]; a generated command passes its own, filled
  /// from a template there.
  EditorCommand({
    required this.id,
    String? baseLabel,
    required this.menu,
    required this.run,
    this.shortcut,
    this.key,
    this.group = 0,
    this.order = 0,
    this.enabled,
    String? description,
    this.generated = false,
    this.labelBuilder,
    this.textIntent,
    this.submenu,
  }) : baseLabel = baseLabel ?? EditorStrings.label(id),
       description = description ?? EditorStrings.description(id);

  /// Stable, dotted, never renamed: `project.open`, `window.panel.console`.
  final String id;

  /// The label that never changes — what the palette indexes and the tests name.
  final String baseLabel;

  /// Computes the label now (a toggle reads "Show Console" or "Hide Console").
  final String Function()? labelBuilder;

  /// The label now.
  String get label => labelBuilder?.call() ?? baseLabel;
  final EditorMenu menu;
  final SingleActivator? shortcut;

  /// A bare key (no ⌘) that runs it from any panel, as the Scene view's
  /// F or W do in Unity: after the focused panel's own keys (the
  /// Hierarchy's arrows, the viewport's fly keys) and never while a text
  /// field has focus. Not a native menu key equivalent — the menu bar
  /// would take the letter from every text field before Flutter sees it.
  final SingleActivator? key;

  /// Menu items are grouped; a divider separates groups.
  final int group;

  /// The submenu of [menu] it sits in (GameObject ▸ 3D Object ▸ Cube);
  /// null: directly in the menu. A submenu's items are one group.
  final String? submenu;
  final int order;
  final bool Function()? enabled;
  final String? description;

  /// Generated from state (a recent project) — has no shortcut by design.
  final bool generated;
  final VoidCallback run;

  /// What the shortcut means inside a focused text field (⌘Z there is the
  /// field's own undo, not the document's). The native menu bar takes its
  /// key equivalents before Flutter sees them, so the registry forwards
  /// this intent to the field instead of running the command.
  final Intent? textIntent;

  bool get isEnabled => enabled?.call() ?? true;
}

/// "Run the command with this id" — what the window's `Shortcuts` map to.
final class CommandIntent extends Intent {
  const CommandIntent(this.id, {this.bare = false});

  final String id;

  /// From the command's bare [EditorCommand.key]: typing wins over it.
  final bool bare;
}

/// Runs a [CommandIntent]; disabled commands leave the key to whoever is
/// below (a text field's own bindings).
final class CommandAction extends Action<CommandIntent> {
  CommandAction(this.registry);

  final CommandRegistry registry;

  @override
  bool isEnabled(CommandIntent intent) {
    final c = registry.byId(intent.id);
    if (c == null) return false;
    if (intent.bare && CommandRegistry.focusOwnsBareKeys) return false;
    return c.isEnabled || (c.textIntent != null && CommandRegistry.focusIsTextField);
  }

  @override
  Object? invoke(CommandIntent intent) {
    registry.execute(intent.id);
    return null;
  }
}

/// A region whose keys are its own — the Game view while the game plays:
/// with the focus inside it the editor's bare keys ([EditorCommand.key])
/// do nothing, exactly as while typing. ⌘-shortcuts still reach the editor.
class KeyboardOwner extends InheritedWidget {
  const KeyboardOwner({super.key, required super.child});

  @override
  bool updateShouldNotify(KeyboardOwner oldWidget) => false;
}

final class CommandRegistry extends ChangeNotifier {
  final _commands = <String, EditorCommand>{};
  final _executed = <String>[];

  /// Ids executed so far, oldest first (tests and the palette's recency).
  List<String> get executed => List.unmodifiable(_executed);

  List<EditorCommand> get all {
    final list = _commands.values.toList();
    list.sort((a, b) {
      final m = a.menu.index.compareTo(b.menu.index);
      if (m != 0) return m;
      final g = a.group.compareTo(b.group);
      if (g != 0) return g;
      final o = a.order.compareTo(b.order);
      return o != 0 ? o : a.baseLabel.compareTo(b.baseLabel);
    });
    return list;
  }

  EditorCommand? byId(String id) => _commands[id];

  List<EditorCommand> inMenu(EditorMenu menu) => all.where((c) => c.menu == menu).toList();

  void register(EditorCommand command) {
    _commands[command.id] = command;
    notifyListeners();
  }

  void registerAll(Iterable<EditorCommand> commands) {
    for (final c in commands) {
      _commands[c.id] = c;
    }
    notifyListeners();
  }

  /// Removes every command whose id starts with [prefix] (regenerating recents).
  void unregisterPrefix(String prefix) {
    _commands.removeWhere((id, _) => id.startsWith(prefix));
    notifyListeners();
  }

  /// Runs the command when it exists and is enabled; false otherwise. A
  /// command with a [EditorCommand.textIntent] forwards that intent to the
  /// focused text field instead, when there is one.
  bool execute(String id) {
    final c = _commands[id];
    if (c == null) return false;
    if (c.textIntent != null && focusIsTextField) {
      final ctx = primaryFocus!.context!;
      Actions.maybeInvoke(ctx, c.textIntent!);
      return true;
    }
    if (!c.isEnabled) return false;
    _executed.add(id);
    c.run();
    return true;
  }

  /// Whether keyboard focus is inside an editable text field right now.
  static bool get focusIsTextField {
    final ctx = primaryFocus?.context;
    if (ctx == null) return false;
    return ctx.findAncestorStateOfType<EditableTextState>() != null;
  }

  /// Whether the focus is where bare keys are someone else's: a text
  /// field, or a [KeyboardOwner] (the playing game).
  static bool get focusOwnsBareKeys {
    if (focusIsTextField) return true;
    return primaryFocus?.context?.getInheritedWidgetOfExactType<KeyboardOwner>() != null;
  }

  /// The `Shortcuts` map for the window: every shortcut → its command intent.
  Map<ShortcutActivator, Intent> get shortcutIntents => {
    for (final c in _commands.values) ...{
      if (c.shortcut != null) c.shortcut!: CommandIntent(c.id),
      if (c.key != null) c.key!: CommandIntent(c.id, bare: true),
    },
  };

  /// Something a command depends on changed (a project opened, a panel hidden).
  void refresh() => notifyListeners();

  /// The bindings for a `CallbackShortcuts` above the whole window.
  Map<ShortcutActivator, VoidCallback> get shortcutBindings => {
    for (final c in _commands.values)
      if (c.shortcut != null) c.shortcut!: () => execute(c.id),
  };

  /// Two commands never share a shortcut; the duplicates, if any.
  List<String> get duplicateShortcuts {
    final seen = <String, String>{};
    final dups = <String>[];
    for (final c in _commands.values) {
      for (final s in [c.shortcut, c.key].nonNulls) {
        final key = shortcutLabel(s);
        final other = seen[key];
        if (other != null) dups.add('$key: $other and ${c.id}');
        seen[key] = c.id;
      }
    }
    return dups;
  }

  /// `⇧⌘P`, `⌥⌘5` — macOS symbols in the platform's order (control, option, shift, command).
  static String shortcutLabel(SingleActivator s) {
    final b = StringBuffer();
    if (s.control) b.write('⌃');
    if (s.alt) b.write('⌥');
    if (s.shift) b.write('⇧');
    if (s.meta) b.write('⌘');
    b.write(_keyLabel(s.trigger));
    return b.toString();
  }

  static String _keyLabel(LogicalKeyboardKey key) {
    if (key == LogicalKeyboardKey.comma) return ',';
    if (key == LogicalKeyboardKey.period) return '.';
    if (key == LogicalKeyboardKey.slash) return '/';
    if (key == LogicalKeyboardKey.enter) return '↩';
    if (key == LogicalKeyboardKey.escape) return '⎋';
    if (key == LogicalKeyboardKey.backspace) return '⌫';
    if (key == LogicalKeyboardKey.delete) return '⌦';
    final label = key.keyLabel;
    return label.length == 1 ? label.toUpperCase() : label;
  }
}
