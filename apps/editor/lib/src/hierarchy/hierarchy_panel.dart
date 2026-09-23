// The Hierarchy panel: a virtualised tree of the focused document's
// entities (one `ListView.builder` over the tree's cached rows, a fixed row
// height, so 10 000 entities scroll without building more than the visible
// rows), search and filter, select / multi-select / range, rename in place,
// the context menu, drag-and-drop with above / onto / below zones, prefab
// and error badges, and the keyboard (arrows, ↩ / F2 rename, ⌫ delete, ⌘A).
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../commands/commands.dart';
import '../inspector/field_widgets.dart' show AssetDragPayload;
import '../play/play_document.dart' show PlayDocument;
import '../theme/icons.dart';
import '../theme/lock_button.dart';
import '../theme/panel_focus.dart';
import '../theme/tokens.dart';
import 'hierarchy_model.dart';
import 'hierarchy_tree.dart';

/// One tree row, from the density preference (item 9a) — the scroll math,
/// the list's `itemExtent` and the row itself all measure with this.
double hierarchyRowHeight(BuildContext context) => context.tokens.row;

class HierarchyPanel extends StatefulWidget {
  const HierarchyPanel({super.key, required this.model, required this.onOpenDocument, this.searchFocus, this.commands});

  final HierarchyModel model;

  /// The empty state's "Open Document…".
  final VoidCallback onOpenDocument;
  final FocusNode? searchFocus;

  /// The registry the context menu's Prefab mode items run through
  /// (`prefab.*` — the same commands as the Edit menu and the palette).
  final CommandRegistry? commands;

  @override
  State<HierarchyPanel> createState() => _HierarchyPanelState();
}

class _HierarchyPanelState extends State<HierarchyPanel> {
  final _search = TextEditingController();
  late final FocusNode _searchFocus = widget.searchFocus ?? FocusNode(debugLabel: 'hierarchy.search');
  final _listFocus = FocusNode(debugLabel: 'hierarchy.list');
  final _scroll = ScrollController();
  int _seenSearchRequests = 0;
  String? _lastLead;
  TextEditingController? _rename;
  String? _renameFor;
  DateTime? _lastTap;
  String? _lastTapId;
  String? _wasRenaming;

  HierarchyModel get model => widget.model;

  @override
  void initState() {
    super.initState();
    model.addListener(_onModel);
    _search.text = model.filter.query;
  }

  @override
  void dispose() {
    model.removeListener(_onModel);
    _search.dispose();
    if (widget.searchFocus == null) _searchFocus.dispose();
    _listFocus.dispose();
    _scroll.dispose();
    _rename?.dispose();
    super.dispose();
  }

