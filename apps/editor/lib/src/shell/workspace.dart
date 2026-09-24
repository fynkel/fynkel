// The docking workspace: the panel registry, the §22.2 default layout, show /
// hide / focus of panels, and the per-user layout file with its "Reset
// layout" fallback. Findings from the spike (ADR-0016): every panel is a
// keepAlive item; the package's string carries no selected tabs, so they are
// stored beside it.
//
// Item 9e: a panel's tab is drawn here (icon, title, a padlock when the
// copy is locked, a close × that fades in under the pointer); a right click
// opens its menu (Maximise / Restore, New <Panel>, Close) and a double
// click maximises or restores. The Inspector, the Project browser, the
// Hierarchy and the Scene view can be open more than once: the first copy
// has the panel's id, the others `<id>#<n>` (see [PanelIds]).
import 'dart:convert';
import 'dart:io';

import 'package:docking/docking.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';

import '../theme/icons.dart';
import '../theme/panel_focus.dart';
import '../theme/theme.dart';
import '../theme/tokens.dart';

/// Where a panel goes when it is shown and its group is gone.
enum PanelHome { left, center, right, bottom }

/// Builds one copy of a panel; [id] is the copy's id ([PanelIds]).
typedef PanelBuilder = Widget Function(BuildContext context, String id);

final class PanelSpec {
  const PanelSpec({
    required this.id,
    required this.title,
    required this.icon,
    required this.home,
    required this.builder,
    this.copies = false,
    this.fullTitle,
    this.locked,
  });

  /// Stable id: the layout file and the `window.panel.<id>` command use it.
  final String id;
  final String title;
  final IconData icon;
  final PanelHome home;
  final PanelBuilder builder;

  /// More than one copy can be open (item 9e).
  final bool copies;

  /// The name where [title] alone is ambiguous — "New Project Browser",
  /// not "New Project"; "New Scene View", not "New Scene".
  final String? fullTitle;

  String get name => fullTitle ?? title;

  /// Whether a copy is locked — its tab shows a padlock. Null for a panel
  /// without a lock. The shell calls [Workspace.refreshTabs] when it changes.
  final bool Function(String id)? locked;
}

/// The ids of panel copies: the first copy of a panel has the spec's id,
/// the n-th `<id>#<n>` (n ≥ 2). The layout string stores ids with their
/// length, so `#` needs no escaping.
abstract final class PanelIds {
  static String base(String id) {
    final i = id.indexOf('#');
    return i < 0 ? id : id.substring(0, i);
  }

  /// 1 for the first copy, n for `<id>#<n>`, 0 for an id that is neither.
  static int number(String id) {
    final i = id.indexOf('#');
    if (i < 0) return 1;
    final n = int.tryParse(id.substring(i + 1));
    return n != null && n >= 2 && '$n' == id.substring(i + 1) ? n : 0;
  }

  static String of(String base, int n) => n <= 1 ? base : '$base#$n';
}

final class PanelRegistry {
  PanelRegistry(Iterable<PanelSpec> specs) : _specs = {for (final s in specs) s.id: s};

  final Map<String, PanelSpec> _specs;

  List<PanelSpec> get all => _specs.values.toList();

  /// The spec of a panel or of one of its copies.
  PanelSpec? byId(String id) {
    final n = PanelIds.number(id);
    final spec = _specs[PanelIds.base(id)];
    if (spec == null || n == 0 || (n > 1 && !spec.copies)) return null;
    return spec;
  }

  Iterable<String> get ids => _specs.keys;

  /// A copy's tab title: "Inspector", "Inspector 2".
  String titleOf(String id) {
    final spec = byId(id);
    if (spec == null) return id;
    final n = PanelIds.number(id);
    return n > 1 ? '${spec.title} $n' : spec.title;
  }

  /// The §22.2 panel set and its default placement.
  static const hierarchy = 'hierarchy';
  static const scene = 'scene';
  static const game = 'game';
  static const inspector = 'inspector';
  static const project = 'project';
  static const console = 'console';
  static const profiler = 'profiler';
  static const animation = 'animation';
  static const defaultOrder = [hierarchy, scene, inspector, project, console, profiler, animation, game];

