// Editor extensions (ADR-0028): the packages' editor parts the host passes
// in, called into a registry the shell reads. Every call into a package is
// wrapped — a throwing `registerEditor` skips that package, a throwing
// command is a Console error — and the registry is built again after every
// hot reload, so a save in `lib/editor/` shows at once.
import 'package:flutter/widgets.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart' show Diagnostic, Severity;
import 'package:fynkel/fynkel.dart' show BehaviourRegistry, DataAsset;
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import '../commands/commands.dart';
import '../documents/open_document.dart';
import '../inspector/inspector_model.dart' show readEntities;
import '../preview/preview_provider.dart';
import 'editor_access.dart';

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

/// A window a package registered (Unity's `EditorWindow`).
@immutable
class ExtensionWindow {
  const ExtensionWindow({
    required this.package,
    required this.id,
    required this.title,
    required this.menu,
    required this.label,
    required this.build,
    this.submenu,
    this.icon,
  });

  final String package;
  final String id;
  final String title;

  /// Where the command that shows it goes.
  final EditorMenu menu;
  final String? submenu;
  final String label;
  final IconData? icon;
  final Widget Function(BuildContext context, EditorContext editor) build;

  /// Its panel's id in the layout.
  String get panelId => panelIdOf(id);

  /// The command that shows it.
  String get commandId => 'window.panel.$panelId';

  static const panelPrefix = 'ext:';
  static String panelIdOf(String id) => '$panelPrefix$id';
}

/// A custom Inspector a package registered (Unity's `CustomEditor`): the
/// card of a script or the page of a data type.
class ExtensionInspector {
  ExtensionInspector({required this.package, required this.dartType, required this.build});

  final String package;

  /// The class as registered (`inspector<Collectible>`).
  final Type dartType;
  final Widget Function(BuildContext context, InspectorContext inspector) build;

  /// The script's or data type's id, once the game's registry named it.
  String? typeId;
}

/// A property drawer a package registered: for a record class, or by
/// the name `@Expose(drawer:)` gives.
@immutable
class ExtensionDrawer {
  const ExtensionDrawer({required this.package, required this.build, this.record, this.name});

  final String package;

  /// The record class's name (`drawer<Wave>`), or null.
  final String? record;

  /// The drawer's name (`namedDrawer('minMax', …)`), or null.
  final String? name;
  final Widget Function(BuildContext context, FieldContext field) build;

  /// How the Console names it.
  String get title => record != null ? 'drawer<$record>' : 'drawer "$name"';
}

/// Where a context entry is offered.
enum ContextMenuPlace { hierarchy, project, component }

/// A context-menu entry a package registered: the Hierarchy's, the
/// Project panel's or a component card's ⋮.
@immutable
class ExtensionContextItem {
  const ExtensionContextItem({
    required this.package,
    required this.place,
    required this.id,
    required this.label,
    required this.run,
    this.enabled,
    this.component,
    this.kind,
    this.dataType,
  });

  final String package;
  final ContextMenuPlace place;
  final String id;
  final String label;

  /// The objects' or the assets' (a `List<EditorObject>` or a
  /// `List<EditorAsset>`, by [place]).
  final void Function(EditorContext context, List<Object> targets) run;
  final bool Function(EditorContext context, List<Object> targets)? enabled;

  /// Hierarchy: only when every object has one; component: the card's type.
  final String? component;

  /// Project: only when every asset is of this kind / data type.
  final String? kind;
  final String? dataType;
}

/// One line a panel adds to its context menu.
@immutable
class ContextEntry {
  const ContextEntry({required this.key, required this.label, required this.enabled, required this.run});

  final Key key;
  final String label;
  final bool enabled;
  final VoidCallback run;
}

/// The shell's [EditorExtensionHost] for the panels below it (their
/// context menus add the extensions' entries).
class EditorExtensionScope extends InheritedWidget {
  const EditorExtensionScope({super.key, required this.host, required super.child});

  final EditorExtensionHost host;

  static EditorExtensionHost? maybeOf(BuildContext context) =>
      context.getInheritedWidgetOfExactType<EditorExtensionScope>()?.host;

  @override
  bool updateShouldNotify(EditorExtensionScope old) => !identical(host, old.host);
}

/// What one package registered, or why it registered nothing.
class PackageExtensions {
  PackageExtensions(this.package);