  void _onModel() {
    // A rename that ended (committed or cancelled) hands the keyboard back to the list.
    final renaming = model.renaming;
    if (_wasRenaming != null && renaming == null) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _listFocus.requestFocus();
      });
    }
    _wasRenaming = renaming;
    if (model.searchFocusRequests != _seenSearchRequests) {
      _seenSearchRequests = model.searchFocusRequests;
      WidgetsBinding.instance.addPostFrameCallback((_) => _searchFocus.requestFocus());
    }
    final q = model.filter.query;
    if (_search.text != q) _search.text = q;
    final lead = model.tab?.lead;
    if (lead != null && lead != _lastLead) {
      _lastLead = lead;
      WidgetsBinding.instance.addPostFrameCallback((_) => _scrollTo(lead));
    }
    if (mounted) setState(() {});
  }

  void _scrollTo(String id) {
    final t = model.tree;
    if (t == null || !_scroll.hasClients) return;
    final i = t.rowIndexOf(id);
    if (i < 0) return;
    final h = hierarchyRowHeight(context);
    final top = i * h;
    final viewTop = _scroll.offset;
    final viewBottom = viewTop + _scroll.position.viewportDimension;
    if (top < viewTop) {
      _scroll.jumpTo(top);
    } else if (top + h > viewBottom) {
      _scroll.jumpTo(top + h - _scroll.position.viewportDimension);
    }
  }

  // ── Keys ──────────────────────────────────────────────────────────────

  KeyEventResult _onKey(FocusNode node, KeyEvent e) {
    if (e is! KeyDownEvent && e is! KeyRepeatEvent) return KeyEventResult.ignored;
    final k = e.logicalKey;
    if (model.renaming != null) {
      // The rename field owns the keyboard; only ⎋ is ours.
      if (k == LogicalKeyboardKey.escape) {
        model.cancelRename();
        return KeyEventResult.handled;
      }
      return KeyEventResult.ignored;
    }
    final shift = HardwareKeyboard.instance.isShiftPressed;
    final meta = HardwareKeyboard.instance.isMetaPressed;
    if (k == LogicalKeyboardKey.arrowDown) {
      model.step(1, extend: shift);
    } else if (k == LogicalKeyboardKey.arrowUp) {
      model.step(-1, extend: shift);
    } else if (k == LogicalKeyboardKey.arrowRight) {
      model.arrow(true);
    } else if (k == LogicalKeyboardKey.arrowLeft) {
      model.arrow(false);
    } else if (k == LogicalKeyboardKey.enter || k == LogicalKeyboardKey.f2) {
      model.beginRename();
    } else if ((k == LogicalKeyboardKey.backspace || k == LogicalKeyboardKey.delete) && !meta) {
      model.delete();
    } else if (k == LogicalKeyboardKey.keyA && meta) {
      model.selectAll();
    } else if (k == LogicalKeyboardKey.escape) {
      if (model.filter.isEmpty) {
        model.clearSelection();
      } else {
        model.clearFilter();
      }
    } else {
      return KeyEventResult.ignored;
    }
    return KeyEventResult.handled;
  }

  // ── Build ─────────────────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    final tab = model.tab;
    if (tab == null) return _empty(context, theme);
    final tree = tab.tree;
    final rows = tree.rows;
    return Column(
      children: [
        _toolbar(context, theme),
        if (model.locked)
          Container(
            key: const Key('hierarchy.lockedTo'),
            width: double.infinity,
            color: t.header,
            padding: EdgeInsets.symmetric(horizontal: t.pad, vertical: 3),
            child: Text(
              'Locked to ${tab.tabLabel}',
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary),
            ),
          ),
        if (tab.document is PlayDocument)
          Container(
            key: const Key('hierarchy.playing'),
            width: double.infinity,
            color: t.header,
            padding: EdgeInsets.symmetric(horizontal: t.pad, vertical: 3),
            child: Text(
              'Playing — the running game, as it is now',
              style: theme.textTheme.bodySmall?.copyWith(color: t.accent),
            ),
          )
        else if (tab.document.readOnly != null)
          Container(
            key: const Key('hierarchy.readOnly'),
            width: double.infinity,
            color: t.header,
            padding: EdgeInsets.symmetric(horizontal: t.pad, vertical: 3),
            child: Text(
              'Read-only — ${tab.document.readOnly}',
              style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
            ),
          ),
        Expanded(
          child: rows.isEmpty
              ? Center(
                  child: Text(
                    tree.isEmpty
                        ? 'This document has no entities yet. Use + to create one.'
                        : 'Nothing matches the filter.',
                    key: const Key('hierarchy.noRows'),
                    textAlign: TextAlign.center,
                    style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
                  ),
                )
              : Focus(
                  focusNode: _listFocus,
                  onKeyEvent: _onKey,
                  child: GestureDetector(
                    behavior: HitTestBehavior.translucent,
                    onTap: () {
                      _listFocus.requestFocus();
                      model.clearSelection();
                    },
                    child: _RootDropTarget(
                      model: model,
                      child: ListView.builder(
                        key: const Key('hierarchy.list'),
                        controller: _scroll,
                        itemCount: rows.length,
                        itemExtent: hierarchyRowHeight(context),
                        itemBuilder: (context, i) => _row(context, theme, tab.tree, rows[i]),
                      ),
                    ),
                  ),
                ),
        ),
        if (tab.lastError != null) _errorBar(context, theme, tab.lastError!),
        _status(context, theme, tree, tab.selection.length),
      ],
    );
  }

  Widget _empty(BuildContext context, ThemeData theme) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(FIcons.accountTreeOutlined, size: 32, color: context.tokens.textDisabled),
            const SizedBox(height: 12),
            Text('No document open', style: theme.textTheme.titleMedium),
            const SizedBox(height: 4),
            Text(
              'The Hierarchy shows the entities of the focused tab.',
              textAlign: TextAlign.center,
              style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
            ),
            const SizedBox(height: 12),
            FilledButton.tonalIcon(
              key: const Key('hierarchy.openDocument'),
              onPressed: widget.onOpenDocument,
              icon: const Icon(FIcons.folderOpen),
              label: const Text('Open Document…'),
            ),
          ],
        ),
      ),
    );
  }

  Widget _toolbar(BuildContext context, ThemeData theme) {
    final filter = model.filter;
    final activeFilters =
        filter.componentTypes.length + (filter.prefab == PrefabFilter.any ? 0 : 1) + (filter.hasError ? 1 : 0);
    final t = context.tokens;
    return Container(
      decoration: BoxDecoration(
        color: t.header,
        border: Border(bottom: t.hairline),
      ),
      padding: EdgeInsets.fromLTRB(t.gap, 3, 3, 3),
      child: Row(
        children: [
          Expanded(
            child: TextField(
              key: const Key('hierarchy.search'),
              controller: _search,
              focusNode: _searchFocus,
              style: theme.textTheme.bodySmall?.copyWith(color: t.text),
              decoration: InputDecoration(
                prefixIcon: Icon(FIcons.search, size: 13, color: t.textDisabled),
                prefixIconConstraints: BoxConstraints(minWidth: t.row, minHeight: t.row),
                hintText: 'Search name, id, component…',
                suffixIcon: filter.query.isEmpty
                    ? null
                    : IconButton(
                        tooltip: 'Clear',
                        iconSize: 12,
                        icon: const Icon(FIcons.close),
                        onPressed: () {
                          _search.clear();
                          model.setQuery('');
                        },
                      ),
              ),
              onChanged: model.setQuery,
              onSubmitted: (_) => _listFocus.requestFocus(),
            ),
          ),
          Badge(
            isLabelVisible: activeFilters > 0,
            label: Text('$activeFilters'),
            child: PopupMenuButton<String>(
              popUpAnimationStyle: AnimationStyle.noAnimation,
              key: const Key('hierarchy.filter'),
              tooltip: 'Filter by component, prefab state, errors',
              icon: Icon(FIcons.filterList, color: activeFilters > 0 ? t.accent : null),
              itemBuilder: (context) => [
                CheckedPopupMenuItem(
                  key: const Key('hierarchy.filter.hasError'),
                  value: 'hasError',
                  checked: filter.hasError,
                  child: const Text('Has error'),
                ),
                const PopupMenuDivider(),
                CheckedPopupMenuItem(
                  value: 'prefab.any',
                  checked: filter.prefab == PrefabFilter.any,
                  child: const Text('Any entity'),
                ),
                CheckedPopupMenuItem(
                  key: const Key('hierarchy.filter.instances'),
                  value: 'prefab.instances',
                  checked: filter.prefab == PrefabFilter.instances,
                  child: const Text('Prefab instances only'),
                ),
                CheckedPopupMenuItem(
                  value: 'prefab.plain',
                  checked: filter.prefab == PrefabFilter.plain,
                  child: const Text('Plain entities only'),
                ),
                const PopupMenuDivider(),
                for (final t in model.presentComponentTypes)
                  CheckedPopupMenuItem(
                    key: Key('hierarchy.filter.component.$t'),
                    value: 'component:$t',
                    checked: filter.componentTypes.contains(t),
                    child: Text(t),
                  ),
                if (!filter.isEmpty) ...[
                  const PopupMenuDivider(),
                  const PopupMenuItem(value: 'clear', child: Text('Clear filters')),
                ],
              ],
              onSelected: (v) {
                if (v == 'hasError') {
                  model.setFilter(filter.copyWith(hasError: !filter.hasError));
                } else if (v == 'prefab.any') {
                  model.setFilter(filter.copyWith(prefab: PrefabFilter.any));
                } else if (v == 'prefab.instances') {
                  model.setFilter(filter.copyWith(prefab: PrefabFilter.instances));
                } else if (v == 'prefab.plain') {
                  model.setFilter(filter.copyWith(prefab: PrefabFilter.plain));
                } else if (v.startsWith('component:')) {
                  model.toggleComponentFilter(v.substring('component:'.length));
                } else if (v == 'clear') {
                  _search.clear();
                  model.clearFilter();
                }
              },
            ),
          ),
          LockButton(
            key: const Key('hierarchy.lock'),
            locked: model.locked,
            lockTip: 'Lock: keep showing this document when another tab is focused',
            unlockTip: 'Unlock: follow the focused tab again',
            onChanged: model.hasDocument ? model.setLocked : null,
          ),
          PopupMenuButton<String>(
            popUpAnimationStyle: AnimationStyle.noAnimation,
            key: const Key('hierarchy.create'),
            tooltip: 'Create an entity (under the selection when one is selected)',
            icon: const Icon(FIcons.add),
            enabled: model.canEdit,
            itemBuilder: (context) => [
              const PopupMenuItem(
                key: Key('hierarchy.create.empty'),
                value: '',
                child: ListTile(dense: true, leading: Icon(FIcons.addBoxOutlined), title: Text('Create Empty')),
              ),
              const PopupMenuDivider(),
              for (final c in model.nativeComponentTypes)
                if (c.type != 'fynkel.Transform')
                  PopupMenuItem(
                    key: Key('hierarchy.create.${c.type}'),
                    value: c.type,
                    child: Text('Create with ${c.type}'),
                  ),
            ],
            onSelected: (v) => v.isEmpty ? model.createEmpty() : model.createWith(v),
          ),
          PopupMenuButton<String>(
            popUpAnimationStyle: AnimationStyle.noAnimation,
            key: const Key('hierarchy.more'),
            tooltip: 'More',
            icon: const Icon(FIcons.moreVert),
            itemBuilder: (context) => [
              CheckedPopupMenuItem(
                key: const Key('hierarchy.keepWorld'),
                value: 'keepWorld',
                checked: model.keepWorldTransform,
                child: const Text('Keep world transform when reparenting'),
              ),
              const PopupMenuDivider(),
              const PopupMenuItem(value: 'expandAll', child: Text('Expand All')),
              const PopupMenuItem(value: 'collapseAll', child: Text('Collapse All')),
            ],
            onSelected: (v) {
              switch (v) {
                case 'keepWorld':
                  model.toggleKeepWorldTransform();
                case 'expandAll':
                  model.expandAll();
                case 'collapseAll':
                  model.collapseAll();
              }
            },
          ),
        ],
      ),
    );
  }

  Widget _errorBar(BuildContext context, ThemeData theme, String text) {
    final t = context.tokens;
    return Container(
      key: const Key('hierarchy.error'),
      decoration: BoxDecoration(
        color: t.surfaceOf(t.error, .22),
        border: Border(top: t.hairline),
      ),
      padding: EdgeInsets.fromLTRB(t.pad, 2, 2, 2),
      child: Row(
        children: [
          Icon(FIcons.errorOutline, size: 13, color: t.error),
          const SizedBox(width: 6),
          Expanded(
            child: Text(text, style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onErrorContainer)),
          ),
          IconButton(
            iconSize: 12,
            padding: EdgeInsets.zero,
            icon: const Icon(FIcons.close),
            onPressed: () {
              model.tab?.lastError = null;
              setState(() {});
            },
          ),
        ],
      ),
    );
  }

  Widget _status(BuildContext context, ThemeData theme, HierarchyTree tree, int selected) {
    final visible = tree.rows.length;
    final t = context.tokens;
    return Container(
      key: const Key('hierarchy.status'),
      height: t.row + 2,
      padding: EdgeInsets.symmetric(horizontal: t.pad),
      decoration: BoxDecoration(
        color: t.header,
        border: Border(top: t.hairline),
      ),
      alignment: Alignment.centerLeft,
      child: Text(
        '${tree.entityCount} entities'
        '${tree.filter.isEmpty ? '' : ' · ${tree.matching.length} match'}'
        '${visible != tree.entityCount && tree.filter.isEmpty ? ' · $visible shown' : ''}'
        '${selected > 0 ? ' · $selected selected' : ''}'
        '${model.keepWorldTransform ? '' : ' · local transforms kept on move'}',
        style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
      ),
    );
  }

  // ── Rows ──────────────────────────────────────────────────────────────

  Widget _row(BuildContext context, ThemeData theme, HierarchyTree tree, HierarchyRow row) {
    final n = row.node;
    final selected = model.tab!.isSelected(n.id);
    final renaming = model.renaming == n.id;
    return _HierarchyRowWidget(
      key: ValueKey('row.${n.id}'),
      model: model,
      row: row,
      selected: selected,
      renaming: renaming,
      renameController: renaming ? _renameController(n) : null,
      errors: tree.errors[n.id] ?? const [],
      onTap: () => _tapRow(n.id),
      onDoubleTap: () => model.beginRename(n.id),
      onSecondaryTap: (at) => _contextMenu(context, n.id, at),
      onToggle: () => model.toggle(n.id),
      onRenameSubmit: (v) => model.rename(n.id, v),
      onRenameCancel: model.cancelRename,
    );
  }

  TextEditingController _renameController(HierarchyNode n) {
    if (_renameFor != n.id) {
      _rename?.dispose();
      _rename = TextEditingController(text: n.name)
        ..selection = TextSelection(baseOffset: 0, extentOffset: n.name.length);
      _renameFor = n.id;
    }
    return _rename!;
  }

  void _tapRow(String id) {
    final now = DateTime.now();
    final isDouble = _lastTapId == id && _lastTap != null && now.difference(_lastTap!) < kDoubleTapTimeout;
    _lastTap = now;
    _lastTapId = id;
    _listFocus.requestFocus();
    if (isDouble && model.tab!.isSelected(id) && model.tab!.selection.length == 1) {
      model.beginRename(id);
      return;
    }
    model.select(
      id,
      toggle: HardwareKeyboard.instance.isMetaPressed || HardwareKeyboard.instance.isControlPressed,
      range: HardwareKeyboard.instance.isShiftPressed,
    );
  }

  Future<void> _contextMenu(BuildContext context, String id, Offset at) async {
    final tab = model.tab!;
    if (!tab.isSelected(id)) model.select(id);
    final n = tab.tree[id];
    if (n == null) return;
    final many = tab.selection.length > 1;
    final derived = n.isDerived;
    final anyDerived = tab.selection.any((s) => tab.tree[s]?.isDerived ?? false);
    final prefabCommands = [
      for (final cid in const [
        'prefab.revertOverrides',
        'prefab.applyOverrides',
        'prefab.unpack',
        'prefab.open',
        'prefab.selectAsset',
      ])
        ?widget.commands?.byId(cid),
    ];
    final overlay = Overlay.of(context).context.findRenderObject() as RenderBox;
    final choice = await showMenu<String>(
      popUpAnimationStyle: AnimationStyle.noAnimation,
      context: context,
      position: RelativeRect.fromRect(Rect.fromPoints(at, at), Offset.zero & overlay.size),
      items: [
        PopupMenuItem(value: 'rename', enabled: model.canRename(id) && !many, child: const Text('Rename  ↩')),
        PopupMenuItem(
          value: 'duplicate',
          enabled: model.canEdit && !anyDerived,
          child: Text(many ? 'Duplicate Selection  ⌘D' : 'Duplicate  ⌘D'),
        ),
        PopupMenuItem(
          value: 'delete',
          enabled: model.canEdit,
          child: Text(
            many
                ? 'Delete Selection  ⌫'
                : derived
                ? (n.added ? 'Delete  ⌫  (reverts the override that added it)' : 'Delete  ⌫  (as an override)')
                : 'Delete  ⌫',
          ),
        ),
        const PopupMenuDivider(),
        PopupMenuItem(
          value: 'createEmpty',
          enabled: model.canEdit && !many,
          child: Text(derived ? 'Create Empty Child  (as an override)' : 'Create Empty Child'),
        ),
        for (final c in model.nativeComponentTypes)
          if (c.type != 'fynkel.Transform')
            PopupMenuItem(
              value: 'createWith:${c.type}',
              enabled: model.canEdit && !many,
              child: Text('Create Child with ${c.type}'),
            ),
        const PopupMenuDivider(),
        PopupMenuItem(
          value: 'moveToRoot',
          enabled: model.canEdit && !anyDerived && tab.selection.any((s) => tab.tree[s]?.parent != null),
          child: const Text('Move to Root'),
        ),
        PopupMenuItem(
          value: 'toggle',
          enabled: n.children.isNotEmpty || n.isPrefabInstance,
          child: Text(tab.tree.expanded.contains(id) ? 'Collapse' : 'Expand'),
        ),
        if ((n.isPrefabInstance || derived) && prefabCommands.isNotEmpty) ...[
          const PopupMenuDivider(),
          for (final c in prefabCommands)
            PopupMenuItem(
              key: Key('hierarchy.menu.${c.id}'),
              value: 'command:${c.id}',
              enabled: c.isEnabled,
              child: Text('${c.label}${c.shortcut == null ? '' : '  ${CommandRegistry.shortcutLabel(c.shortcut!)}'}'),
            ),
        ],
        const PopupMenuDivider(),
        PopupMenuItem(value: 'copyId', child: Text('Copy id  ${id.length > 13 ? '${id.substring(0, 13)}…' : id}')),
        if (n.isPrefabInstance) PopupMenuItem(value: 'copyPrefab', child: Text('Copy prefab asset id')),
      ],
    );
    switch (choice) {
      case 'rename':
        model.beginRename(id);
      case 'duplicate':
        model.duplicate();
      case 'delete':
        model.delete();
      case 'createEmpty':
        model.createEmpty(parent: id);
      case 'moveToRoot':
        model.moveToRoot();
      case 'toggle':
        model.toggle(id);
      case 'copyId':
        await Clipboard.setData(ClipboardData(text: id));
      case 'copyPrefab':
        await Clipboard.setData(ClipboardData(text: n.prefabAsset ?? ''));
      case null:
        break;
      default:
        if (choice.startsWith('createWith:')) model.createWith(choice.substring('createWith:'.length), parent: id);
        if (choice.startsWith('command:')) widget.commands?.execute(choice.substring('command:'.length));
    }
  }
}

