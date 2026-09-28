// The Hierarchy panel: a virtualised tree of the focused document's
// entities (one `ListView.builder` over the tree's cached rows, a fixed row
// height, so 10 000 entities scroll without building more than the visible
// rows), search and filter, select / multi-select / range, rename in place,
// the context menu, drag-and-drop with above / onto / below zones, prefab
// and error badges, and the keyboard (arrows, ↩ / F2 rename, ⌫ delete, ⌘A).
// With several scenes open each is a section under its header (Unity's
// multi-scene Hierarchy): the focused one is the active scene, a click in
// another activates it, rows dragged onto another scene move there.
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../commands/commands.dart';
import '../extensions/editor_extensions.dart' show ContextEntry, EditorExtensionScope;
import '../documents/open_document.dart';
import '../l10n/strings.dart';
import '../inspector/field_widgets.dart' show AssetDragPayload, assetKindIcon;
import '../play/play_document.dart' show PlayDocument;
import '../theme/cascading_menu.dart';
import '../theme/icons.dart';
import '../theme/lock_button.dart';
import '../theme/panel_focus.dart';
import '../theme/tokens.dart';
import 'hierarchy_model.dart';
import 'hierarchy_tree.dart';
import 'presets.dart';

/// One tree row, from the density preference (item 9a) — the scroll math,
/// the list's `itemExtent` and the row itself all measure with this.
double hierarchyRowHeight(BuildContext context) => context.tokens.row;

class HierarchyPanel extends StatefulWidget {
  const HierarchyPanel({
    super.key,
    required this.model,
    required this.onOpenDocument,
    this.searchFocus,
    this.commands,
    this.onSaveScene,
    this.onRemoveScene,
    this.onAddScene,
    this.onSelectSceneAsset,
  });

  final HierarchyModel model;

  /// The empty state's "Open Document…".
  final VoidCallback onOpenDocument;
  final FocusNode? searchFocus;

  /// The registry the context menu's Prefab mode items run through
  /// (`prefab.*` — the same commands as the Edit menu and the palette).
  final CommandRegistry? commands;

  /// A scene header's Save Scene, Remove Scene (asks about unsaved
  /// changes), a scene asset dropped on the Hierarchy (Unity's additive
  /// open) and Select Scene Asset.
  final void Function(OpenDocument scene)? onSaveScene;
  final void Function(OpenDocument scene)? onRemoveScene;
  final void Function(String sceneAsset)? onAddScene;
  final void Function(OpenDocument scene)? onSelectSceneAsset;

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
  int _renameStarted = 0;
  DateTime? _lastTap;
  String? _lastTapId;
  String? _wasRenaming;

  /// Scene sections folded under their header, by document id.
  final Set<String> _folded = {};

