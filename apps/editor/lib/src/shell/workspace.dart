// The docking workspace: the panel registry, the §22.2 default layout, show /
// hide / focus of panels, and the per-user layout file with its "Reset
// layout" fallback. Findings from the spike (ADR-0016): every panel is a
// keepAlive item; the package's string carries no selected tabs, so they are
// stored beside it.
import 'dart:convert';
import 'dart:io';

import 'package:docking/docking.dart';
import 'package:flutter/material.dart';

/// Where a panel goes when it is shown and its group is gone.
enum PanelHome { left, center, right, bottom }

final class PanelSpec {
  const PanelSpec({
    required this.id,
    required this.title,
    required this.icon,
    required this.home,
    required this.builder,
  });

  /// Stable id: the layout file and the `window.panel.<id>` command use it.
  final String id;
  final String title;
  final IconData icon;
  final PanelHome home;
  final WidgetBuilder builder;
}

final class PanelRegistry {
  PanelRegistry(Iterable<PanelSpec> specs) : _specs = {for (final s in specs) s.id: s};

  final Map<String, PanelSpec> _specs;

  List<PanelSpec> get all => _specs.values.toList();
  PanelSpec? byId(String id) => _specs[id];
  Iterable<String> get ids => _specs.keys;

  /// The §22.2 panel set and its default placement.
  static const hierarchy = 'hierarchy';
  static const scene = 'scene';
  static const inspector = 'inspector';
  static const project = 'project';
  static const console = 'console';
  static const profiler = 'profiler';
  static const animation = 'animation';
  static const import = 'import';
  static const defaultOrder = [hierarchy, scene, inspector, project, console, profiler, animation, import];
}

/// What is saved: the package's layout string, its format tag, and the
/// selected item of every tab group.
final class LayoutSnapshot {
  const LayoutSnapshot({required this.layout, required this.selected, this.dockingFormat = format});

  static const format = 'V1';

  final String layout;
  final List<String> selected;
  final String dockingFormat;

  Map<String, Object?> toJson() => {'dockingFormat': dockingFormat, 'layout': layout, 'selected': selected};

  static LayoutSnapshot? fromJson(Object? json) {
    if (json is! Map || json['layout'] is! String) return null;
    return LayoutSnapshot(
      layout: json['layout'] as String,
      selected: [
        for (final s in json['selected'] as List? ?? const [])
          if (s is String) s,
      ],
      dockingFormat: json['dockingFormat'] as String? ?? '',
    );
  }
}

abstract class LayoutStore {
  LayoutSnapshot? read();
  void write(LayoutSnapshot snapshot);
  void clear();
}

final class MemoryLayoutStore implements LayoutStore {
  LayoutSnapshot? snapshot;

  @override
  LayoutSnapshot? read() => snapshot;
  @override
  void write(LayoutSnapshot s) => snapshot = s;
  @override
  void clear() => snapshot = null;
}

/// `layout.json` next to the preferences; atomic replacement.
final class FileLayoutStore implements LayoutStore {
  FileLayoutStore(this.file);

  final File file;

  @override
  LayoutSnapshot? read() {
    try {
      if (!file.existsSync()) return null;
      return LayoutSnapshot.fromJson(jsonDecode(file.readAsStringSync()));
    } catch (_) {
      return null;
    }
  }

  @override
  void write(LayoutSnapshot s) {
    try {
      file.parent.createSync(recursive: true);
      final tmp = File('${file.path}.tmp');
      tmp.writeAsStringSync('${const JsonEncoder.withIndent('  ').convert(s.toJson())}\n');
      tmp.renameSync(file.path);
    } catch (_) {
      // A layout that cannot be saved is not worth interrupting the user for.
    }
  }

  @override
  void clear() {
    try {
      if (file.existsSync()) file.deleteSync();
    } catch (_) {}
  }
}

class _PanelParser extends LayoutParser with LayoutParserMixin {
  const _PanelParser();
}

class _PanelBuilder with AreaBuilderMixin {
  _PanelBuilder(this.workspace);

  final Workspace workspace;

