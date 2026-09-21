// The Project panel (item 5): a folder tree beside the folder's assets, or
// one flat list; search, kind filters, sort; New ▾, the asset history's
// undo/redo; rows drag as `AssetDragPayload` (onto Inspector fields, the
// Hierarchy, a folder here); the context menu, inline rename, delete with
// the "referenced by" dialog; double-click opens documents and shows the
// Asset panel for sources.
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:path/path.dart' as p;

import '../env/tools.dart';
import '../inspector/field_widgets.dart' show AssetDragPayload, assetKindIcon;
import '../project/editor_project.dart';
import 'asset_preview.dart';
import 'project_browser_model.dart';

const projectRowHeight = 26.0;

class ProjectPanel extends StatefulWidget {
  const ProjectPanel({
    super.key,
    required this.model,
    required this.onOpenDocument,
    required this.onShowAsset,
    this.run = runProcess,
    this.searchFocus,
  });

  final ProjectBrowserModel model;

  /// A scene or prefab double-clicked: into a tab.
  final void Function(String id) onOpenDocument;

  /// Any other asset double-clicked: the Asset panel is shown.
  final VoidCallback onShowAsset;
  final RunProcess run;
  final FocusNode? searchFocus;

  @override
  State<ProjectPanel> createState() => _ProjectPanelState();
}

class _ProjectPanelState extends State<ProjectPanel> {
  ProjectBrowserModel get model => widget.model;
  final _search = TextEditingController();
  late FocusNode _searchFocus;
  bool _ownSearchFocus = false;
  final _listFocus = FocusNode(debugLabel: 'project.list');
  int _seenSearchRequests = 0;
  DeleteRefusal? _shownRefusal;
  TextEditingController? _rename;
  String? _renameTarget;
  String? _lastTapId;
  DateTime? _lastTapAt;

  @override
  void initState() {
    super.initState();
    _searchFocus = widget.searchFocus ?? FocusNode(debugLabel: 'project.search');
    _ownSearchFocus = widget.searchFocus == null;
    _search.text = model.query;
    model.addListener(_onModel);
    _seenSearchRequests = model.searchFocusRequests;
  }

  @override
  void didUpdateWidget(covariant ProjectPanel old) {
    super.didUpdateWidget(old);
    if (old.model != widget.model) {
      old.model.removeListener(_onModel);
      widget.model.addListener(_onModel);
    }
  }

  @override
  void dispose() {
    model.removeListener(_onModel);
    _search.dispose();
    if (_ownSearchFocus) _searchFocus.dispose();
    _listFocus.dispose();
    _rename?.dispose();
    super.dispose();
  }