  /// What the list shows: the rows alone, or with several scenes each
  /// scene's header and (loaded, unfolded) its rows.
  List<_Item> _items = const [];

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
    final pinged = model.pinged;
    if (pinged != null && model.pingRequests != _seenPings) {
      _seenPings = model.pingRequests;
      WidgetsBinding.instance.addPostFrameCallback((_) => _scrollTo(pinged));
    }
    if (mounted) setState(() {});
  }

  int _seenPings = 0;

  void _scrollTo(String id) {
    final t = model.tree;
    final tab = model.tab;
    if (t == null || tab == null || !_scroll.hasClients) return;
    final int i;
    if (model.multiScene) {
      final r = t.rowIndexOf(id);
      final header = _items.indexWhere((x) => x.row == null && identical(x.scene, tab));
      i = r < 0 || header < 0 || _folded.contains(tab.document.id) ? -1 : header + 1 + r;
    } else {
      i = t.rowIndexOf(id);
    }
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
    PanelFocus.primary(context, _listFocus);
    final theme = Theme.of(context);
    final t = context.tokens;
    final tab = model.tab;
    if (tab == null) return _empty(context, theme);
    final tree = tab.tree;
    final rows = tree.rows;
    final multi = model.multiScene;
    _items = multi
        ? [
            for (final s in model.scenes) ...[
              _Item(s),
              if (model.isLoaded(s) && !_folded.contains(s.document.id))
                for (final r in s.tree.rows) _Item(s, r),
            ],
          ]
        : [for (final r in rows) _Item(tab, r)];
    return Column(
      children: [
        _toolbar(context, theme),
        // Prefab mode (Unity's): the prefab's name under a ‹ back.
        if (tab.kind == 'prefab' && !identical(tab, model.tabs.live))
          Container(
            key: const Key('hierarchy.prefabHeader'),
            color: t.header,
            height: t.row + 2,
            padding: EdgeInsets.only(right: t.pad),
            child: Row(
              children: [
                IconButton(
                  key: const Key('hierarchy.prefab.back'),
                  tooltip: 'Back from Prefab Mode',
                  iconSize: 14,
                  visualDensity: VisualDensity.compact,
                  icon: const Icon(FIcons.chevronLeft),
                  onPressed: () => widget.commands?.execute('prefab.back'),
                ),
                Icon(assetKindIcon('prefab'), size: 13, color: t.textSecondary),
                const SizedBox(width: 4),
                Expanded(
                  child: Text(
                    tab.title,
                    overflow: TextOverflow.ellipsis,
                    style: theme.textTheme.bodySmall?.copyWith(color: t.text, fontWeight: FontWeight.w600),
                  ),
                ),
              ],
            ),
          ),
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
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Playing — the running game, as it is now',
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.bodySmall?.copyWith(color: t.accent),
                ),
                // What was deleted during Play goes on Stop unless applied.
                if (model.deletionsToApply case final names when names.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(top: 4, bottom: 2),
                    child: Row(
                      children: [
                        Expanded(
                          child: Tooltip(
                            message: names.join('\n'),
                            child: Text(
                              names.length == 1 ? 'Deleted: ${names.single}' : '${names.length} objects deleted',
                              key: const Key('hierarchy.deletions'),
                              overflow: TextOverflow.ellipsis,
                              style: theme.textTheme.bodySmall,
                            ),
                          ),
                        ),
                        FilledButton.tonal(
                          key: const Key('hierarchy.applyDeletions'),
                          style: FilledButton.styleFrom(
                            padding: EdgeInsets.symmetric(horizontal: t.pad),
                            minimumSize: const Size(0, 24),
                            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                            visualDensity: VisualDensity.compact,
                          ),
                          onPressed: model.applyDeletionsToScene,
                          child: Text('Apply to Scene', style: theme.textTheme.bodySmall),
                        ),
                      ],
                    ),
                  ),
              ],
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
          child: rows.isEmpty && !multi
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
                    // The empty space's click is the mouse's; the rows are the screen reader's.
                    excludeFromSemantics: true,
                    behavior: HitTestBehavior.translucent,
                    onTap: () {
                      _listFocus.requestFocus();
                      model.clearSelection();
                    },
                    onSecondaryTapUp: (d) => _emptySpaceMenu(context, d.globalPosition),
                    child: _RootDropTarget(
                      model: model,
                      onAddScene: widget.onAddScene,
                      child: ListView.builder(
                        key: const Key('hierarchy.list'),
                        controller: _scroll,
                        itemCount: _items.length,
                        itemExtent: hierarchyRowHeight(context),
                        itemBuilder: (context, i) => _item(context, theme, _items[i]),
                      ),
                    ),
                  ),
                ),
        ),
        if (tab.lastError != null) _errorBar(context, theme, tab.lastError!),
        // Unity has no footer: this one shows only what a search or a
        // setting hides (docs/ui-refresh).
        if (!tree.filter.isEmpty || !model.keepWorldTransform) _status(context, theme, tree, tab.selection.length),
      ],
    );
  }

  // No document: one quiet line and a quiet way to open one (docs/ui-refresh).
  Widget _empty(BuildContext context, ThemeData theme) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text('No document open', style: theme.textTheme.bodySmall?.copyWith(color: context.tokens.textDisabled)),
            const SizedBox(height: 4),
            TextButton(
              key: const Key('hierarchy.openDocument'),
              onPressed: widget.onOpenDocument,
              child: const Text('Open Document…'),
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
      padding: const EdgeInsets.fromLTRB(3, 3, 3, 3),
      child: Row(
        children: [
          // Unity's + first, then the search.
          Builder(
            builder: (context) => IconButton(
              key: const Key('hierarchy.create'),
              tooltip: 'Create an entity (under the selection when one is selected)',
              icon: const Icon(FIcons.add),
              onPressed: model.canEdit
                  ? () {
                      final box = context.findRenderObject()! as RenderBox;
                      showCascadingMenu(context, box.localToGlobal(box.size.bottomLeft(Offset.zero)), [
                        MenuAction(
                          'Create Empty',
                          key: const Key('hierarchy.create.empty'),
                          onSelected: () => model.createEmpty(),
                        ),
                        const MenuSeparator(),
                        ..._createLines('hierarchy.create', enabled: true, underSelection: true),
                      ]);
                    }
                  : null,
            ),
          ),
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

  // ── Scenes ────────────────────────────────────────────────────────────

  Widget _item(BuildContext context, ThemeData theme, _Item item) {
    final r = item.row;
    if (r == null) return _sceneHeader(context, theme, item.scene);
    if (identical(item.scene, model.tab)) return _row(context, theme, item.scene.tree, r);
    return _otherRow(context, theme, item.scene, r);
  }

  Widget _sceneHeader(BuildContext context, ThemeData theme, OpenDocument scene) {
    final t = context.tokens;
    final id = scene.document.id;
    final active = identical(scene, model.tab);
    final loaded = model.isLoaded(scene);
    final folded = _folded.contains(id);
    final header = GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: () {
        _listFocus.requestFocus();
        if (active) {
          model.clearSelection();
        } else if (loaded) {
          model.activateScene(scene);
        }
      },
      onSecondaryTapUp: (d) => _sceneMenu(context, scene, d.globalPosition),
      child: Container(
        key: Key('hierarchy.scene.$id'),
        height: hierarchyRowHeight(context),
        padding: const EdgeInsets.only(left: 4),
        decoration: BoxDecoration(
          color: t.header,
          border: Border(bottom: t.hairline),
        ),
        child: Row(
          children: [
            SizedBox(
              width: 16,
              child: loaded
                  ? Semantics(
                      button: true,
                      label: folded ? 'Expand' : 'Collapse',
                      child: InkWell(
                        key: Key('hierarchy.scene.fold.$id'),
                        onTap: () => setState(() => folded ? _folded.remove(id) : _folded.add(id)),
                        child: Icon(folded ? FIcons.chevronRight : FIcons.expandMore, size: 14, color: t.textSecondary),
                      ),
                    )
                  : null,
            ),
            Icon(assetKindIcon('scene'), size: 13, color: loaded ? t.textSecondary : t.textDisabled),
            const SizedBox(width: 6),
            Expanded(
              child: Text(
                '${scene.title}${scene.dirty ? '*' : ''}${loaded ? '' : '  (not loaded)'}',
                key: Key('hierarchy.scene.name.$id'),
                overflow: TextOverflow.ellipsis,
                style: theme.textTheme.bodySmall?.copyWith(
                  color: loaded ? t.text : t.textDisabled,
                  fontWeight: active ? FontWeight.w700 : FontWeight.w500,
                ),
              ),
            ),
            Builder(
              builder: (context) => IconButton(
                key: Key('hierarchy.scene.menu.$id'),
                tooltip: 'Scene',
                iconSize: 14,
                visualDensity: VisualDensity.compact,
                icon: const Icon(FIcons.moreVert),
                onPressed: () {
                  final box = context.findRenderObject() as RenderBox;
                  _sceneMenu(context, scene, box.localToGlobal(box.size.bottomLeft(Offset.zero)));
                },
              ),
            ),
          ],
        ),
      ),
    );
    return Semantics(
      header: true,
      label: '${scene.title} scene${active ? ', active' : ''}${loaded ? '' : ', not loaded'}',
      child: _SceneDropTarget(model: model, scene: scene, onAddScene: widget.onAddScene, child: header),
    );
  }

  /// Unity's scene header menu.
  Future<void> _sceneMenu(BuildContext context, OpenDocument scene, Offset at) async {
    final active = identical(scene, model.tab);
    final loaded = model.isLoaded(scene);
    final key = 'hierarchy.scene.${scene.document.id}';
    await showCascadingMenu(context, at, [
      MenuAction(
        'Set Active Scene',
        key: Key('$key.setActive'),
        enabled: !active && loaded,
        onSelected: () => model.activateScene(scene),
      ),
      const MenuSeparator(),
      MenuAction(
        'Save Scene',
        key: Key('$key.save'),
        enabled: loaded && scene.dirty && widget.onSaveScene != null,
        onSelected: () => widget.onSaveScene?.call(scene),
      ),
      const MenuSeparator(),
      if (loaded)
        MenuAction(
          'Unload Scene',
          key: Key('$key.unload'),
          enabled: model.tabs.cannotUnload(scene) == null,
          onSelected: () => model.unloadScene(scene),
        )
      else
        MenuAction('Load Scene', key: Key('$key.load'), onSelected: () => model.loadScene(scene)),
      MenuAction(
        'Remove Scene',
        key: Key('$key.remove'),
        enabled: model.scenes.length > 1 && widget.onRemoveScene != null,
        onSelected: () => widget.onRemoveScene?.call(scene),
      ),
      const MenuSeparator(),
      MenuAction(
        'Select Scene Asset',
        key: Key('$key.selectAsset'),
        enabled: widget.onSelectSceneAsset != null,
        onSelected: () => widget.onSelectSceneAsset?.call(scene),
      ),
    ]);
  }

  /// Another loaded scene's row: drawn like the active scene's; a click
  /// makes its scene the active one and selects it there (Unity's).
  Widget _otherRow(BuildContext context, ThemeData theme, OpenDocument scene, HierarchyRow row) {
    final n = row.node;
    void activate() {
      _listFocus.requestFocus();
      model.activateScene(scene);
      model.select(n.id);
    }

    return _HierarchyRowWidget(
      key: ValueKey(model.tab!.tree[n.id] == null ? 'row.${n.id}' : 'row.${scene.document.id}.${n.id}'),
      model: model,
      row: row,
      scene: scene,
      selected: false,
      inactive: !scene.tree.activeInHierarchy(n.id),
      renaming: false,
      renameController: null,
      errors: scene.tree.errors[n.id] ?? const [],
      onTap: activate,
      onDoubleTap: _frame,
      onSecondaryTap: (at) {
        activate();
        _contextMenu(context, n.id, at);
      },
      onToggle: () => model.toggle(n.id, recursive: HardwareKeyboard.instance.isAltPressed, scene: scene),
      onRenameSubmit: (_) {},
      onRenameCancel: () {},
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
      pinged: model.pinged == n.id,
      inactive: !tree.activeInHierarchy(n.id),
      renaming: renaming,
      renameController: renaming ? _renameController(n) : null,
      errors: tree.errors[n.id] ?? const [],
      onTap: () => _tapRow(n.id),
      onDoubleTap: _frame,
      onSecondaryTap: (at) => _contextMenu(context, n.id, at),
      onToggle: () => model.toggle(n.id, recursive: HardwareKeyboard.instance.isAltPressed),
      onRenameSubmit: (v) => model.rename(n.id, v),
      onRenameCancel: model.cancelRename,
      // Unity's › at an instance's row end: Prefab Mode in Context.
      onOpenPrefab: n.isPrefabInstance && model.tab?.expansionOf(n.id) != null
          ? () {
              model.select(n.id);
              widget.commands?.execute('prefab.open');
            }
          : null,
    );
  }

  void _frame() => widget.commands?.execute('view.frameSelected');

  TextEditingController _renameController(HierarchyNode n) {
    // A new rename starts from the name, not from text an Esc abandoned.
    if (_renameFor != n.id || _renameStarted != model.renameStarted) {
      _renameStarted = model.renameStarted;
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
    if (isDouble && model.tab!.isSelected(id)) {
      // Unity: a double-click frames the object in the Scene view.
      _frame();
      return;
    }
    model.select(
      id,
      toggle: HardwareKeyboard.instance.isMetaPressed || HardwareKeyboard.instance.isControlPressed,
      range: HardwareKeyboard.instance.isShiftPressed,
    );
  }

  /// Unity's right-click on the Hierarchy's empty space: create at the
  /// root, paste, expand or collapse everything.
  Future<void> _emptySpaceMenu(BuildContext context, Offset at) async {
    final tab = model.tab;
    if (tab == null) return;
    _listFocus.requestFocus();
    model.clearSelection();
    await showCascadingMenu(context, at, [
      MenuAction(
        'Paste  ⌘V',
        key: const Key('hierarchy.empty.paste'),
        enabled: model.canPaste,
        onSelected: () => model.paste(),
      ),
      const MenuSeparator(),
      MenuAction(
        'Create Empty  ⇧⌘N',
        key: const Key('hierarchy.empty.createEmpty'),
        enabled: model.canEdit,
        onSelected: () => model.createEmpty(underSelection: false),
      ),
      ..._createLines('hierarchy.empty', enabled: model.canEdit, underSelection: false),
      const MenuSeparator(),
      MenuAction('Expand All', onSelected: model.expandAll),
      MenuAction('Collapse All', onSelected: model.collapseAll),
    ]);
  }

  /// Unity's 3D Object ▸ / Light ▸ / Camera and the component picker —
  /// the + menu's and both context menus' create items (ADR-0023).
  List<MenuLine> _createLines(String keyPrefix, {required bool enabled, required bool underSelection}) {
    MenuAction preset(GameObjectPreset p) => MenuAction(
      p.label,
      key: Key('$keyPrefix.${p.name}'),
      enabled: enabled,
      onSelected: () => model.createPreset(p, underSelection: underSelection),
    );
    return [
      MenuSubmenu(EditorStrings.menu3dObject, key: Key('$keyPrefix.3dObject'), [
        for (final p in GameObjectPreset.of(PresetGroup.object3d)) preset(p),
      ]),
      MenuSubmenu(EditorStrings.menuLight, key: Key('$keyPrefix.light'), [
        for (final p in GameObjectPreset.of(PresetGroup.light)) preset(p),
      ]),
      for (final p in GameObjectPreset.of(PresetGroup.camera)) preset(p),
      MenuAction(
        'Create with Component…',
        key: Key('$keyPrefix.createWith'),
        enabled: enabled && widget.commands?.byId('hierarchy.createWith') != null,
        onSelected: () => widget.commands?.execute('hierarchy.createWith'),
      ),
    ];
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
        'prefab.unpackCompletely',
        'prefab.open',
        'prefab.selectAsset',
      ])
        ?widget.commands?.byId(cid),
    ];
    final extensionEntries = EditorExtensionScope.maybeOf(context)?.hierarchyEntries(tab) ?? const <ContextEntry>[];
    await showCascadingMenu(context, at, [
      MenuAction('Cut  ⌘X', enabled: model.canEdit && !anyDerived, onSelected: model.cut),
      MenuAction('Copy  ⌘C', enabled: !anyDerived, onSelected: model.copy),
      MenuAction('Paste  ⌘V', enabled: model.canPaste, onSelected: () => model.paste()),
      MenuAction('Paste As Child  ⇧⌘V', enabled: model.canPaste && !many, onSelected: () => model.paste(asChild: true)),
      const MenuSeparator(),
      MenuAction('Rename  ↩', enabled: model.canRename(id) && !many, onSelected: () => model.beginRename(id)),
      MenuAction(
        many ? 'Duplicate Selection  ⌘D' : 'Duplicate  ⌘D',
        enabled: model.canEdit && !anyDerived,
        onSelected: model.duplicate,
      ),
      MenuAction(
        many
            ? 'Delete Selection  ⌫'
            : derived
            ? (n.added ? 'Delete  ⌫  (reverts the override that added it)' : 'Delete  ⌫  (as an override)')
            : 'Delete  ⌫',
        enabled: model.canEdit,
        onSelected: model.delete,
      ),
      const MenuSeparator(),
      MenuAction(
        derived ? 'Create Empty Child  (as an override)' : 'Create Empty Child',
        enabled: model.canEdit && !many,
        onSelected: () => model.createEmpty(parent: id),
      ),
      // The row is the selection, so a created object goes under it.
      ..._createLines('hierarchy.menu', enabled: model.canEdit && !many, underSelection: true),
      const MenuSeparator(),
      MenuAction(
        'Move to Root',
        enabled: model.canEdit && !anyDerived && tab.selection.any((s) => tab.tree[s]?.parent != null),
        onSelected: model.moveToRoot,
      ),
      MenuAction(
        tab.tree.expanded.contains(id) ? 'Collapse' : 'Expand',
        enabled: n.children.isNotEmpty || n.isPrefabInstance,
        onSelected: () => model.toggle(id),
      ),
      if ((n.isPrefabInstance || derived) && prefabCommands.isNotEmpty) ...[
        const MenuSeparator(),
        for (final c in prefabCommands)
          MenuAction(
            '${c.label}${c.shortcut == null ? '' : '  ${CommandRegistry.shortcutLabel(c.shortcut!)}'}',
            key: Key('hierarchy.menu.${c.id}'),
            enabled: c.isEnabled,
            onSelected: () => widget.commands?.execute(c.id),
          ),
      ],
      if (model.showsPlay) ...[
        const MenuSeparator(),
        MenuAction(
          many ? 'Apply Selection to Scene' : 'Apply to Scene',
          key: const Key('hierarchy.menu.applyToScene'),
          onSelected: model.applyToScene,
        ),
      ],
      // The extensions' entries (ADR-0028), for the selection.
      if (extensionEntries.isNotEmpty) ...[
        const MenuSeparator(),
        for (final e in extensionEntries) MenuAction(e.label, key: e.key, enabled: e.enabled, onSelected: e.run),
      ],
      const MenuSeparator(),
      MenuAction(
        'Copy id  ${id.length > 13 ? '${id.substring(0, 13)}…' : id}',
        onSelected: () => Clipboard.setData(ClipboardData(text: id)),
      ),
      if (n.isPrefabInstance)
        MenuAction(
          'Copy prefab asset id',
          onSelected: () => Clipboard.setData(ClipboardData(text: n.prefabAsset ?? '')),
        ),
    ]);
  }
}