  final String package;
  final menuItems = <ExtensionMenuItem>[];
  final windows = <ExtensionWindow>[];
  final contextItems = <ExtensionContextItem>[];
  final inspectors = <ExtensionInspector>[];
  final drawers = <ExtensionDrawer>[];
  final previews = <PreviewProvider>[];

  /// `registerEditor` threw: nothing of the package's is used.
  String? error;

  bool get isEmpty =>
      menuItems.isEmpty &&
      windows.isEmpty &&
      contextItems.isEmpty &&
      inspectors.isEmpty &&
      drawers.isEmpty &&
      previews.isEmpty;

  void clear() {
    menuItems.clear();
    windows.clear();
    contextItems.clear();
    inspectors.clear();
    drawers.clear();
    previews.clear();
  }
}

/// The editor's side of the packages' editor parts: calls each
/// `registerEditor` ([rebuild]), keeps what they registered and their
/// Console lines ([log]).
class EditorExtensionHost extends ChangeNotifier {
  EditorExtensionHost(this.parts, {this.access});

  /// In the order their `registerEditor` runs (the host's order:
  /// dependencies first, the game last).
  final List<EditorPart> parts;

  /// What a context reads and changes: the shell's documents, project and
  /// Project panel. Null without a shell (a context then sees no scene
  /// and no assets).
  EditorAccess? access;

  /// What each package registered, in [parts] order, after the last
  /// [rebuild].
  List<PackageExtensions> get packages => List.unmodifiable(_packages);
  var _packages = <PackageExtensions>[];

  /// Every menu item that was accepted.
  Iterable<ExtensionMenuItem> get menuItems => _packages.expand((p) => p.menuItems);

  /// Every window that was accepted.
  Iterable<ExtensionWindow> get windows => _packages.expand((p) => p.windows);

  /// The window registered as [id] now; null when none is (its package
  /// failed, or the last reload removed it).
  ExtensionWindow? window(String id) => windows.where((w) => w.id == id).firstOrNull;

  /// The custom Inspector of the script or data type [typeId]; null when
  /// none is registered (the default one draws it).
  ExtensionInspector? inspectorFor(String typeId) => _inspectors[typeId];
  var _inspectors = <String, ExtensionInspector>{};

  /// The drawer for a field naming [named] (`@Expose(drawer:)`), else for
  /// the record class [record]; null: the default editor draws it.
  ExtensionDrawer? drawerFor({String? named, String? record}) =>
      (named == null ? null : _drawers['name:$named']) ?? (record == null ? null : _drawers['record:$record']);
  var _drawers = <String, ExtensionDrawer>{};

  /// Every preview the packages registered, in registration order (the
  /// Inspector asks them before the editor's own).
  Iterable<PreviewProvider> get previews => _packages.expand((p) => p.previews);

  Iterable<ExtensionContextItem> _contextItems(ContextMenuPlace place) =>
      _packages.expand((p) => p.contextItems).where((i) => i.place == place);

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
    final windowIds = <String>{};
    final contextIds = <String>{};
    final inspectors = <String, ExtensionInspector>{};
    final drawers = <String, ExtensionDrawer>{};
    Map<Type, String>? typeIds;
    for (final part in parts) {
      final registered = PackageExtensions(part.package);
      final editor = _Registrar(this, registered);
      try {
        part.register(editor);
      } catch (e, stack) {
        registered
          ..clear()
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
      registered.windows.removeWhere((w) {
        final why = !windowIds.add(w.id)
            ? 'another extension already registered a window "${w.id}"'
            : (idTaken?.call(w.commandId) ?? false)
            ? 'the editor already has a command "${w.commandId}"'
            : null;
        if (why == null) return false;
        _error(part.package, 'window "${w.title}" left out: $why.');
        return true;
      });
      registered.contextItems.removeWhere((item) {
        if (contextIds.add('${item.place.name}/${item.id}')) return false;
        _error(
          part.package,
          'context entry "${item.label}" left out: another extension already registered "${item.id}".',
        );
        return true;
      });
      registered.inspectors.removeWhere((i) {
        final id = (typeIds ??= _typeIds())[i.dartType];
        final why = id == null
            ? '${i.dartType} is not a script or data type the game registers'
            : inspectors.containsKey(id)
            ? 'another extension already registered an Inspector for $id'
            : null;
        if (why != null) {
          _error(part.package, 'inspector<${i.dartType}> left out: $why.');
          return true;
        }
        inspectors[id!] = i..typeId = id;
        return false;
      });
      registered.drawers.removeWhere((d) {
        final key = d.record != null ? 'record:${d.record}' : 'name:${d.name}';
        if (drawers.containsKey(key)) {
          _error(part.package, '${d.title} left out: another extension already registered it.');
          return true;
        }
        drawers[key] = d;
        return false;
      });
      out.add(registered);
    }
    _packages = out;
    _inspectors = inspectors;
    _drawers = drawers;
    _inspectorErrors.clear();
    _drawerErrors.clear();
    _previewErrors.clear();
    notifyListeners();
  }

