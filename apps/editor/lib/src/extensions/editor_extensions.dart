// Editor extensions (ADR-0028): the packages' editor parts the host passes
// in, called into a registry the shell reads. Every call into a package is
// wrapped — a throwing `registerEditor` skips that package, a throwing
// command is a Console error — and the registry is built again after every
// hot reload, so a save in `lib/editor/` shows at once.
import 'package:flutter/widgets.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart' show Diagnostic, Severity;
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import '../commands/commands.dart';

/// One package's editor part, as the generated host passes it:
/// `EditorPart('code_only', editor0.registerEditor)`.
@immutable
class EditorPart {
  const EditorPart(this.package, this.register);

  final String package;
  final void Function(EditorExtensions editor) register;
}

/// A menu item a package registered, placed.
@immutable
class ExtensionMenuItem {
  const ExtensionMenuItem({
    required this.package,
    required this.path,
    required this.id,
    required this.menu,
    required this.label,
    required this.run,
    this.submenu,
    this.enabled,
    this.shortcut,
    this.description,
  });

  final String package;

  /// As registered: `Tools/Level/Ring of Coins`.
  final String path;
  final String id;
  final EditorMenu menu;
  final String? submenu;
  final String label;
  final void Function(EditorContext context) run;
  final bool Function(EditorContext context)? enabled;
  final SingleActivator? shortcut;
  final String? description;
}

/// What one package registered, or why it registered nothing.
class PackageExtensions {
  PackageExtensions(this.package);

  final String package;
  final menuItems = <ExtensionMenuItem>[];

  /// `registerEditor` threw: nothing of the package's is used.
  String? error;

  bool get isEmpty => menuItems.isEmpty;
}

/// The editor's side of the packages' editor parts: calls each
/// `registerEditor` ([rebuild]), keeps what they registered and their
/// Console lines ([log]).
class EditorExtensionHost extends ChangeNotifier {
  EditorExtensionHost(this.parts);

  /// In the order their `registerEditor` runs (the host's order:
  /// dependencies first, the game last).
  final List<EditorPart> parts;

  /// What each package registered, in [parts] order, after the last
  /// [rebuild].
  List<PackageExtensions> get packages => List.unmodifiable(_packages);
  var _packages = <PackageExtensions>[];

  /// Every menu item that was accepted.
  Iterable<ExtensionMenuItem> get menuItems => _packages.expand((p) => p.menuItems);

  /// The extensions' Console lines, oldest first (the last [logLimit]).
  List<Diagnostic> get log => List.unmodifiable(_log);
  final _log = <Diagnostic>[];
  static const logLimit = 500;

  /// The Console code of an extension's lines.
  static const logCode = 'extension';

  /// Calls every part's `registerEditor` again, from nothing. [idTaken]
  /// and [shortcutTaken] are the editor's own commands, which an
  /// extension cannot take.
  void rebuild({bool Function(String id)? idTaken, bool Function(SingleActivator shortcut)? shortcutTaken}) {
    final out = <PackageExtensions>[];
    final ids = <String>{};
    final keys = <String>{};
    for (final part in parts) {
      final registered = PackageExtensions(part.package);
      final editor = _Registrar(this, registered);
      try {
        part.register(editor);
      } catch (e, stack) {
        registered
          ..menuItems.clear()
          ..error = '$e';
        _error(part.package, 'registerEditor failed, so none of its extensions are used: $e', stack);
        out.add(registered);
        continue;
      }
      registered.menuItems.removeWhere((item) {
        String? why;
        if (idTaken?.call(item.id) ?? false) {
          why = 'the editor already has a command "${item.id}"';
        } else if (!ids.add(item.id)) {
          why = 'another extension already registered "${item.id}"';
        } else if (item.shortcut case final s?) {
          final label = CommandRegistry.shortcutLabel(s);
          if ((shortcutTaken?.call(s) ?? false) || !keys.add(label)) {
            why = 'its shortcut $label is taken';
          }
        }
        if (why == null) return false;
        _error(part.package, 'menu item "${item.path}" left out: $why.');
        return true;
      });
      out.add(registered);
    }
    _packages = out;
    notifyListeners();
  }