  @override
  DockingItem buildDockingItem({required dynamic id, required double? weight, required bool maximized}) {
    final spec = workspace.registry.byId(id as String);
    if (spec == null) throw StateError('unknown panel "$id"');
    return workspace._item(spec, weight: weight, maximized: maximized);
  }
}

/// The layout controller the main window and the Window menu share.
final class Workspace extends ChangeNotifier {
  Workspace({required this.registry, required this.store}) {
    if (!_restore()) _applyDefault();
  }

  final PanelRegistry registry;
  final LayoutStore store;
  late DockingLayout layout;
  final _listenedControllers = <MultiSplitViewController>{};

  /// Set when the saved layout could not be loaded and the default replaced it.
  String? restoreProblem;

  /// The §22.2 default workspace.
  DockingArea defaultRoot() {
    DockingItem it(String id, {double? weight}) => _item(registry.byId(id)!, weight: weight);
    final bottom = [
      for (final id in [
        PanelRegistry.project,
        PanelRegistry.console,
        PanelRegistry.profiler,
        PanelRegistry.animation,
        PanelRegistry.import,
      ])
        if (registry.byId(id) != null) it(id),
    ];
    return DockingRow([
      it(PanelRegistry.hierarchy, weight: .2),
      DockingColumn([it(PanelRegistry.scene, weight: .7), DockingTabs(bottom, weight: .3)], weight: .6),
      it(PanelRegistry.inspector, weight: .2),
    ]);
  }

  DockingItem _item(PanelSpec spec, {double? weight, bool maximized = false}) {
    return DockingItem(
      id: spec.id,
      name: spec.title,
      keepAlive: true,
      weight: weight,
      maximized: maximized,
      widget: Builder(builder: spec.builder),
    );
  }

  void _applyDefault() {
    _attach(DockingLayout(root: defaultRoot()));
  }

  void _attach(DockingLayout l) {
    layout = l;
    layout.addListener(_onLayoutChanged);
    _listenSplitters();
    notifyListeners();
  }

  void _detach() {
    layout.removeListener(_onLayoutChanged);
    for (final c in _listenedControllers) {
      c.removeListener(_onSplitterChanged);
    }
    _listenedControllers.clear();
  }

  /// Divider drags change weights without a layout notification; listen to
  /// every row's and column's splitter controller.
  void _listenSplitters() {
    for (final area in layout.layoutAreas()) {
      final controller = switch (area) {
        DockingRow r => r.controller,
        DockingColumn c => c.controller,
        _ => null,
      };
      if (controller != null && _listenedControllers.add(controller)) controller.addListener(_onSplitterChanged);
    }
  }

  void _onLayoutChanged() {
    _listenSplitters();
    save();
    notifyListeners();
  }

  void _onSplitterChanged() => save();

  bool _restore() {
    final snapshot = store.read();
    if (snapshot == null) return false;
    if (snapshot.dockingFormat != LayoutSnapshot.format) {
      restoreProblem =
          'the saved layout uses format "${snapshot.dockingFormat}", this editor writes "${LayoutSnapshot.format}"';
      return false;
    }
    try {
      final l = DockingLayout();
      l.load(layout: snapshot.layout, parser: const _PanelParser(), builder: _PanelBuilder(this));
      if (l.root == null) return false;
      for (final area in l.layoutAreas()) {
        if (area is DockingTabs) {
          for (var i = 0; i < area.childrenCount; i++) {
            if (snapshot.selected.contains(area.childAt(i).id)) area.selectedIndex = i;
          }
        }
      }
      _attach(l);
      return true;
    } catch (e) {
      restoreProblem = 'the saved layout could not be loaded ($e)';
      return false;
    }
  }

  LayoutSnapshot snapshot() {
    final selected = <String>[];
    for (final area in layout.layoutAreas()) {
      if (area is DockingTabs && area.childrenCount > 0) {
        selected.add(area.childAt(area.selectedIndex.clamp(0, area.childrenCount - 1)).id as String);
      }
    }
    return LayoutSnapshot(
      layout: layout.stringify(parser: const _PanelParser()),
      selected: selected,
    );
  }