  void _onModel() {
    if (!mounted) return;
    if (model.searchFocusRequests != _seenSearchRequests) {
      _seenSearchRequests = model.searchFocusRequests;
      WidgetsBinding.instance.addPostFrameCallback((_) => _searchFocus.requestFocus());
    }
    if (_search.text != model.query) _search.text = model.query;
    final refusal = model.pendingDelete;
    if (refusal != null && !identical(refusal, _shownRefusal)) {
      _shownRefusal = refusal;
      WidgetsBinding.instance.addPostFrameCallback((_) => _askDeleteAnyway(refusal));
    }
    if (refusal == null) _shownRefusal = null;
    final target = model.renamingAsset ?? (model.renamingFolder == null ? null : 'folder:${model.renamingFolder}');
    if (target != _renameTarget) {
      _renameTarget = target;
      _rename?.dispose();
      _rename = null;
      if (target != null) {
        final text = model.renamingAsset != null
            ? ProjectBrowserModel.editableName(model.entry(model.renamingAsset!)!)
            : p.basename(model.renamingFolder!);
        _rename = TextEditingController(text: text)
          ..selection = TextSelection(baseOffset: 0, extentOffset: text.length);
      } else {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) _listFocus.requestFocus();
        });
      }
    }
    setState(() {});
  }

  // ── Actions ───────────────────────────────────────────────────────────

  void _activate(AssetEntry e) {
    if (e.document == 'scene' || e.document == 'prefab') {
      widget.onOpenDocument(e.id);
    } else {
      model.select(e.id);
      widget.onShowAsset();
    }
  }

  void _tap(AssetEntry e) {
    _listFocus.requestFocus();
    final shift = HardwareKeyboard.instance.isShiftPressed;
    final meta = HardwareKeyboard.instance.isMetaPressed || HardwareKeyboard.instance.isControlPressed;
    final now = DateTime.now();
    final doubled =
        !shift &&
        !meta &&
        _lastTapId == e.id &&
        _lastTapAt != null &&
        now.difference(_lastTapAt!) < const Duration(milliseconds: 400);
    _lastTapId = e.id;
    _lastTapAt = now;
    if (doubled) {
      _lastTapAt = null;
      model.select(e.id);
      _activate(e);
      return;
    }
    model.select(e.id, toggle: meta, range: shift);
  }

  Future<void> _reveal(String rel) async {
    final err = await revealInFileBrowser(model.project.assetFile(rel), run: widget.run);
    if (err != null && mounted) {
      ScaffoldMessenger.maybeOf(context)?.showSnackBar(SnackBar(content: Text('Reveal in Finder failed: $err')));
    }
  }

  Future<void> _askDeleteAnyway(DeleteRefusal refusal) async {
    if (!mounted) return;
    final names = [for (final id in refusal.ids) model.entry(id)?.path ?? id];
    final byDocument = <String, List<Diagnostic>>{};
    for (final d in refusal.dependents) {
      byDocument.putIfAbsent(model.assetPath(d.document ?? '?'), () => []).add(d);
    }
    final force = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        key: const Key('project.deleteRefused'),
        title: Text(names.length == 1 ? 'Delete ${p.basename(names.single)}?' : 'Delete ${names.length} assets?'),
        content: SizedBox(
          width: 480,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                'Still referenced by ${refusal.dependents.length} field${refusal.dependents.length == 1 ? '' : 's'} '
                'in ${byDocument.length} document${byDocument.length == 1 ? '' : 's'}. Deleting anyway leaves those '
                'references dangling — fynkel validate reports each one (FYN-A0004).',
              ),
              const SizedBox(height: 12),
              Flexible(
                child: SingleChildScrollView(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      for (final e in byDocument.entries) ...[
                        Text(e.key, style: Theme.of(context).textTheme.labelLarge),
                        for (final d in e.value)
                          Padding(
                            padding: const EdgeInsets.only(left: 12),
                            child: Text(
                              d.pointer ?? d.message,
                              style: Theme.of(context).textTheme.bodySmall?.copyWith(fontFamily: 'monospace'),
                            ),
                          ),
                      ],
                    ],
                  ),
                ),
              ),
            ],
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Cancel')),
          FilledButton(
            key: const Key('project.deleteAnyway'),
            style: FilledButton.styleFrom(backgroundColor: Theme.of(context).colorScheme.error),
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Delete anyway'),
          ),
        ],
      ),
    );
    if (!mounted) return;
    if (force == true) {
      model.delete(ids: refusal.ids, force: true);
    } else {
      model.cancelDelete();
    }
  }

  Future<void> _moveTo(List<String> ids) async {
    final folders = ['', ...model.folders];
    final chosen = await showDialog<String>(
      context: context,
      builder: (context) => SimpleDialog(
        key: const Key('project.moveTo'),
        title: Text(
          ids.length == 1
              ? 'Move ${p.basename(model.entry(ids.single)?.path ?? '')} to…'
              : 'Move ${ids.length} assets to…',
        ),
        children: [
          for (final f in folders)
            SimpleDialogOption(
              key: Key('project.moveTo.${f.isEmpty ? 'root' : f}'),
              onPressed: () => Navigator.pop(context, f),
              child: Row(
                children: [
                  Icon(f.isEmpty ? Icons.home_outlined : Icons.folder_outlined, size: 16),
                  const SizedBox(width: 8),
                  Text(f.isEmpty ? 'assets (root)' : f),
                ],
              ),
            ),
        ],
      ),
    );
    if (chosen != null) model.move(ids, chosen);
  }

  Future<void> _assetMenu(BuildContext context, AssetEntry e, Offset at) async {
    if (!model.isSelected(e.id)) model.select(e.id);
    final ids = model.selection.toList();
    final many = ids.length > 1;
    final isDoc = e.document == 'scene' || e.document == 'prefab';
    final overlay = Overlay.of(context).context.findRenderObject() as RenderBox;
    final choice = await showMenu<String>(
      context: context,
      position: RelativeRect.fromRect(Rect.fromPoints(at, at), Offset.zero & overlay.size),
      items: [
        if (isDoc) PopupMenuItem(value: 'open', enabled: !many, child: const Text('Open  ⇧⌥⌘O')),
        const PopupMenuItem(value: 'show', child: Text('Show in Asset Panel')),
        const PopupMenuItem(value: 'reveal', child: Text('Reveal in Finder  ⇧⌥⌘E')),
        const PopupMenuDivider(),
        PopupMenuItem(value: 'rename', enabled: model.canEdit && !many, child: const Text('Rename  ⇧⌥⌘R')),
        PopupMenuItem(
          value: 'move',
          enabled: model.canEdit,
          child: Text(many ? 'Move Selection to…  ⇧⌥⌘V' : 'Move to…  ⇧⌥⌘V'),
        ),
        PopupMenuItem(
          value: 'delete',
          enabled: model.canEdit,
          child: Text(many ? 'Delete Selection  ⇧⌥⌘⌫' : 'Delete  ⇧⌥⌘⌫'),
        ),
        if (e.isSource)
          PopupMenuItem(value: 'reimport', enabled: model.canEdit && !many, child: const Text('Reimport  ⇧⌥⌘I')),
        const PopupMenuDivider(),
        PopupMenuItem(value: 'copyId', child: Text('Copy id  ${e.id.substring(0, 13)}…')),
        PopupMenuItem(value: 'copyPath', child: Text('Copy path  ${e.path}')),
        const PopupMenuDivider(),
        PopupMenuItem(value: 'new:scene', enabled: model.canEdit, child: const Text('New Scene Here')),
        PopupMenuItem(value: 'new:prefab', enabled: model.canEdit, child: const Text('New Prefab Here')),
        PopupMenuItem(value: 'new:material', enabled: model.canEdit, child: const Text('New Material Here')),
        PopupMenuItem(value: 'new:folder', enabled: model.canEdit, child: const Text('New Folder Here')),
      ],
    );
    if (!mounted) return;
    final here = e.path.contains('/') ? e.path.substring(0, e.path.lastIndexOf('/')) : '';
    switch (choice) {
      case 'open':
        widget.onOpenDocument(e.id);
      case 'show':
        widget.onShowAsset();
      case 'reveal':
        await _reveal(e.path);
      case 'rename':
        model.beginRename(e.id);
      case 'move':
        await _moveTo(ids);
      case 'delete':
        model.delete(ids: ids);
      case 'reimport':
        final r = model.reimport(e.id);
        if (r != null && !r.ok && mounted) {
          ScaffoldMessenger.maybeOf(this.context)?.showSnackBar(
            SnackBar(content: Text('Reimport refused: ${r.diagnostics.map((d) => d.message).join('; ')}')),
          );
        }
      case 'copyId':
        await Clipboard.setData(ClipboardData(text: e.id));
      case 'copyPath':
        await Clipboard.setData(ClipboardData(text: e.path));
      case 'new:folder':
        model.newFolder(inFolder: here);
      case null:
        break;
      default:
        if (choice.startsWith('new:')) model.newDocument(choice.substring(4), inFolder: here);
    }
  }

  Future<void> _folderMenu(BuildContext context, String rel, Offset at) async {
    final overlay = Overlay.of(context).context.findRenderObject() as RenderBox;
    final isRoot = rel.isEmpty;
    final choice = await showMenu<String>(
      context: context,
      position: RelativeRect.fromRect(Rect.fromPoints(at, at), Offset.zero & overlay.size),
      items: [
        const PopupMenuItem(value: 'open', child: Text('Show Folder')),
        const PopupMenuItem(value: 'reveal', child: Text('Reveal in Finder')),
        const PopupMenuDivider(),
        PopupMenuItem(value: 'new:scene', enabled: model.canEdit, child: const Text('New Scene Here')),
        PopupMenuItem(value: 'new:prefab', enabled: model.canEdit, child: const Text('New Prefab Here')),
        PopupMenuItem(value: 'new:material', enabled: model.canEdit, child: const Text('New Material Here')),
        PopupMenuItem(value: 'new:folder', enabled: model.canEdit, child: const Text('New Folder Here')),
        if (!isRoot) ...[
          const PopupMenuDivider(),
          PopupMenuItem(value: 'rename', enabled: model.canEdit, child: const Text('Rename Folder')),
          PopupMenuItem(value: 'delete', enabled: model.canEdit, child: const Text('Delete Empty Folder')),
        ],
      ],
    );
    if (!mounted) return;
    switch (choice) {
      case 'open':
        model.setFolder(rel);
      case 'reveal':
        await _reveal(rel);
      case 'rename':
        model.beginRenameFolder(rel);
      case 'delete':
        model.deleteFolder(rel);
      case 'new:folder':
        model.newFolder(inFolder: rel);
      case null:
        break;
      default:
        if (choice.startsWith('new:')) model.newDocument(choice.substring(4), inFolder: rel);
    }
  }

  // ── Keys ──────────────────────────────────────────────────────────────

  KeyEventResult _onKey(FocusNode node, KeyEvent e) {
    if (e is! KeyDownEvent && e is! KeyRepeatEvent) return KeyEventResult.ignored;
    final k = e.logicalKey;
    if (model.renamingAsset != null || model.renamingFolder != null) {
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
    } else if (k == LogicalKeyboardKey.enter) {
      final lead = model.leadEntry;
      if (lead != null) _activate(lead);
    } else if (k == LogicalKeyboardKey.f2) {
      model.beginRename();
    } else if ((k == LogicalKeyboardKey.backspace || k == LogicalKeyboardKey.delete) && !meta) {
      if (model.selection.isNotEmpty) model.delete();
    } else if (k == LogicalKeyboardKey.keyA && meta) {
      model.selectAll();
    } else if (k == LogicalKeyboardKey.escape) {
      if (model.filtering) {
        _search.clear();
        model.clearFilters();
      } else {
        model.clearSelection();
      }
    } else if (k == LogicalKeyboardKey.arrowLeft && model.view == BrowserView.folders && !model.filtering) {
      model.goUp();
    } else {
      return KeyEventResult.ignored;
    }
    return KeyEventResult.handled;
  }

  // ── Build ─────────────────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final folders = model.visibleFolders;
    final assets = model.visibleAssets;
    return Column(
      children: [
        _toolbar(context, theme),
        const Divider(height: 1),
        Expanded(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (model.view == BrowserView.folders) ...[
                SizedBox(width: 180, child: _tree(theme)),
                const VerticalDivider(width: 1),
              ],
              Expanded(
                child: Focus(
                  focusNode: _listFocus,
                  onKeyEvent: _onKey,
                  child: GestureDetector(
                    behavior: HitTestBehavior.translucent,
                    // The empty-space tap has no semantic meaning; with it in the semantics tree,
                    // rows leaving the list while a menu route pops trip the framework's
                    // "node.built" assertion (the kind filter did exactly that).
                    excludeFromSemantics: true,
                    onTap: () {
                      _listFocus.requestFocus();
                      model.clearSelection();
                    },
                    onSecondaryTapUp: (d) => _folderMenu(context, model.folder, d.globalPosition),
                    child: folders.isEmpty && assets.isEmpty
                        ? Center(
                            child: Text(
                              model.filtering
                                  ? 'Nothing matches.'
                                  : 'This folder is empty. Use New ▾ or drop files here.',
                              key: const Key('project.noRows'),
                              textAlign: TextAlign.center,
                              style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
                            ),
                          )
                        : ListView.builder(
                            key: const Key('project.list'),
                            itemCount: folders.length + assets.length,
                            itemExtent: projectRowHeight,
                            itemBuilder: (context, i) => i < folders.length
                                ? _folderRow(context, theme, folders[i], inList: true)
                                : _assetRow(context, theme, assets[i - folders.length]),
                          ),
                  ),
                ),
              ),
            ],
          ),
        ),
        if (model.lastError != null) _errorBar(theme, model.lastError!),
        const Divider(height: 1),
        _status(theme, assets.length),
      ],
    );
  }

  Widget _toolbar(BuildContext context, ThemeData theme) {
    final history = model.history;
    final kinds = model.kinds;
    final counts = model.countsByKind;
    return Padding(
      padding: const EdgeInsets.fromLTRB(4, 4, 4, 4),
      child: Row(
        children: [
          SegmentedButton<BrowserView>(
            key: const Key('project.view'),
            showSelectedIcon: false,
            style: const ButtonStyle(
              visualDensity: VisualDensity.compact,
              padding: WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 6)),
            ),
            segments: const [
              ButtonSegment(
                value: BrowserView.folders,
                icon: Icon(Icons.folder_outlined, size: 16),
                tooltip: 'Folders',
              ),
              ButtonSegment(
                value: BrowserView.list,
                icon: Icon(Icons.view_list_outlined, size: 16),
                tooltip: 'Flat list',
              ),
            ],
            selected: {model.view},
            onSelectionChanged: (s) => model.setView(s.first),
          ),
          const SizedBox(width: 6),
          Expanded(
            child: TextField(
              key: const Key('project.search'),
              controller: _search,
              focusNode: _searchFocus,
              decoration: InputDecoration(
                isDense: true,
                prefixIcon: const Icon(Icons.search, size: 16),
                hintText: model.view == BrowserView.folders && model.folder.isNotEmpty
                    ? 'Search under ${model.folder}…'
                    : 'Search name, path, id, sub-asset…',
                border: const OutlineInputBorder(),
                contentPadding: const EdgeInsets.symmetric(vertical: 6),
                suffixIcon: model.query.isEmpty
                    ? null
                    : IconButton(
                        tooltip: 'Clear',
                        icon: const Icon(Icons.close, size: 14),
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
            isLabelVisible: kinds.isNotEmpty,
            label: Text('${kinds.length}'),
            child: PopupMenuButton<String>(
              key: const Key('project.filter'),
              tooltip: 'Show only these kinds',
              icon: Icon(Icons.filter_list, size: 18, color: kinds.isNotEmpty ? theme.colorScheme.primary : null),
              itemBuilder: (context) => [
                for (final k in browserKinds)
                  CheckedPopupMenuItem(
                    key: Key('project.filter.$k'),
                    value: k,
                    checked: kinds.contains(k),
                    child: Text('$k  ${counts[k] ?? 0}'),
                  ),
                if (kinds.isNotEmpty) ...[
                  const PopupMenuDivider(),
                  const PopupMenuItem(value: 'clear', child: Text('Show every kind')),
                ],
              ],
              onSelected: (v) {
                if (v == 'clear') {
                  kinds.clear();
                  model.clearFilters();
                  _search.clear();
                } else {
                  model.toggleKind(v);
                }
              },
            ),
          ),
          PopupMenuButton<AssetSort>(
            key: const Key('project.sort'),
            tooltip: 'Sort (again to flip)',
            icon: Icon(model.descending ? Icons.arrow_downward : Icons.arrow_upward, size: 18),
            itemBuilder: (context) => [
              for (final s in AssetSort.values)
                CheckedPopupMenuItem(
                  key: Key('project.sort.${s.name}'),
                  value: s,
                  checked: model.sort == s,
                  child: Text('By ${s.name}'),
                ),
            ],
            onSelected: (s) => model.setSort(s),
          ),
          PopupMenuButton<String>(
            key: const Key('project.new'),
            tooltip: 'New scene, prefab, material or folder in the shown folder',
            icon: const Icon(Icons.add, size: 18),
            enabled: model.canEdit,
            itemBuilder: (context) => [
              PopupMenuItem(
                key: const Key('project.new.scene'),
                value: 'scene',
                child: ListTile(dense: true, leading: Icon(assetKindIcon('scene')), title: const Text('New Scene')),
              ),
              PopupMenuItem(
                key: const Key('project.new.prefab'),
                value: 'prefab',
                child: ListTile(dense: true, leading: Icon(assetKindIcon('prefab')), title: const Text('New Prefab')),
              ),
              PopupMenuItem(
                key: const Key('project.new.material'),
                value: 'material',
                child: ListTile(
                  dense: true,
                  leading: Icon(assetKindIcon('material')),
                  title: const Text('New Material'),
                ),
              ),
              const PopupMenuDivider(),
              const PopupMenuItem(
                key: Key('project.new.folder'),
                value: 'folder',
                child: ListTile(
                  dense: true,
                  leading: Icon(Icons.create_new_folder_outlined),
                  title: Text('New Folder'),
                ),
              ),
            ],
            onSelected: (v) => v == 'folder' ? model.newFolder() : model.newDocument(v),
          ),
          IconButton(
            key: const Key('project.undo'),
            tooltip: history.canUndo ? 'Undo ${history.undoLabel}  ⌥⌘Z' : 'Nothing to undo',
            icon: const Icon(Icons.undo, size: 18),
            onPressed: history.canUndo && model.canEdit ? model.undo : null,
          ),
          IconButton(
            key: const Key('project.redo'),
            tooltip: history.canRedo ? 'Redo ${history.redoLabel}  ⇧⌥⌘Z' : 'Nothing to redo',
            icon: const Icon(Icons.redo, size: 18),
            onPressed: history.canRedo && model.canEdit ? model.redo : null,
          ),
        ],
      ),
    );
  }

  Widget _tree(ThemeData theme) {
    final folders = model.folders;
    return ListView(
      key: const Key('project.tree'),
      itemExtent: projectRowHeight,
      children: [
        _folderRow(context, theme, '', inList: false),
        for (final f in folders) _folderRow(context, theme, f, inList: false),
      ],
    );
  }

  Widget _folderRow(BuildContext context, ThemeData theme, String rel, {required bool inList}) {
    final depth = inList || rel.isEmpty ? 0 : rel.split('/').length;
    final name = rel.isEmpty ? 'assets' : p.basename(rel);
    final current = !inList && rel == model.folder;
    final renaming = model.renamingFolder == rel && rel.isNotEmpty;
    return _FolderRow(
      key: Key('project.folder.${inList ? 'row' : 'tree'}.${rel.isEmpty ? 'root' : rel}'),
      model: model,
      rel: rel,
      name: name,
      depth: depth,
      current: current,
      renaming: renaming,
      renameController: renaming ? _rename : null,
      onTap: () {
        _listFocus.requestFocus();
        model.setFolder(rel);
      },
      onSecondaryTap: (at) => _folderMenu(context, rel, at),
      onRenameSubmit: (text) => model.renameFolder(rel, text),
    );
  }

  Widget _assetRow(BuildContext context, ThemeData theme, AssetEntry e) {
    final renaming = model.renamingAsset == e.id;
    return _AssetRow(
      key: Key('project.asset.${e.id}'),
      model: model,
      entry: e,
      selected: model.isSelected(e.id),
      lead: model.lead == e.id,
      showPath: model.view == BrowserView.list || model.filtering,
      renaming: renaming,
      renameController: renaming ? _rename : null,
      onTap: () => _tap(e),
      onSecondaryTap: (at) => _assetMenu(context, e, at),
      onRenameSubmit: (text) => model.rename(e.id, text),
      onRenameCancel: model.cancelRename,
    );
  }

  Widget _errorBar(ThemeData theme, String message) {
    return Container(
      key: const Key('project.error'),
      color: theme.colorScheme.errorContainer,
      padding: const EdgeInsets.fromLTRB(8, 4, 4, 4),
      child: Row(
        children: [
          Icon(Icons.error_outline, size: 16, color: theme.colorScheme.onErrorContainer),
          const SizedBox(width: 6),
          Expanded(
            child: Text(
              message,
              style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onErrorContainer),
              maxLines: 3,
              overflow: TextOverflow.ellipsis,
            ),
          ),
          IconButton(tooltip: 'Dismiss', icon: const Icon(Icons.close, size: 14), onPressed: model.clearError),
        ],
      ),
    );
  }

  Widget _status(ThemeData theme, int shown) {
    final total = model.entries.length;
    final selected = model.selection.length;
    final where = model.view == BrowserView.list
        ? 'every folder'
        : model.folder.isEmpty
        ? 'assets'
        : 'assets/${model.folder}';
    final text = [
      shown == total ? '$total asset${total == 1 ? '' : 's'}' : '$shown of $total assets',
      'in $where',
      if (selected > 0) '· $selected selected',
    ].join(' ');
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
      child: Row(
        children: [
          if (model.view == BrowserView.folders && model.folder.isNotEmpty)
            InkWell(
              key: const Key('project.up'),
              onTap: model.goUp,
              child: const Padding(padding: EdgeInsets.only(right: 6), child: Icon(Icons.arrow_upward, size: 14)),
            ),
          Expanded(
            child: Text(
              text,
              key: const Key('project.status'),
              style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
              overflow: TextOverflow.ellipsis,
            ),
          ),
        ],
      ),
    );
  }
}