  /// The context an extension's callbacks get.
  EditorContext contextFor(String package) => _Context(this, package);

  /// Runs [item] with its package's context; a throw is a Console error.
  void run(ExtensionMenuItem item) {
    try {
      item.run(contextFor(item.package));
    } catch (e, stack) {
      _error(item.package, '"${item.path}" failed: $e', stack);
    }
  }

  /// Whether [item] is enabled now; a throwing check disables it.
  bool isEnabled(ExtensionMenuItem item) {
    final check = item.enabled;
    if (check == null) return true;
    try {
      return check(contextFor(item.package));
    } catch (_) {
      return false;
    }
  }

  void _add(String package, Severity severity, String message) {
    _log.add(Diagnostic(code: logCode, severity: severity, message: '$package: $message'));
    if (_log.length > logLimit) _log.removeRange(0, _log.length - logLimit);
    notifyListeners();
  }

  void _error(String package, String message, [StackTrace? stack]) {
    final at = stack == null ? '' : _firstFrame(stack);
    _add(package, Severity.error, at.isEmpty ? message : '$message ($at)');
  }

  /// The first frame of [stack] in a package (where it went wrong).
  static String _firstFrame(StackTrace stack) {
    for (final line in stack.toString().split('\n')) {
      final m = RegExp(r'\((package:[^)]+)\)').firstMatch(line);
      if (m != null) return m.group(1)!;
    }
    return '';
  }

  void clearLog() {
    _log.clear();
    notifyListeners();
  }
}

/// A package's `EditorExtensions`: checks what it is given and records it.
class _Registrar implements EditorExtensions {
  _Registrar(this.host, this.registered);

  final EditorExtensionHost host;
  final PackageExtensions registered;

  @override
  String get package => registered.package;

  @override
  void menuItem(
    String path, {
    required String id,
    required void Function(EditorContext context) run,
    bool Function(EditorContext context)? enabled,
    SingleActivator? shortcut,
    String? description,
  }) {
    final placed = placeMenuPath(path);
    final why = switch (placed) {
      (null, final String why) => why,
      _ when id.trim().isEmpty || id.contains(RegExp(r'\s')) =>
        'its id "$id" must be one dotted word (game.snapToGrid)',
      _ => null,
    };
    if (why != null) {
      host._error(package, 'menu item "$path" left out: $why.');
      return;
    }
    final (menu, submenu, label) = placed.$1!;
    registered.menuItems.add(
      ExtensionMenuItem(
        package: package,
        path: path,
        id: id,
        menu: menu,
        submenu: submenu,
        label: label,
        run: run,
        enabled: enabled,
        shortcut: shortcut,
        description: description,
      ),
    );
  }
}

/// `Tools/Snap to Grid` → (Tools, no submenu, "Snap to Grid");
/// `Tools/Level/Ring` → (Tools, "Level", "Ring"); anything else → why not.
((EditorMenu, String?, String)?, String?) placeMenuPath(String path) {
  final parts = path.split('/').map((s) => s.trim()).toList();
  if (parts.length < 2 || parts.length > 3 || parts.any((s) => s.isEmpty)) {
    return (null, 'a path is Menu/Item or Menu/Submenu/Item');
  }
  final menu = EditorMenu.values.where((m) => m.title.toLowerCase() == parts.first.toLowerCase()).firstOrNull;
  if (menu == null) {
    final names = EditorMenu.values.map((m) => m.title).join(', ');
    return (null, '"${parts.first}" is not a menu ($names)');
  }
  return ((menu, parts.length == 3 ? parts[1] : null, parts.last), null);
}

class _Context implements EditorContext {
  _Context(this.host, this.package);

  final EditorExtensionHost host;

  @override
  final String package;

  @override
  void log(String message) => host._add(package, Severity.info, message);

  @override
  void logWarning(String message) => host._add(package, Severity.warning, message);

  @override
  void logError(String message) => host._add(package, Severity.error, message);
}