  /// The context an extension's callbacks get: its scene is [tab] when
  /// given (a panel's own document), else the one in front.
  EditorContext contextFor(String package, {OpenDocument? tab}) => _Context(this, package, tab);

  /// The Hierarchy's lines for [tab]'s selection: the entries whose
  /// component every selected object has.
  List<ContextEntry> hierarchyEntries(OpenDocument tab) {
    if (tab.selection.isEmpty) return const [];
    final out = <ContextEntry>[];
    List<EditorObject>? objects;
    for (final item in _contextItems(ContextMenuPlace.hierarchy)) {
      final context = contextFor(item.package, tab: tab);
      objects ??= context.selection;
      final targets = objects;
      final c = item.component;
      if (c != null && !targets.every((o) => o.has(c))) continue;
      out.add(_entry(item, context, targets));
    }
    return out;
  }

  /// The Project panel's lines for the assets [ids]: the entries whose
  /// kind and data type every one of them is.
  List<ContextEntry> projectEntries(List<String> ids) {
    final out = <ContextEntry>[];
    for (final item in _contextItems(ContextMenuPlace.project)) {
      final context = contextFor(item.package);
      final targets = [
        for (final a in context.assets.all)
          if (ids.contains(a.id)) a,
      ];
      if (targets.isEmpty) continue;
      if (item.kind case final k? when !targets.every((a) => a.kind == k)) continue;
      if (item.dataType case final d? when !targets.every((a) => a.dataType == d)) continue;
      out.add(_entry(item, context, targets));
    }
    return out;
  }

  /// A [type] card's ⋮ lines, for the objects [ids] of [tab].
  List<ContextEntry> componentEntries(OpenDocument tab, String type, List<String> ids) {
    final out = <ContextEntry>[];
    List<EditorObject>? objects;
    for (final item in _contextItems(ContextMenuPlace.component)) {
      if (item.component != type) continue;
      final context = contextFor(item.package, tab: tab);
      objects ??= [for (final id in ids) ?context.scene?.byId(id)];
      out.add(_entry(item, context, objects));
    }
    return out;
  }

  ContextEntry _entry(ExtensionContextItem item, EditorContext context, List<Object> targets) {
    var enabled = true;
    if (item.enabled case final check?) {
      try {
        enabled = check(context, targets);
      } catch (_) {
        enabled = false;
      }
    }
    return ContextEntry(
      key: Key('extension.${item.place.name}.${item.id}'),
      label: item.label,
      enabled: enabled,
      run: () {
        try {
          item.run(context, targets);
        } catch (e, stack) {
          _error(item.package, '"${item.label}" failed: $e', stack);
        }
      },
    );
  }

  /// The game's scripts and data types by class: made once each (never
  /// run) to learn it, as `BehaviourRegistry.dataTypeOf` does.
  Map<Type, String> _typeIds() {
    final register = access?.registerBehaviours;
    if (register == null) return const {};
    final registry = BehaviourRegistry();
    try {
      register(registry);
    } catch (_) {
      return const {};
    }
    final out = <Type, String>{};
    for (final (id, create) in [
      for (final t in registry.types) (t.id, t.create),
      for (final t in registry.dataTypes) (t.id, t.create),
    ]) {
      try {
        out[create().runtimeType] = id;
      } catch (_) {
        // A constructor that throws: no custom Inspector can name it.
      }
    }
    return out;
  }

  /// A custom Inspector's build failed: the Console says so once per
  /// distinct error.
  void inspectorFailed(ExtensionInspector i, Object error, StackTrace stack) {
    final line = 'the ${i.typeId} Inspector failed, so the default one is shown: $error';
    if (_inspectorErrors[i.typeId!] == line) return;
    _inspectorErrors[i.typeId!] = line;
    _error(i.package, line, stack);
  }

  final _inspectorErrors = <String, String>{};