// ── Rows ────────────────────────────────────────────────────────────────

/// A folder in the tree or in the list: a drop target for asset rows
/// (the whole selection moves when the dragged asset is selected).
class _FolderRow extends StatefulWidget {
  const _FolderRow({
    super.key,
    required this.model,
    required this.rel,
    required this.name,
    required this.depth,
    required this.current,
    required this.renaming,
    required this.renameController,
    required this.onTap,
    required this.onSecondaryTap,
    required this.onRenameSubmit,
  });

  final ProjectBrowserModel model;
  final String rel;
  final String name;
  final int depth;
  final bool current;
  final bool renaming;
  final TextEditingController? renameController;
  final VoidCallback onTap;
  final void Function(Offset at) onSecondaryTap;
  final void Function(String text) onRenameSubmit;

  @override
  State<_FolderRow> createState() => _FolderRowState();
}

class _FolderRowState extends State<_FolderRow> {
  bool _hot = false;

  List<String> _dragged(AssetDragPayload d) => widget.model.isSelected(d.id) ? widget.model.selection.toList() : [d.id];

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final content = Container(
      height: projectRowHeight,
      padding: EdgeInsets.only(left: 6.0 + widget.depth * 14),
      decoration: BoxDecoration(
        color: widget.current
            ? theme.colorScheme.primaryContainer.withValues(alpha: .7)
            : (_hot ? theme.colorScheme.primary.withValues(alpha: .15) : null),
        border: _hot ? Border.all(color: theme.colorScheme.primary) : null,
      ),
      child: Row(
        children: [
          Icon(
            widget.rel.isEmpty ? Icons.home_outlined : (widget.current ? Icons.folder_open : Icons.folder_outlined),
            size: 16,
            color: theme.colorScheme.primary,
          ),
          const SizedBox(width: 6),
          Expanded(
            child: widget.renaming
                ? _InlineRename(
                    key: Key('project.renameFolder.${widget.rel}'),
                    controller: widget.renameController!,
                    onSubmit: widget.onRenameSubmit,
                    onCancel: widget.model.cancelRename,
                  )
                : Text(widget.name, style: theme.textTheme.bodyMedium, overflow: TextOverflow.ellipsis),
          ),
        ],
      ),
    );
    if (widget.renaming) return content;
    return DragTarget<AssetDragPayload>(
      onWillAcceptWithDetails: (d) {
        final ok = widget.model.canMove(_dragged(d.data), widget.rel);
        if (ok != _hot) setState(() => _hot = ok);
        return ok;
      },
      onLeave: (_) => setState(() => _hot = false),
      onAcceptWithDetails: (d) {
        setState(() => _hot = false);
        widget.model.move(_dragged(d.data), widget.rel);
      },
      builder: (context, _, _) => GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        onSecondaryTapUp: (d) => widget.onSecondaryTap(d.globalPosition),
        child: content,
      ),
    );
  }
}