  void save() => store.write(snapshot());

  /// "Reset layout": the default, saved.
  void reset() {
    _detach();
    restoreProblem = null;
    _applyDefault();
    save();
  }

  bool isVisible(String id) => layout.findDockingItem(id) != null;
  Set<String> get visiblePanels => {
    for (final id in registry.ids)
      if (isVisible(id)) id,
  };

  /// Selects the panel's tab (and un-maximises anything else in the way).
  void focus(String id) {
    final item = layout.findDockingItem(id);
    if (item == null) return;
    final maximized = layout.maximizedArea;
    if (maximized != null && maximized != item && maximized != item.parent) layout.restore();
    final tabs = layout.findDockingTabsWithItem(id);
    if (tabs != null) {
      for (var i = 0; i < tabs.childrenCount; i++) {
        if (tabs.childAt(i).id == id) tabs.selectedIndex = i;
      }
    }
    layout.rebuild();
    save();
    notifyListeners();
  }

  void hide(String id) {
    final item = layout.findDockingItem(id);
    if (item == null) return;
    if (layout.layoutAreas().whereType<DockingItem>().length == 1) {
      // The last panel stays: an empty workspace has nothing to drop onto.
      return;
    }
    layout.removeItem(item: item);
  }

  /// Shows a hidden panel: into a tab group of its home when one is visible,
  /// otherwise at the edge of the layout its home names.
  void show(String id) {
    final spec = registry.byId(id);
    if (spec == null || isVisible(id)) {
      focus(id);
      return;
    }
    final item = _item(spec);
    final items = layout.layoutAreas().whereType<DockingItem>().toList();
    if (items.isEmpty) {
      _detach();
      _attach(DockingLayout(root: item));
      save();
      return;
    }
    final sibling = items.where((i) => registry.byId(i.id as String)?.home == spec.home).firstOrNull;
    if (sibling != null) {
      final tabs = layout.findDockingTabsWithItem(sibling.id);
      if (tabs != null) {
        layout.addItemOn(newItem: item, targetArea: tabs, dropIndex: tabs.childrenCount);
      } else {
        layout.addItemOn(newItem: item, targetArea: sibling, dropIndex: 1);
      }
    } else {
      // No panel of this home is visible: put it at the edge of the whole
      // workspace. Docking cannot re-wrap a root, so the tree is rebuilt
      // from fresh areas (panel widget state restarts; models live in the
      // shell, so nothing the user typed is lost).
      final old = layout.root!;
      final root = switch (spec.home) {
        PanelHome.left => DockingRow([_withWeight(item, .2), _clone(old)]),
        PanelHome.right || PanelHome.center => DockingRow([_clone(old), _withWeight(item, .2)]),
        PanelHome.bottom => DockingColumn([_clone(old), _withWeight(item, .3)]),
      };
      _detach();
      _attach(DockingLayout(root: root));
      save();
    }
    focus(id);
  }

  DockingItem _withWeight(DockingItem item, double weight) => _item(registry.byId(item.id as String)!, weight: weight);

  /// A fresh copy of an area tree (same ids, names, weights, selected tabs).
  DockingArea _clone(DockingArea a) => switch (a) {
    DockingItem i => _item(registry.byId(i.id as String)!, weight: i.weight, maximized: i.maximized),
    DockingTabs t => DockingTabs(
      [for (var i = 0; i < t.childrenCount; i++) _clone(t.childAt(i)) as DockingItem],
      weight: t.weight,
      maximized: t.maximized,
    )..selectedIndex = t.selectedIndex,
    DockingRow r => DockingRow([for (var i = 0; i < r.childrenCount; i++) _clone(r.childAt(i))], weight: r.weight),
    DockingColumn c => DockingColumn([
      for (var i = 0; i < c.childrenCount; i++) _clone(c.childAt(i)),
    ], weight: c.weight),
    _ => throw StateError('unknown docking area ${a.runtimeType}'),
  };

  void toggle(String id) => isVisible(id) ? hide(id) : show(id);

  @override
  void dispose() {
    _detach();
    super.dispose();
  }
}