  /// Panels that existed once and are gone. A saved layout that still names
  /// one loads without it (and is saved again without it) instead of being
  /// thrown away: `import` was the Asset panel, whose content became the
  /// Inspector's asset pages in item 9c.
  static const retired = {'import'};
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
    if (spec == null && PanelRegistry.retired.contains(id)) {
      // A placeholder the restore removes before the layout is shown.
      return DockingItem(id: id, name: id, widget: const SizedBox.shrink());
    }
    if (spec == null) throw StateError('unknown panel "$id"');
    return workspace._item(spec, id, weight: weight, maximized: maximized);
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

  /// The panel copy that holds keyboard focus, or null (item 9b: the accent
  /// follows focus — its tab's icon is drawn in the accent).
  final focusedPanel = ValueNotifier<String?>(null);

  /// Per panel, its copies by when they last had the keyboard, the latest
  /// first ([copiesByRecency]).
  final _recent = <String, List<String>>{};

  /// Per panel copy, what moves the keyboard into it ([focusKeyboard]).
  final _handles = <String, PanelFocusHandle>{};

  /// Bumped by [refreshTabs]: the tabs redraw their padlocks.
  final _tabs = ValueNotifier<int>(0);

  @override
  void dispose() {
    _detach();
    focusedPanel.dispose();
    _tabs.dispose();
    super.dispose();
  }

  /// A copy's lock changed: its tab redraws.
  void refreshTabs() => _tabs.value++;

  /// The §22.2 default workspace.
  DockingArea defaultRoot() {
    DockingItem it(String id, {double? weight}) => _item(registry.byId(id)!, id, weight: weight);
    final bottom = [
      for (final id in [PanelRegistry.project, PanelRegistry.console, PanelRegistry.profiler, PanelRegistry.animation])
        if (registry.byId(id) != null) it(id),
    ];
    return DockingRow([
      it(PanelRegistry.hierarchy, weight: .2),
      DockingColumn([
        // Scene and Game side by side as tabs, as in Unity (ADR-0021).
        if (registry.byId(PanelRegistry.game) != null)
          DockingTabs([it(PanelRegistry.scene), it(PanelRegistry.game)], weight: .7)
        else
          it(PanelRegistry.scene, weight: .7),
        DockingTabs(bottom, weight: .3),
      ], weight: .6),
      it(PanelRegistry.inspector, weight: .2),
    ]);
  }