/// The list's background accepts a drop as "append to the roots": entity
/// rows reparent there, a prefab from the Project browser instantiates.
class _RootDropTarget extends StatelessWidget {
  const _RootDropTarget({required this.model, required this.child});

  final HierarchyModel model;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    DropTarget root() => DropTarget(parent: null, index: model.tree?.roots.length ?? 0);
    return DragTarget<AssetDragPayload>(
      onWillAcceptWithDetails: (d) => model.canDropAsset(d.data.toRef(), d.data.kind, root()),
      onAcceptWithDetails: (d) =>
          model.instantiatePrefab(d.data.toRef(), name: prefabInstanceName(d.data.name), target: root()),
      builder: (context, assets, _) => DragTarget<List<String>>(
        onWillAcceptWithDetails: (d) => model.canDrop(d.data, root()),
        onAcceptWithDetails: (d) => model.reparent(d.data, root()),
        builder: (context, candidates, _) => assets.isNotEmpty
            ? DecoratedBox(
                decoration: BoxDecoration(border: Border.all(color: Theme.of(context).colorScheme.primary)),
                child: child,
              )
            : child,
      ),
    );
  }
}

enum _Zone { above, onto, below }

class _HierarchyRowWidget extends StatefulWidget {
  const _HierarchyRowWidget({
    super.key,
    required this.model,
    required this.row,
    required this.selected,
    required this.renaming,
    required this.renameController,
    required this.errors,
    required this.onTap,
    required this.onDoubleTap,
    required this.onSecondaryTap,
    required this.onToggle,
    required this.onRenameSubmit,
    required this.onRenameCancel,
  });