class _AssetRow extends StatelessWidget {
  const _AssetRow({
    super.key,
    required this.model,
    required this.entry,
    required this.selected,
    required this.lead,
    required this.showPath,
    required this.renaming,
    required this.renameController,
    required this.onTap,
    required this.onSecondaryTap,
    required this.onRenameSubmit,
    required this.onRenameCancel,
  });

  final ProjectBrowserModel model;
  final AssetEntry entry;
  final bool selected;
  final bool lead;
  final bool showPath;
  final bool renaming;
  final TextEditingController? renameController;
  final VoidCallback onTap;
  final void Function(Offset at) onSecondaryTap;
  final void Function(String text) onRenameSubmit;
  final VoidCallback onRenameCancel;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final e = entry;
    final file = p.basename(e.path);
    final kind = browserKindOf(e);
    final stem = ProjectBrowserModel.editableName(e);
    final showName = !e.isSource && e.name.isNotEmpty && e.name != stem;
    final content = Container(
      height: projectRowHeight,
      padding: const EdgeInsets.only(left: 6),
      color: selected ? theme.colorScheme.primaryContainer.withValues(alpha: lead ? 1 : .6) : null,
      child: Row(
        children: [
          AssetPreviewWidget(preview: model.preview(e), size: 20),
          const SizedBox(width: 8),
          if (renaming)
            Expanded(
              child: _InlineRename(
                key: Key('project.rename.${e.id}'),
                controller: renameController!,
                onSubmit: onRenameSubmit,
                onCancel: onRenameCancel,
              ),
            )
          else ...[
            Flexible(
              flex: 3,
              child: Text(file, style: theme.textTheme.bodyMedium, overflow: TextOverflow.ellipsis),
            ),
            if (showName) ...[
              const SizedBox(width: 8),
              Flexible(
                flex: 2,
                child: Text(
                  '“${e.name}”',
                  style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ],
            if (showPath && e.path.contains('/')) ...[
              const SizedBox(width: 8),
              Flexible(
                flex: 3,
                child: Text(
                  e.path.substring(0, e.path.lastIndexOf('/')),
                  style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ],
            const Spacer(),
            if (e.isSource && e.subAssets.isNotEmpty)
              Tooltip(
                message: e.subAssets.keys.join(', '),
                child: Text(
                  '${e.subAssets.length} sub',
                  style: theme.textTheme.labelSmall?.copyWith(color: theme.hintColor),
                ),
              ),
            const SizedBox(width: 8),
            Text(kind, style: theme.textTheme.labelSmall?.copyWith(color: theme.hintColor)),
            const SizedBox(width: 8),
          ],
        ],
      ),
    );
    final interactive = GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: onTap,
      onSecondaryTapUp: (d) => onSecondaryTap(d.globalPosition),
      child: content,
    );
    if (renaming) return interactive;
    final payload = model.dragPayload(e);
    if (payload == null) return interactive;
    return Draggable<AssetDragPayload>(
      data: payload,
      dragAnchorStrategy: pointerDragAnchorStrategy,
      feedback: Material(
        elevation: 4,
        borderRadius: BorderRadius.circular(4),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(assetKindIcon(payload.kind), size: 14),
              const SizedBox(width: 6),
              Text(
                selected && model.selection.length > 1 ? '${model.selection.length} assets' : payload.name,
                style: theme.textTheme.bodySmall,
              ),
            ],
          ),
        ),
      ),
      childWhenDragging: Opacity(opacity: .4, child: content),
      child: interactive,
    );
  }
}

