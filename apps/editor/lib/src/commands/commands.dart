// The command registry: every user action has one stable id, one label, one
// menu placement and (unless it is generated, like a recent project) one
// shortcut — so it is reachable three ways: menu, palette, keyboard.
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

/// The menus, in menu-bar order.
enum EditorMenu {
  project('Project'),
  edit('Edit'),
  view('View'),
  play('Play'),
  window('Window'),
  help('Help');

  const EditorMenu(this.title);

  final String title;
}

final class EditorCommand {
  EditorCommand({
    required this.id,
    required this.baseLabel,
    required this.menu,
    required this.run,
    this.shortcut,
    this.group = 0,
    this.order = 0,
    this.enabled,
    this.description,
    this.generated = false,
    this.labelBuilder,
    this.textIntent,
  });

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

  /// Menu items are grouped; a divider separates groups.
  final int group;
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
  const CommandIntent(this.id);

  final String id;
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
    return c.isEnabled || (c.textIntent != null && CommandRegistry.focusIsTextField);
  }

  @override
  Object? invoke(CommandIntent intent) {
    registry.execute(intent.id);
    return null;
  }
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

  /// The `Shortcuts` map for the window: every shortcut → its command intent.
  Map<ShortcutActivator, Intent> get shortcutIntents => {
    for (final c in _commands.values)
      if (c.shortcut != null) c.shortcut!: CommandIntent(c.id),
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
      final s = c.shortcut;
      if (s == null) continue;
      final key = shortcutLabel(s);
      final other = seen[key];
      if (other != null) dups.add('$key: $other and ${c.id}');
      seen[key] = c.id;
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