  DockingItem _item(PanelSpec spec, String id, {double? weight, bool maximized = false}) {
    return DockingItem(
      id: id,
      name: registry.titleOf(id),
      keepAlive: true,
      closable: false,
      weight: weight,
      maximized: maximized,
      leading: (context, status) => _PanelTab(workspace: this, spec: spec, id: id, status: status),
      widget: PanelFocus(
        debugLabel: 'panel.$id',
        handle: _handles.putIfAbsent(id, PanelFocusHandle.new),
        onChanged: (focused) {
          if (focused) {
            _touch(id);
            focusedPanel.value = id;
          } else if (focusedPanel.value == id) {
            focusedPanel.value = null;
          }
        },
        child: Builder(builder: (context) => spec.builder(context, id)),
      ),
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
      final retired = [
        for (final id in PanelRegistry.retired)
          if (l.findDockingItem(id) != null) id,
      ];
      if (retired.isNotEmpty) l.removeItemByIds(retired);
      if (l.root == null) return false;
      for (final area in l.layoutAreas()) {
        if (area is DockingTabs) {
          for (var i = 0; i < area.childrenCount; i++) {
            if (snapshot.selected.contains(area.childAt(i).id)) area.selectedIndex = i;
          }
        }
      }
      _attach(l);
      if (retired.isNotEmpty) save();
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
    _recent.clear();
    _applyDefault();
    save();
  }

  /// This exact copy is in the layout.
  bool isVisible(String id) => layout.findDockingItem(id) != null;

  /// Some copy of the panel [base] is in the layout.
  bool isShown(String base) => copiesOf(base).isNotEmpty;

  /// The panels with at least one copy in the layout, by their spec ids.
  Set<String> get visiblePanels => {
    for (final id in registry.ids)
      if (isShown(id)) id,
  };

  /// The copies of [base] in the layout, the first copy first.
  List<String> copiesOf(String base) {
    final out = [
      for (final item in layout.layoutAreas().whereType<DockingItem>())
        if (item.id is String && PanelIds.base(item.id as String) == base && registry.byId(item.id as String) != null)
          item.id as String,
    ];
    out.sort((a, b) => PanelIds.number(a).compareTo(PanelIds.number(b)));
    return out;
  }

  /// The copies of [base] in the layout, the one that had the keyboard
  /// last first, then the others that had it, then the rest by number.
  List<String> copiesByRecency(String base) {
    final copies = copiesOf(base);
    final recent = [
      for (final id in _recent[base] ?? const <String>[])
        if (copies.contains(id)) id,
    ];
    return [...recent, ...copies.where((id) => !recent.contains(id))];
  }

  /// The copy of [base] that commands address: the one that had the
  /// keyboard last, else the first one in the layout; null when none is.
  String? activeCopy(String base) => copiesByRecency(base).firstOrNull;

  /// [id] is now the copy of its panel used last.
  void _touch(String id) {
    (_recent[PanelIds.base(id)] ??= [])
      ..remove(id)
      ..insert(0, id);
  }

  /// Selects the panel's tab (and un-maximises anything else in the way);
  /// the copy becomes its panel's [activeCopy]. A panel's spec id focuses
  /// its [activeCopy].
  void focus(String id) {
    final item = layout.findDockingItem(id) ?? _activeItem(id);
    if (item == null) return;
    _touch(item.id as String);
    final maximized = layout.maximizedArea;
    if (maximized != null && maximized != item && maximized != item.parent) layout.restore();
    final tabs = layout.findDockingTabsWithItem(item.id);
    if (tabs != null) {
      for (var i = 0; i < tabs.childrenCount; i++) {
        if (tabs.childAt(i).id == item.id) tabs.selectedIndex = i;
      }
    }
    layout.rebuild();
    save();
    notifyListeners();
  }

  /// Shows the panel and moves the keyboard into it, a frame later when
  /// its tab has come forward (Window → Go to *Panel*, Unity's ⌘1…).
  void focusKeyboard(String id) {
    show(id);
    final target = PanelIds.number(id) == 1 ? activeCopy(id) ?? id : id;
    WidgetsBinding.instance.addPostFrameCallback((_) => _handles[target]?.focus());
    WidgetsBinding.instance.scheduleFrame();
  }

  DockingItem? _activeItem(String id) {
    final copy = PanelIds.number(id) == 1 ? activeCopy(id) : null;
    return copy == null ? null : layout.findDockingItem(copy);
  }

  /// Removes one copy from the layout — never the last panel: an empty
  /// workspace has nothing to drop onto.
  void hide(String id) {
    final item = layout.findDockingItem(id);
    if (item == null || !canClose) return;
    layout.removeItem(item: item);
  }

  /// Whether a panel can be closed (it is not the only one left).
  bool get canClose => layout.layoutAreas().whereType<DockingItem>().length > 1;

  /// Shows a panel: a spec id with a copy in the layout focuses that copy;
  /// otherwise the panel goes into a tab group of its home when one is
  /// visible, else at the edge of the layout its home names.
  void show(String id) {
    final spec = registry.byId(id);
    if (spec == null) return;
    if (isVisible(id) || (PanelIds.number(id) == 1 && isShown(id))) {
      focus(id);
      return;
    }
    _place(_item(spec, id), spec.home);
    focus(id);
  }

  void _place(DockingItem item, PanelHome home) {
    final items = layout.layoutAreas().whereType<DockingItem>().toList();
    if (items.isEmpty) {
      _detach();
      _attach(DockingLayout(root: item));
      save();
      return;
    }
    final sibling = items.where((i) => registry.byId(i.id as String)?.home == home).firstOrNull;
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
      final root = switch (home) {
        PanelHome.left => DockingRow([_withWeight(item, .2), _clone(old)]),
        PanelHome.right || PanelHome.center => DockingRow([_clone(old), _withWeight(item, .2)]),
        PanelHome.bottom => DockingColumn([_clone(old), _withWeight(item, .3)]),
      };
      _detach();
      _attach(DockingLayout(root: root));
      save();
    }
  }