  /// A property drawer's build failed: the Console says so once per
  /// distinct error.
  void drawerFailed(ExtensionDrawer d, Object error, StackTrace stack) {
    final line = '${d.title} failed, so the field is drawn the default way: $error';
    if (_drawerErrors[d.title] == line) return;
    _drawerErrors[d.title] = line;
    _error(d.package, line, stack);
  }

  final _drawerErrors = <String, String>{};

  /// A window's build failed: the Console says so once per distinct error.
  void windowFailed(ExtensionWindow w, Object error, StackTrace stack) {
    final line = 'window "${w.title}" failed: $error';
    if (_windowErrors[w.id] == line) return;
    _windowErrors[w.id] = line;
    _error(w.package, line, stack);
  }

  final _windowErrors = <String, String>{};

  /// A preview's builder failed: the Console says so once per distinct
  /// error.
  void previewFailed(PreviewProvider provider, Object error, StackTrace stack) {
    final line = 'the ${provider.title} preview failed: $error';
    if (_previewErrors[provider.title] == line) return;
    _previewErrors[provider.title] = line;
    _error(provider.package, line, stack);
  }

  final _previewErrors = <String, String>{};

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

  /// Public for the window view: an error line attributed to [package].
  void error(String package, String message, [StackTrace? stack]) => _error(package, message, stack);

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
      _ when _badId(id) => 'its id "$id" must be one dotted word (game.snapToGrid)',
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

  @override
  void window(
    String id, {
    required String title,
    String? menu,
    IconData? icon,
    required Widget Function(BuildContext context, EditorContext editor) build,
  }) {
    final path = menu ?? 'Window/$title';
    final placed = placeMenuPath(path);
    final why = switch (placed) {
      (null, final String why) => why,
      _ when _badId(id) => 'its id "$id" must be one dotted word (game.levelTools)',
      _ when title.trim().isEmpty => 'it needs a title',
      _ => null,
    };
    if (why != null) {
      host._error(package, 'window "$title" left out: $why.');
      return;
    }
    final (m, submenu, label) = placed.$1!;
    registered.windows.add(
      ExtensionWindow(
        package: package,
        id: id,
        title: title,
        menu: m,
        submenu: submenu,
        label: label,
        icon: icon,
        build: build,
      ),
    );
  }

  @override
  void inspector<T>(Widget Function(BuildContext context, InspectorContext inspector) build) {
    if (T == dynamic || T == Object) {
      host._error(package, 'an inspector left out: name its class, inspector<Collectible>(…).');
      return;
    }
    registered.inspectors.add(ExtensionInspector(package: package, dartType: T, build: build));
  }

  @override
  void drawer<T>(Widget Function(BuildContext context, FieldContext field) build) {
    if (T == dynamic || T == Object) {
      host._error(package, 'a drawer left out: name its record class, drawer<Wave>(…).');
      return;
    }
    registered.drawers.add(ExtensionDrawer(package: package, record: '$T', build: build));
  }

  @override
  void namedDrawer(String name, Widget Function(BuildContext context, FieldContext field) build) {
    if (!RegExp(r'^[A-Za-z_][A-Za-z0-9_.]*$').hasMatch(name)) {
      host._error(package, 'drawer "$name" left out: its name must be one word, as @Expose(drawer:) gives it.');
      return;
    }
    registered.drawers.add(ExtensionDrawer(package: package, name: name, build: build));
  }

  bool _contextOk(String label, String id) {
    final why = label.trim().isEmpty
        ? 'it needs a label'
        : _badId(id)
        ? 'its id "$id" must be one dotted word (game.makeBigCoin)'
        : null;
    if (why != null) host._error(package, 'context entry "$label" left out: $why.');
    return why == null;
  }

  @override
  void preview({
    String? kind,
    String? dataType,
    bool Function(EditorAsset asset)? accepts,
    required Widget Function(BuildContext context, PreviewContext preview) build,
    Widget Function(BuildContext context, PreviewContext preview)? toolbar,
    String? Function(PreviewContext preview)? info,
  }) {
    if (kind == null && dataType == null && accepts == null) {
      host._error(package, 'a preview left out: say which assets it takes (kind:, dataType: or accepts:).');
      return;
    }
    registered.previews.add(
      PreviewProvider(
        package: package,
        kind: kind,
        dataType: dataType,
        accepts: accepts,
        build: build,
        toolbar: toolbar,
        info: info,
      ),
    );
  }