/// The list's background accepts a drop as "append to the roots": entity
/// rows reparent there, a prefab from the Project browser instantiates.
class _RootDropTarget extends StatelessWidget {
  const _RootDropTarget({required this.model, required this.child, this.onAddScene});

  final HierarchyModel model;
  final Widget child;

  /// A scene from the Project: added to the open scenes (Unity's drop of a
  /// scene onto the Hierarchy).
  final void Function(String sceneAsset)? onAddScene;

  @override
  Widget build(BuildContext context) {
    DropTarget root() => DropTarget(parent: null, index: model.tree?.roots.length ?? 0);
    return DragTarget<AssetDragPayload>(
      onWillAcceptWithDetails: (d) => d.data.kind == 'scene'
          ? onAddScene != null && model.multiScene
          : model.canDropAsset(d.data.toRef(), d.data.kind, root()),
      onAcceptWithDetails: (d) => d.data.kind == 'scene'
          ? onAddScene?.call(d.data.id)
          : model.instantiatePrefab(d.data.toRef(), name: prefabInstanceName(d.data.name), target: root()),
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

/// A scene's header takes the active scene's rows dragged onto it (they
/// move to that scene), a scene from the Project (added) and a prefab
/// (instantiated at that scene's roots).
class _SceneDropTarget extends StatefulWidget {
  const _SceneDropTarget({required this.model, required this.scene, required this.child, this.onAddScene});

  final HierarchyModel model;
  final OpenDocument scene;
  final Widget child;
  final void Function(String sceneAsset)? onAddScene;

  @override
  State<_SceneDropTarget> createState() => _SceneDropTargetState();
}

class _SceneDropTargetState extends State<_SceneDropTarget> {
  bool _over = false;

  HierarchyModel get model => widget.model;
  OpenDocument get scene => widget.scene;
  bool get _active => identical(scene, model.tab);

  bool _acceptsAsset(AssetDragPayload a) {
    if (a.kind == 'scene') return widget.onAddScene != null;
    if (!model.isLoaded(scene)) return false;
    final root = DropTarget(parent: null, index: scene.tree.roots.length);
    return _active && model.canDropAsset(a.toRef(), a.kind, root) || !_active && a.kind == 'prefab';
  }

  bool _acceptsRows(List<String> ids) => _active
      ? model.canDrop(ids, DropTarget(parent: null, index: scene.tree.roots.length))
      : model.cannotMoveToScene(ids, scene) == null;

  void _setOver(bool v) {
    if (v != _over) setState(() => _over = v);
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return DragTarget<AssetDragPayload>(
      onWillAcceptWithDetails: (d) {
        final ok = _acceptsAsset(d.data);
        _setOver(ok);
        return ok;
      },
      onLeave: (_) => _setOver(false),
      onAcceptWithDetails: (d) {
        _setOver(false);
        final a = d.data;
        if (a.kind == 'scene') {
          widget.onAddScene?.call(a.id);
          return;
        }
        if (!_active) model.activateScene(scene);
        model.instantiatePrefab(
          a.toRef(),
          name: prefabInstanceName(a.name),
          target: DropTarget(parent: null, index: scene.tree.roots.length),
        );
      },
      builder: (context, _, _) => DragTarget<List<String>>(
        onWillAcceptWithDetails: (d) {
          final ok = _acceptsRows(d.data);
          _setOver(ok);
          return ok;
        },
        onLeave: (_) => _setOver(false),
        onAcceptWithDetails: (d) {
          _setOver(false);
          if (_active) {
            model.reparent(d.data, DropTarget(parent: null, index: scene.tree.roots.length));
          } else {
            model.moveToScene(d.data, scene);
          }
        },
        builder: (context, _, _) => _over
            ? DecoratedBox(
                position: DecorationPosition.foreground,
                decoration: BoxDecoration(border: Border.all(color: t.accent, width: 1.5)),
                child: widget.child,
              )
            : widget.child,
      ),
    );
  }
}

/// One line of the list: a scene's header ([row] null) or one of its rows.
class _Item {
  const _Item(this.scene, [this.row]);

  final OpenDocument scene;
  final HierarchyRow? row;
}

enum _Zone { above, onto, below }

class _HierarchyRowWidget extends StatefulWidget {
  const _HierarchyRowWidget({
    super.key,
    required this.model,
    required this.row,
    this.scene,
    required this.selected,
    this.pinged = false,
    this.inactive = false,
    required this.renaming,
    required this.renameController,
    required this.errors,
    required this.onTap,
    required this.onDoubleTap,
    required this.onSecondaryTap,
    required this.onToggle,
    required this.onRenameSubmit,
    required this.onRenameCancel,
    this.onOpenPrefab,
  });

  final HierarchyModel model;
  final HierarchyRow row;

  /// Another loaded scene the row is in (not the active one): it offers a
  /// drop of the active scene's rows (they move there), not a drag.
  final OpenDocument? scene;
  final bool selected;

  /// Outlined by an object field's ping (Unity's), not selected.
  final bool pinged;

  /// Not active in the hierarchy: the row is greyed, as in Unity.
  final bool inactive;
  final bool renaming;
  final TextEditingController? renameController;
  final List<Diagnostic> errors;
  final VoidCallback onTap;
  final VoidCallback onDoubleTap;
  final void Function(Offset at) onSecondaryTap;
  final VoidCallback onToggle;
  final void Function(String) onRenameSubmit;
  final VoidCallback onRenameCancel;

  /// A prefab instance's ›: its prefab opened in context.
  final VoidCallback? onOpenPrefab;

  @override
  State<_HierarchyRowWidget> createState() => _HierarchyRowWidgetState();
}

class _HierarchyRowWidgetState extends State<_HierarchyRowWidget> {
  _Zone? _zone;
  bool _hover = false;
  final _renameFocus = FocusNode(debugLabel: 'hierarchy.rename');
  bool _submitted = false;

  @override
  void initState() {
    super.initState();
    _renameFocus.addListener(_onRenameFocus);
    // A row made renaming (a new child under an expanded parent): take the
    // focus after the menu that made it has put its own back.
    if (widget.renaming) WidgetsBinding.instance.addPostFrameCallback((_) => _renameFocus.requestFocus());
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
    final tree = widget.scene?.tree ?? widget.model.tree!;
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
    final other = widget.scene;
    final selectionForDrag = model.tab!.isSelected(n.id) ? model.tab!.selection.toList() : [n.id];
    final t = context.tokens;
    final focused = PanelFocus.of(context);
    final onRow = t.onSelectedRow(focused: focused);
    final onRowDim = t.onSelectedRowSecondary(focused: focused);
    final content = Container(
      key: widget.pinged ? Key('hierarchy.pinged.${n.id}') : null,
      height: hierarchyRowHeight(context),
      padding: EdgeInsets.only(left: 4.0 + row.depth * 14),
      decoration: BoxDecoration(
        color: widget.selected
            ? t.selectedRow(focused: focused)
            : (widget.pinged ? t.surfaceOf(t.accent, .18) : null),
        border: switch (_zone) {
          _Zone.above => Border(top: BorderSide(color: t.accent, width: 2)),
          _Zone.below => Border(bottom: BorderSide(color: t.accent, width: 2)),
          _Zone.onto => Border.all(color: t.accent, width: 1.5),
          null => widget.pinged ? Border.all(color: t.accent, width: 1.5) : null,
        },
      ),
      child: Row(
        children: [
          SizedBox(
            width: 16,
            child: row.hasChildren
                ? Semantics(
                    button: true,
                    label: row.expanded ? 'Collapse' : 'Expand',
                    child: InkWell(
                      key: Key('hierarchy.expander.${n.id}'),
                      onTap: widget.onToggle,
                      child: Icon(
                        row.expanded ? FIcons.expandMore : FIcons.chevronRight,
                        size: 14,
                        color: widget.selected ? onRowDim : t.textSecondary,
                      ),
                    ),
                  )
                : null,
          ),
          _prefabTip(
            n,
            Icon(
              _iconOf(n),
              size: 13,
              color: widget.selected
                  ? onRowDim
                  : row.matches && !widget.inactive
                  ? (n.isPrefabInstance ? t.prefab : t.textSecondary)
                  : t.textDisabled,
            ),
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
                          ? (widget.inactive ? onRowDim : onRow)
                          : row.matches && !widget.inactive
                          ? (n.isPrefabInstance
                                ? t.prefab
                                : n.isDerived && !n.added
                                ? t.textSecondary
                                : t.text)
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
          if (widget.onOpenPrefab != null)
            Tooltip(
              message: 'Open the prefab in context',
              waitDuration: const Duration(milliseconds: 600),
              // Unity's: the › shows under the pointer (and on the selection).
              child: Opacity(
                opacity: _hover || widget.selected ? 1 : 0,
                child: InkWell(
                  key: Key('hierarchy.openPrefab.${n.id}'),
                  onTap: widget.onOpenPrefab,
                  child: Padding(
                    padding: const EdgeInsets.only(left: 4),
                    child: Icon(FIcons.chevronRight, size: 14, color: widget.selected ? onRowDim : t.textSecondary),
                  ),
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

    // A screen reader hears the row's name, whether it is selected, and
    // whether its children are showing.
    final interactive = Semantics(
      selected: widget.selected,
      expanded: row.hasChildren ? row.expanded : null,
      child: MouseRegion(
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onTap,
          onSecondaryTapUp: (d) => widget.onSecondaryTap(d.globalPosition),
          child: content,
        ),
      ),
    );

    if (widget.renaming) return interactive;

    // Another scene's row: the active scene's rows dropped onto it (or
    // beside it) move to its scene there.
    if (other != null) {
      bool ok(DropTarget at, List<String> ids) => model.cannotMoveToScene(ids, other, parent: at.parent) == null;
      return DragTarget<List<String>>(
        onWillAcceptWithDetails: (d) {
          final z = _zoneAt(d.offset);
          final accepted = ok(_target(z), d.data);
          setState(() => _zone = accepted ? z : null);
          return accepted;
        },
        onMove: (d) {
          final z = _zoneAt(d.offset);
          final next = ok(_target(z), d.data) ? z : null;
          if (next != _zone) setState(() => _zone = next);
        },
        onLeave: (_) => setState(() => _zone = null),
        onAcceptWithDetails: (d) {
          final at = _target(_zoneAt(d.offset));
          setState(() => _zone = null);
          model.moveToScene(d.data, other, parent: at.parent, index: at.parent == null ? null : at.index);
        },
        builder: (context, _, _) => interactive,
      );
    }

    // A material or a script goes onto the object (Unity's material and
    // script drops), anywhere on the row.
    _Zone? assetZone(DragTargetDetails<AssetDragPayload> d) {
      if (d.data.kind == 'material') return model.canAssignMaterial(d.data.kind, row.id) ? _Zone.onto : null;
      if (d.data.kind == 'script') return model.canAddScript(d.data.id, row.id) ? _Zone.onto : null;
      if (d.data.kind == 'controller') return model.canAssignController(d.data.kind, row.id) ? _Zone.onto : null;
      final z = _zoneAt(d.offset);
      return model.canDropAsset(d.data.toRef(), d.data.kind, _target(z)) ? z : null;
    }

    return DragTarget<AssetDragPayload>(
      onWillAcceptWithDetails: (d) {
        final z = assetZone(d);
        setState(() => _zone = z);
        return z != null;
      },
      onMove: (d) {
        final next = assetZone(d);
        if (next != _zone) setState(() => _zone = next);
      },
      onLeave: (_) => setState(() => _zone = null),
      onAcceptWithDetails: (d) {
        setState(() => _zone = null);
        if (d.data.kind == 'material') {
          model.assignMaterial(d.data.toRef(), row.id);
          return;
        }
        if (d.data.kind == 'script') {
          model.addScript(d.data.id, row.id);
          return;
        }
        if (d.data.kind == 'controller') {
          model.assignController(d.data.toRef(), row.id);
          return;
        }
        final z = _zoneAt(d.offset);
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

  /// A prefab instance says so in its icon's tooltip (Unity draws no
  /// badge: the blue name is the mark).
  Widget _prefabTip(HierarchyNode n, Widget icon) {
    if (!n.isPrefabInstance) return icon;
    return Tooltip(
      key: Key('hierarchy.prefab.${n.id}'),
      message:
          'Prefab instance of ${n.prefabAsset ?? '?'}'
          '${n.overrideCount > 0 ? ' · ${n.overrideCount} override${n.overrideCount == 1 ? '' : 's'}' : ''}',
      child: icon,
    );
  }

  static IconData _iconOf(HierarchyNode n) {
    if (n.isPrefabInstance) return FIcons.widgetsOutlined;
    if (n.hasComponent('fynkel.Camera')) return FIcons.videocamOutlined;
    if (n.hasComponent('fynkel.DirectionalLight') ||
        n.hasComponent('fynkel.PointLight') ||
        n.hasComponent('fynkel.SpotLight')) {
      return FIcons.lightbulbOutline;
    }
    if (n.hasComponent('fynkel.MeshRenderer')) return FIcons.viewInArOutlined;
    if (n.hasComponent('fynkel.RigidBody')) return FIcons.sportsBasketballOutlined;
    if (n.hasComponent('fynkel.Collider')) return FIcons.cropSquare;
    if (n.hasComponent('fynkel.Animator')) return FIcons.animation;
    if (n.children.isNotEmpty) return FIcons.folderOutlined;
    return FIcons.circleOutlined;
  }
}