  /// Opens a new copy of [base] (item 9e) beside [beside] — or beside the
  /// panel's [activeCopy] — splitting its space, and focuses it. The copy
  /// takes the lowest free number. Returns its id; null when the panel
  /// cannot be copied.
  String? openCopy(String base, {String? beside}) {
    final spec = registry.byId(base);
    if (spec == null || !spec.copies) return null;
    if (!isShown(base)) {
      show(base);
      return base;
    }
    var n = 2;
    while (isVisible(PanelIds.of(base, n))) {
      n++;
    }
    final id = PanelIds.of(base, n);
    final nextId = beside ?? activeCopy(base) ?? base;
    // Beside a panel in a tab group means beside the group: docking does
    // not split one tab of a group.
    final DockingArea? next = layout.findDockingTabsWithItem(nextId) ?? layout.findDockingItem(nextId);
    // As wide as its neighbour: the split view evens a new pair out (and
    // takes a new column's width from every column alike), so the copy
    // does not squeeze the group it opened beside.
    final item = _item(spec, id, weight: next?.weight);
    if (next == null) {
      _place(item, spec.home);
    } else {
      final layoutMaximized = layout.maximizedArea;
      if (layoutMaximized != null) layout.restore();
      layout.addItemOn(newItem: item, targetArea: next as DropArea, dropPosition: DropPosition.right);
    }
    focus(id);
    return id;
  }

  bool isMaximized(String id) {
    final item = layout.findDockingItem(id);
    final m = layout.maximizedArea;
    return item != null && m != null && (m == item || m == item.parent);
  }

  /// The tab menu's Maximise / Restore and a tab's double click.
  void toggleMaximized(String id) {
    final item = layout.findDockingItem(id);
    if (item == null) return;
    if (isMaximized(id)) {
      layout.restore();
    } else {
      layout.maximizeDockingItem(item);
    }
    save();
    notifyListeners();
  }

  DockingItem _withWeight(DockingItem item, double weight) =>
      _item(registry.byId(item.id as String)!, item.id as String, weight: weight);

  /// A fresh copy of an area tree (same ids, names, weights, selected tabs).
  DockingArea _clone(DockingArea a) => switch (a) {
    DockingItem i => _item(registry.byId(i.id as String)!, i.id as String, weight: i.weight, maximized: i.maximized),
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

  /// The Window menu's Show / Hide: hides every copy of [base] (all but
  /// the last panel), or shows the panel.
  void toggle(String base) {
    final copies = copiesOf(base);
    if (copies.isEmpty) {
      show(base);
      return;
    }
    for (final id in copies.reversed) {
      hide(id);
    }
  }
}

/// A panel's tab (item 9e): the icon (in the accent while focus is in the
/// panel), the title, a padlock when the copy is locked, and a close × that
/// fades in under the pointer. A right click opens the tab's menu, a double
/// click maximises or restores. The package draws its own title after this
/// at zero size (`chrome.dart`): it still names the tab for the drag, the
/// overflow menu and a screen reader.
class _PanelTab extends StatefulWidget {
  const _PanelTab({required this.workspace, required this.spec, required this.id, required this.status});

  final Workspace workspace;
  final PanelSpec spec;
  final String id;
  final TabStatus status;

  @override
  State<_PanelTab> createState() => _PanelTabState();
}

class _PanelTabState extends State<_PanelTab> {
  bool _hover = false;
  Duration? _lastDown;
  Offset? _lastDownAt;

  Workspace get _ws => widget.workspace;

  void _pointerDown(PointerDownEvent e) {
    if (e.kind == PointerDeviceKind.mouse && e.buttons == kSecondaryMouseButton) {
      _menu(e.position);
      return;
    }
    if (e.buttons != kPrimaryButton) return;
    // A double click, read from the raw pointer: a double-tap recognizer
    // would hold the package's tap (tab selection) until it gave up.
    final last = _lastDown, at = _lastDownAt;
    if (last != null &&
        at != null &&
        e.timeStamp - last <= kDoubleTapTimeout &&
        (e.position - at).distance <= kDoubleTapSlop) {
      _lastDown = null;
      _ws.toggleMaximized(widget.id);
      return;
    }
    _lastDown = e.timeStamp;
    _lastDownAt = e.position;
  }