  final HierarchyModel model;
  final HierarchyRow row;
  final bool selected;
  final bool renaming;
  final TextEditingController? renameController;
  final List<Diagnostic> errors;
  final VoidCallback onTap;
  final VoidCallback onDoubleTap;
  final void Function(Offset at) onSecondaryTap;
  final VoidCallback onToggle;
  final void Function(String) onRenameSubmit;
  final VoidCallback onRenameCancel;

  @override
  State<_HierarchyRowWidget> createState() => _HierarchyRowWidgetState();
}

class _HierarchyRowWidgetState extends State<_HierarchyRowWidget> {
  _Zone? _zone;
  final _renameFocus = FocusNode(debugLabel: 'hierarchy.rename');
  bool _submitted = false;

  @override
  void initState() {
    super.initState();
    _renameFocus.addListener(_onRenameFocus);
  }

  @override
  void didUpdateWidget(covariant _HierarchyRowWidget old) {
    super.didUpdateWidget(old);
    if (widget.renaming && !old.renaming) {
      _submitted = false;
      WidgetsBinding.instance.addPostFrameCallback((_) => _renameFocus.requestFocus());
    }
  }

  @override
  void dispose() {
    _renameFocus.removeListener(_onRenameFocus);
    _renameFocus.dispose();
    super.dispose();
  }