  @override
  void hierarchyMenuItem(
    String label, {
    required String id,
    String? withComponent,
    required void Function(EditorContext context, List<EditorObject> objects) run,
    bool Function(EditorContext context, List<EditorObject> objects)? enabled,
  }) {
    if (!_contextOk(label, id)) return;
    registered.contextItems.add(
      ExtensionContextItem(
        package: package,
        place: ContextMenuPlace.hierarchy,
        id: id,
        label: label,
        component: withComponent,
        run: (c, t) => run(c, t.cast<EditorObject>()),
        enabled: enabled == null ? null : (c, t) => enabled(c, t.cast<EditorObject>()),
      ),
    );
  }

  @override
  void projectMenuItem(
    String label, {
    required String id,
    String? kind,
    String? dataType,
    required void Function(EditorContext context, List<EditorAsset> assets) run,
    bool Function(EditorContext context, List<EditorAsset> assets)? enabled,
  }) {
    if (!_contextOk(label, id)) return;
    registered.contextItems.add(
      ExtensionContextItem(
        package: package,
        place: ContextMenuPlace.project,
        id: id,
        label: label,
        kind: kind,
        dataType: dataType,
        run: (c, t) => run(c, t.cast<EditorAsset>()),
        enabled: enabled == null ? null : (c, t) => enabled(c, t.cast<EditorAsset>()),
      ),
    );
  }

  @override
  void componentMenuItem(
    String type,
    String label, {
    required String id,
    required void Function(EditorContext context, List<EditorObject> objects) run,
    bool Function(EditorContext context, List<EditorObject> objects)? enabled,
  }) {
    if (!_contextOk(label, id)) return;
    registered.contextItems.add(
      ExtensionContextItem(
        package: package,
        place: ContextMenuPlace.component,
        id: id,
        label: label,
        component: type,
        run: (c, t) => run(c, t.cast<EditorObject>()),
        enabled: enabled == null ? null : (c, t) => enabled(c, t.cast<EditorObject>()),
      ),
    );
  }
}

extension on _Registrar {
  bool _badId(String id) => id.trim().isEmpty || id.contains(RegExp(r'\s'));
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
  _Context(this.host, this.package, this.tab);

  final EditorExtensionHost host;

  /// A panel's own document; null: the one in front.
  final OpenDocument? tab;

  @override
  final String package;

  @override
  void log(String message) => host._add(package, Severity.info, message);

  @override
  void logWarning(String message) => host._add(package, Severity.warning, message);

  @override
  void logError(String message) => host._add(package, Severity.error, message);

  OpenDocument? get _shown => tab ?? host.access?.shown;

  bool get _playing {
    final t = _shown;
    return t != null && identical(t, host.access?.tabs()?.live);
  }

  @override
  EditorScene? get scene {
    final tab = _shown;
    return tab == null ? null : sceneOf(tab, playing: _playing);
  }

  @override
  List<EditorObject> get selection {
    final tab = _shown;
    if (tab == null || tab.selection.isEmpty) return const [];
    final (infos, perEntity) = readEntities(tab, tab.selection.toList());
    return List.unmodifiable([for (final (i, info) in infos.indexed) objectOf(tab, info, perEntity[i])]);
  }

  @override
  void select(Iterable<EditorObject> objects) {
    final tab = _shown;
    if (tab == null) return;
    tab.setSelection([
      for (final o in objects)
        if (tab.tree.contains(o.id)) o.id,
    ]);
  }

  @override
  EditResult edit(String label, void Function(EditorEdit edit) changes) {
    final access = host.access;
    final tab = _shown;
    if (access == null || tab == null) {
      const why = 'no scene or prefab is open';
      logError('"$label" changed nothing: $why');
      return const EditResult(ok: false, error: why);
    }
    return runEdit(access, tab, label, changes, logError: logError);
  }

  @override
  EditorAssets get assets => host.access == null ? const _NoAssets() : assetsOf(host.access!, logError: logError);

  @override
  EditorPlay? get play => host.access?.play();
}

/// No project: nothing to find, nothing created.
final class _NoAssets implements EditorAssets {
  const _NoAssets();

  @override
  List<EditorAsset> get all => const [];

  @override
  List<EditorAsset> ofKind(String kind) => const [];

  @override
  List<EditorAsset> ofDataType(String typeId) => const [];

  @override
  EditorAsset? find(String path) => null;

  @override
  List<EditorAsset> get selected => const [];

  @override
  EditorAsset? createData(String path, DataAsset value) => null;

  @override
  void ping(EditorAsset asset) {}

  @override
  void open(EditorAsset asset) {}
}