  Future<void> _menu(Offset at) async {
    final overlay = Overlay.of(context).context.findRenderObject() as RenderBox;
    final spec = widget.spec;
    final choice = await showMenu<String>(
      popUpAnimationStyle: AnimationStyle.noAnimation,
      context: context,
      position: RelativeRect.fromRect(Rect.fromPoints(at, at), Offset.zero & overlay.size),
      items: [
        PopupMenuItem(
          key: const Key('tab.menu.maximize'),
          value: 'maximize',
          child: Text(_ws.isMaximized(widget.id) ? 'Restore' : 'Maximise'),
        ),
        if (spec.copies) PopupMenuItem(key: const Key('tab.menu.copy'), value: 'copy', child: Text('New ${spec.name}')),
        const PopupMenuDivider(),
        PopupMenuItem(
          key: const Key('tab.menu.close'),
          value: 'close',
          enabled: _ws.canClose,
          child: const Text('Close'),
        ),
      ],
    );
    switch (choice) {
      case 'maximize':
        _ws.toggleMaximized(widget.id);
      case 'copy':
        _ws.openCopy(PanelIds.base(widget.id), beside: widget.id);
      case 'close':
        _ws.hide(widget.id);
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final spec = widget.spec;
    final lit = widget.status != TabStatus.normal || _hover;
    return Listener(
      onPointerDown: _pointerDown,
      child: MouseRegion(
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: Padding(
          key: Key('tab.${widget.id}'),
          padding: EdgeInsets.only(left: 10, right: 4, top: (t.tab - 16) / 2, bottom: (t.tab - 16) / 2),
          child: ListenableBuilder(
            listenable: Listenable.merge([_ws.focusedPanel, _ws._tabs]),
            builder: (context, _) {
              final focused = _ws.focusedPanel.value == widget.id;
              final locked = spec.locked?.call(widget.id) ?? false;
              final title = ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 180),
                child: ExcludeSemantics(
                  child: Text(
                    _ws.registry.titleOf(widget.id),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontFamily: uiFontFamily,
                      fontSize: 11.5,
                      height: 1.2,
                      color: lit ? t.text : t.textSecondary,
                    ),
                  ),
                ),
              );
              // The strip measures a tab unbounded, then squeezes the only
              // tab of a narrow group: the title gives way first (…), and
              // below the icon and the × the tab is cut.
              return LayoutBuilder(
                builder: (context, c) => Flex(
                  direction: Axis.horizontal,
                  mainAxisSize: MainAxisSize.min,
                  clipBehavior: Clip.hardEdge,
                  children: [
                    Icon(spec.icon, size: 13, color: focused ? t.accent : t.textSecondary),
                    const SizedBox(width: 5),
                    if (c.hasBoundedWidth) Flexible(child: title) else title,
                    if (locked) ...[
                      const SizedBox(width: 4),
                      Icon(FIcons.lock, key: Key('tab.locked.${widget.id}'), size: 11, color: t.textSecondary),
                    ],
                    const SizedBox(width: 3),
                    _TabClose(
                      key: Key('tab.close.${widget.id}'),
                      visible: _hover && _ws.canClose,
                      onPressed: () => _ws.hide(widget.id),
                    ),
                  ],
                ),
              );
            },
          ),
        ),
      ),
    );
  }
}

/// The × at a tab's end: always holding its place (the tab does not grow
/// under the pointer), faded in on hover and clickable only then.
class _TabClose extends StatefulWidget {
  const _TabClose({super.key, required this.visible, required this.onPressed});

  final bool visible;
  final VoidCallback onPressed;

  @override
  State<_TabClose> createState() => _TabCloseState();
}

class _TabCloseState extends State<_TabClose> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return IgnorePointer(
      ignoring: !widget.visible,
      child: AnimatedOpacity(
        opacity: widget.visible ? 1 : 0,
        duration: t.motion,
        child: Tooltip(
          message: 'Close',
          child: MouseRegion(
            cursor: SystemMouseCursors.click,
            onEnter: (_) => setState(() => _hover = true),
            onExit: (_) => setState(() => _hover = false),
            child: GestureDetector(
              onTap: widget.onPressed,
              child: Container(
                width: 16,
                height: 16,
                alignment: Alignment.center,
                decoration: BoxDecoration(color: _hover ? t.hover : null, borderRadius: t.cornerControl),
                child: Icon(FIcons.close, size: 11, color: _hover ? t.text : t.textSecondary),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