/// The name field of a row being renamed: ↩ commits, ⎋ cancels, losing
/// focus commits.
class _InlineRename extends StatefulWidget {
  const _InlineRename({super.key, required this.controller, required this.onSubmit, required this.onCancel});

  final TextEditingController controller;
  final void Function(String text) onSubmit;
  final VoidCallback onCancel;

  @override
  State<_InlineRename> createState() => _InlineRenameState();
}

class _InlineRenameState extends State<_InlineRename> {
  final _focus = FocusNode(debugLabel: 'project.rename');
  bool _done = false;

  @override
  void initState() {
    super.initState();
    _focus.addListener(_onFocus);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _focus.requestFocus();
    });
  }

  @override
  void dispose() {
    _focus.removeListener(_onFocus);
    _focus.dispose();
    super.dispose();
  }

  void _onFocus() {
    if (!_focus.hasFocus && !_done) {
      _done = true;
      widget.onSubmit(widget.controller.text);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Focus(
      onKeyEvent: (_, e) {
        if (e is KeyDownEvent && e.logicalKey == LogicalKeyboardKey.escape) {
          _done = true;
          widget.onCancel();
          return KeyEventResult.handled;
        }
        return KeyEventResult.ignored;
      },
      child: TextField(
        controller: widget.controller,
        focusNode: _focus,
        autofocus: true,
        style: Theme.of(context).textTheme.bodyMedium,
        decoration: const InputDecoration(
          isDense: true,
          contentPadding: EdgeInsets.symmetric(horizontal: 4, vertical: 2),
        ),
        onSubmitted: (t) {
          _done = true;
          widget.onSubmit(t);
        },
      ),
    );
  }
}