  void _onRenameFocus() {
    // Focus leaving the field commits, like the Finder.
    if (!_renameFocus.hasFocus && widget.renaming && !_submitted) {
      _submitted = true;
      widget.onRenameSubmit(widget.renameController?.text ?? '');
    }
  }

  _Zone _zoneAt(Offset global) {
    final box = context.findRenderObject() as RenderBox?;
    if (box == null) return _Zone.onto;
    final y = box.globalToLocal(global).dy / box.size.height;
    // Between two roots of a scene there is no order to drop into (the roots
    // follow the entity key order), so a root row offers no insertion line.
    final orderable = widget.row.node.parent != null || widget.model.rootOrderIsEditable;
    if (y < 0.25) return orderable ? _Zone.above : _Zone.onto;
    if (y > 0.75) {
      if (widget.row.hasChildren && widget.row.expanded) return _Zone.below;
      return orderable ? _Zone.below : _Zone.onto;
    }
    return _Zone.onto;
  }

  DropTarget _target(_Zone z) {
    final tree = widget.model.tree!;
    final n = widget.row.node;
    switch (z) {
      case _Zone.onto:
        return DropTarget(parent: n.id, index: n.children.length);
      case _Zone.above:
        final (_, i) = tree.siblingsOf(n.id);
        return DropTarget(parent: n.parent, index: i);
      case _Zone.below:
        if (widget.row.hasChildren && widget.row.expanded) return DropTarget(parent: n.id, index: 0);
        final (_, i) = tree.siblingsOf(n.id);
        return DropTarget(parent: n.parent, index: i + 1);
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final row = widget.row;
    final n = row.node;
    final model = widget.model;
    final selectionForDrag = model.tab!.isSelected(n.id) ? model.tab!.selection.toList() : [n.id];
    final t = context.tokens;
    final focused = PanelFocus.of(context);
    final onRow = t.onSelectedRow(focused: focused);
    final onRowDim = t.onSelectedRowSecondary(focused: focused);
    final content = Container(
      height: hierarchyRowHeight(context),
      padding: EdgeInsets.only(left: 4.0 + row.depth * 14),
      decoration: BoxDecoration(
        color: widget.selected ? t.selectedRow(focused: focused) : null,
        border: switch (_zone) {
          _Zone.above => Border(top: BorderSide(color: t.accent, width: 2)),
          _Zone.below => Border(bottom: BorderSide(color: t.accent, width: 2)),
          _Zone.onto => Border.all(color: t.accent, width: 1.5),
          null => null,
        },
      ),
      child: Row(
        children: [
          SizedBox(
            width: 16,
            child: row.hasChildren
                ? InkWell(
                    key: Key('hierarchy.expander.${n.id}'),
                    onTap: widget.onToggle,
                    child: Icon(
                      row.expanded ? FIcons.expandMore : FIcons.chevronRight,
                      size: 14,
                      color: widget.selected ? onRowDim : t.textSecondary,
                    ),
                  )
                : null,
          ),
          Icon(
            _iconOf(n),
            size: 13,
            color: widget.selected
                ? onRowDim
                : row.matches
                ? t.textSecondary
                : t.textDisabled,
          ),
          const SizedBox(width: 6),
          Expanded(
            child: widget.renaming
                ? TextField(
                    key: Key('hierarchy.rename.${n.id}'),
                    controller: widget.renameController,
                    focusNode: _renameFocus,
                    autofocus: true,
                    style: theme.textTheme.bodySmall,
                    decoration: const InputDecoration(
                      isDense: true,
                      contentPadding: EdgeInsets.symmetric(horizontal: 4, vertical: 2),
                      border: OutlineInputBorder(),
                    ),
                    onSubmitted: (v) {
                      _submitted = true;
                      widget.onRenameSubmit(v);
                    },
                    onTapOutside: (_) {
                      if (!_submitted) {
                        _submitted = true;
                        widget.onRenameSubmit(widget.renameController?.text ?? '');
                      }
                    },
                  )
                : Text(
                    n.name.isEmpty ? '(unnamed)' : n.name,
                    key: Key('hierarchy.name.${n.id}'),
                    overflow: TextOverflow.ellipsis,
                    style: theme.textTheme.bodySmall?.copyWith(
                      color: widget.selected
                          ? onRow
                          : row.matches
                          ? (n.isDerived && !n.added ? t.textSecondary : t.text)
                          : t.textDisabled,
                      fontStyle: n.name.isEmpty || n.isDerived ? FontStyle.italic : null,
                      fontWeight: n.overriddenFields > 0 ? FontWeight.bold : null,
                    ),
                  ),
          ),
          if (n.isDerived && (n.added || n.overriddenFields > 0))
            Tooltip(
              message: n.added
                  ? 'Added to the instance by an override'
                  : '${n.overriddenFields} overridden field${n.overriddenFields == 1 ? '' : 's'}',
              child: Container(
                key: Key('hierarchy.derived.${n.id}'),
                margin: const EdgeInsets.only(left: 4),
                padding: const EdgeInsets.symmetric(horizontal: 4),
                decoration: BoxDecoration(color: t.lineStrong, borderRadius: t.cornerControl),
                child: Text(
                  n.added ? '+' : '${n.overriddenFields}',
                  style: theme.textTheme.labelSmall?.copyWith(color: theme.colorScheme.onSecondaryContainer),
                ),
              ),
            ),
          if (n.isPrefabInstance)
            Tooltip(
              message:
                  'Prefab instance of ${n.prefabAsset ?? '?'}'
                  '${n.overrideCount > 0 ? ' · ${n.overrideCount} override${n.overrideCount == 1 ? '' : 's'}' : ''}',
              child: Container(
                key: Key('hierarchy.prefab.${n.id}'),
                margin: const EdgeInsets.only(left: 4),
                padding: const EdgeInsets.symmetric(horizontal: 4),
                decoration: BoxDecoration(color: t.surfaceOf(t.accent, .28), borderRadius: t.cornerControl),
                child: Row(
                  children: [
                    Icon(FIcons.widgetsOutlined, size: 10, color: theme.colorScheme.onTertiaryContainer),
                    if (n.overrideCount > 0)
                      Text(
                        ' ${n.overrideCount}',
                        style: theme.textTheme.labelSmall?.copyWith(color: theme.colorScheme.onTertiaryContainer),
                      ),
                  ],
                ),
              ),
            ),
          if (widget.errors.isNotEmpty)
            Tooltip(
              message: widget.errors.map((e) => '${e.code}: ${e.message}').join('\n'),
              child: Padding(
                padding: const EdgeInsets.only(left: 4),
                child: Icon(FIcons.error, key: Key('hierarchy.errorBadge.${n.id}'), size: 13, color: t.error),
              ),
            ),
          const SizedBox(width: 6),
        ],
      ),
    );

    final interactive = GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: widget.onTap,
      onSecondaryTapUp: (d) => widget.onSecondaryTap(d.globalPosition),
      child: content,
    );

    if (widget.renaming) return interactive;

    return DragTarget<AssetDragPayload>(
      onWillAcceptWithDetails: (d) {
        final z = _zoneAt(d.offset);
        final ok = model.canDropAsset(d.data.toRef(), d.data.kind, _target(z));
        setState(() => _zone = ok ? z : null);
        return ok;
      },
      onMove: (d) {
        final z = _zoneAt(d.offset);
        final next = model.canDropAsset(d.data.toRef(), d.data.kind, _target(z)) ? z : null;
        if (next != _zone) setState(() => _zone = next);
      },
      onLeave: (_) => setState(() => _zone = null),
      onAcceptWithDetails: (d) {
        final z = _zoneAt(d.offset);
        setState(() => _zone = null);
        model.instantiatePrefab(d.data.toRef(), name: prefabInstanceName(d.data.name), target: _target(z));
      },
      builder: (context, _, _) => DragTarget<List<String>>(
        onWillAcceptWithDetails: (d) {
          final z = _zoneAt(d.offset);
          final ok = model.canDrop(d.data, _target(z));
          setState(() => _zone = ok ? z : null);
          return ok;
        },
        onMove: (d) {
          final z = _zoneAt(d.offset);
          final ok = model.canDrop(d.data, _target(z));
          final next = ok ? z : null;
          if (next != _zone) setState(() => _zone = next);
        },
        onLeave: (_) => setState(() => _zone = null),
        onAcceptWithDetails: (d) {
          final z = _zoneAt(d.offset);
          setState(() => _zone = null);
          model.reparent(d.data, _target(z));
        },
        builder: (context, _, _) => model.canEdit
            ? Draggable<List<String>>(
                data: selectionForDrag,
                dragAnchorStrategy: pointerDragAnchorStrategy,
                feedback: Material(
                  elevation: 4,
                  borderRadius: BorderRadius.circular(4),
                  child: Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                    child: Text(
                      selectionForDrag.length == 1 ? n.name : '${selectionForDrag.length} entities',
                      style: theme.textTheme.bodySmall,
                    ),
                  ),
                ),
                childWhenDragging: Opacity(opacity: .4, child: content),
                child: interactive,
              )
            : interactive,
      ),
    );
  }

  static IconData _iconOf(HierarchyNode n) {
    if (n.isPrefabInstance) return FIcons.widgetsOutlined;
    if (n.hasComponent('fynkel.Camera')) return FIcons.videocamOutlined;
    if (n.hasComponent('fynkel.Light')) return FIcons.lightbulbOutline;
    if (n.hasComponent('fynkel.MeshRenderer')) return FIcons.viewInArOutlined;
    if (n.hasComponent('fynkel.RigidBody')) return FIcons.sportsBasketballOutlined;
    if (n.hasComponent('fynkel.Collider')) return FIcons.cropSquare;
    if (n.hasComponent('fynkel.Animator')) return FIcons.animation;
    if (n.children.isNotEmpty) return FIcons.folderOutlined;
    return FIcons.circleOutlined;
  }
}
