// The Project panel (item 5): a folder tree beside the folder's assets, or
// one flat list; search, kind filters, sort; New ▾, the asset history's
// undo/redo; rows drag as `AssetDragPayload` (onto Inspector fields, the
// Hierarchy, a folder here); the context menu, inline rename, delete with
// the "referenced by" dialog; double-click opens documents and shows any
// other asset in the Inspector (item 9c). Below the assets, the Scripts
// root lists `lib/`: a double-click opens a script in the IDE, and New ▾ →
// Behaviour makes one from a template (roadmap Phase 2 item 1).
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:path/path.dart' as p;

import '../extensions/editor_extensions.dart' show ContextEntry, EditorExtensionScope;
import '../env/tools.dart';
import '../inspector/field_widgets.dart' show AssetDragPayload, assetKindIcon;
import '../project/editor_project.dart';
import '../shell/file_drops.dart';
import '../shell/notices.dart';
import '../inspector/inspector_chrome.dart' show InspectorCaption;
import '../inspector/inspector_panel.dart' show materialIconNamed;
import '../theme/icons.dart';
import '../theme/lock_button.dart';
import '../theme/panel_focus.dart';
import 'asset_preview.dart';
import '../theme/tokens.dart';
import 'project_browser_model.dart';
import 'scripts.dart';

/// One browser row, from the density preference (item 9a).
double projectRowHeight(BuildContext context) => context.tokens.row + 2;

class ProjectPanel extends StatefulWidget {
  const ProjectPanel({
    super.key,
    required this.model,
    required this.onOpenDocument,
    this.onOpenScene,
    required this.onShowAsset,
    this.onFindReferences,
    this.run = runProcess,
    this.searchFocus,
    this.onOpenScript,
    this.generateBehaviours,
  });

  final ProjectBrowserModel model;

  /// A scene or prefab double-clicked: into a tab.
  final void Function(String id) onOpenDocument;

  /// A scene's Open (single: the other scenes close) and Open Scene
  /// Additive (added to the open scenes) — Unity's; without it a scene
  /// opens like any document.
  final void Function(String id, {bool additive})? onOpenScene;

  /// Any other asset double-clicked (or Show in Inspector): the Inspector
  /// shows it and comes forward.
  final VoidCallback onShowAsset;

  /// Find References In Scene: the focused document's Hierarchy searches
  /// for the objects that reference the asset.
  final void Function(String asset, String name)? onFindReferences;
  final RunProcess run;
  final FocusNode? searchFocus;

  /// A script (absolute path) double-clicked: into the IDE. Returns the
  /// problem, or null.
  final Future<String?> Function(String file)? onOpenScript;

  /// `fynkel codegen` for New Behaviour (returns the problem, or null);
  /// null leaves New Behaviour off.
  final Future<String?> Function()? generateBehaviours;

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
  final _listScroll = ScrollController();
  int _seenScrollRequests = 0;
  List<String>? _shownConfirm;
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
    FileDrops.instance.addTaker(_takeFiles);
    FileDrops.instance.hover.addListener(_onFilesHover);
  }

  /// A drag from Finder is over this panel (the list is outlined).
  bool _filesOver = false;

  bool _contains(Offset global) {
    final box = context.findRenderObject();
    if (box is! RenderBox || !box.attached || !box.hasSize) return false;
    return (box.localToGlobal(Offset.zero) & box.size).contains(global);
  }

  void _onFilesHover() {
    final at = FileDrops.instance.hover.value;
    final over = mounted && at != null && model.canEdit && _contains(at);
    if (over != _filesOver) setState(() => _filesOver = over);
  }

  /// Files dropped from Finder on this panel: imported into the shown folder.
  bool _takeFiles(FileDrop drop) {
    if (!mounted || !_contains(drop.position)) return false;
    if (_filesOver) setState(() => _filesOver = false);
    if (model.canEdit) model.importFiles(drop.paths);
    return true;
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
    FileDrops.instance.removeTaker(_takeFiles);
    FileDrops.instance.hover.removeListener(_onFilesHover);
    _search.dispose();
    if (_ownSearchFocus) _searchFocus.dispose();
    _listFocus.dispose();
    _listScroll.dispose();
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
    if (model.scrollRequests != _seenScrollRequests) {
      _seenScrollRequests = model.scrollRequests;
      WidgetsBinding.instance.addPostFrameCallback((_) => _scrollToShown());
    }
    final refusal = model.pendingDelete;
    if (refusal != null && !identical(refusal, _shownRefusal)) {
      _shownRefusal = refusal;
      WidgetsBinding.instance.addPostFrameCallback((_) => _askDeleteAnyway(refusal));
    }
    if (refusal == null) _shownRefusal = null;
    final confirm = model.pendingConfirm;
    if (confirm != null && !identical(confirm, _shownConfirm)) {
      _shownConfirm = confirm;
      WidgetsBinding.instance.addPostFrameCallback((_) => _askDelete(confirm));
    }
    if (confirm == null) _shownConfirm = null;
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

  /// The pinged asset (else the lead, its part) scrolled into the list's
  /// or the grid's view.
  void _scrollToShown() {
    if (!mounted || !_listScroll.hasClients) return;
    final id = model.pinged ?? model.lead;
    final part = model.pinged == null ? model.leadPart : null;
    final i = _rows().indexWhere(
      (r) => switch (r) {
        final AssetEntry a => a.id == id && part == null,
        (final AssetEntry a, final String sub) => a.id == id && sub == part,
        _ => false,
      },
    );
    if (i < 0) return;
    final grid = model.grid;
    final row = grid ? _tileExtent(model.tileSize) : projectRowHeight(context);
    final top = grid ? _gridPad + (i ~/ _gridColumns) * row : i * row;
    final pos = _listScroll.position;
    if (top < pos.pixels || top + row > pos.pixels + pos.viewportDimension) {
      _listScroll.jumpTo((top - pos.viewportDimension / 2 + row / 2).clamp(0, pos.maxScrollExtent));
    }
  }

  /// What the list (or the grid) shows, in order: folders, then each asset
  /// with an expanded model's parts after it (Unity's ▸).
  List<Object> _rows() => [
    ...model.visibleFolders,
    for (final a in model.visibleAssets) ...[a, for (final sub in model.partsOf(a)) (a, sub)],
  ];

  /// The grid's columns at its last layout.
  int _gridColumns = 1;

  /// An arrow key in the grid: the lead [delta] tiles on (a row is
  /// [_gridColumns]), over assets and parts; folders are not selected.
  void _gridStep(int delta, {required bool extend}) {
    final rows = _rows();
    bool pickable(Object r) => r is! String;
    final i = rows.indexWhere(
      (r) => switch (r) {
        final AssetEntry a => a.id == model.lead && model.leadPart == null,
        (final AssetEntry a, final String sub) => a.id == model.lead && model.leadPart == sub,
        _ => false,
      },
    );
    var j = i < 0 ? rows.indexWhere(pickable) : (i + delta).clamp(0, rows.length - 1);
    while (j >= 0 && j < rows.length && !pickable(rows[j])) {
      j += delta.sign;
    }
    if (j < 0 || j >= rows.length || j == i) return;
    switch (rows[j]) {
      case final AssetEntry a:
        extend ? model.extendTo(a.id) : model.select(a.id);
      case (final AssetEntry a, final String sub):
        model.selectPart(a.id, sub);
    }
    WidgetsBinding.instance.addPostFrameCallback((_) => _scrollToShown());
  }

  // ── Actions ───────────────────────────────────────────────────────────

  void _activate(AssetEntry e) {
    if (e.document == 'scene' && widget.onOpenScene != null) {
      widget.onOpenScene!(e.id);
    } else if (e.document == 'scene' || e.document == 'prefab') {
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

  Future<void> _openScript(String rel) async {
    model.selectScript(rel);
    final open = widget.onOpenScript;
    if (open == null) return;
    final err = await open(model.scriptFile(rel));
    if (err != null && mounted) Notices.maybeOf(context)?.show('Open in IDE failed: $err');
  }

  void _tapScript(String rel) {
    _listFocus.requestFocus();
    final now = DateTime.now();
    final doubled =
        _lastTapId == 'script:$rel' &&
        _lastTapAt != null &&
        now.difference(_lastTapAt!) < const Duration(milliseconds: 400);
    _lastTapId = 'script:$rel';
    _lastTapAt = now;
    if (doubled) {
      _lastTapAt = null;
      _openScript(rel);
      return;
    }
    model.selectScript(rel);
  }

  bool get _canNewBehaviour => widget.generateBehaviours != null && model.canNewBehaviour;

  /// New ▾ → Behaviour: asks for the class name, writes the file from the
  /// template, opens it in the IDE, then runs `fynkel codegen` and
  /// registers the type.
  Future<void> _newBehaviour({String? inFolder}) async {
    final generate = widget.generateBehaviours;
    if (generate == null || !model.canNewBehaviour) return;
    final folder = inFolder ?? model.newBehaviourFolder;
    final name = await showDialog<String>(
      animationStyle: AnimationStyle.noAnimation,
      context: context,
      builder: (context) => _NewBehaviourDialog(folder: folder),
    );
    if (name == null || !mounted) return;
    final b = model.writeNewBehaviour(name, inFolder: folder);
    if (b == null) return;
    await _openScript(b.rel);
    final problem = await model.generateBehaviour(b, generate);
    if (problem == null && mounted) {
      Notices.maybeOf(context)?.show('${b.id} is ready: Add Component lists it.');
    }
  }

  Future<void> _scriptMenu(BuildContext context, String rel, Offset at, {required bool folder}) async {
    if (!folder) model.selectScript(rel);
    final overlay = Overlay.of(context).context.findRenderObject() as RenderBox;
    final choice = await showMenu<String>(
      popUpAnimationStyle: AnimationStyle.noAnimation,
      context: context,
      position: RelativeRect.fromRect(Rect.fromPoints(at, at), Offset.zero & overlay.size),
      items: [
        if (folder)
          const PopupMenuItem(value: 'open', child: Text('Show Folder'))
        else
          PopupMenuItem(value: 'open', enabled: widget.onOpenScript != null, child: const Text('Open in IDE')),
        const PopupMenuItem(value: 'reveal', child: Text('Reveal in Finder')),
        PopupMenuItem(value: 'copyPath', child: Text('Copy path  lib/$rel')),
        const PopupMenuDivider(),
        PopupMenuItem(value: 'behaviour', enabled: _canNewBehaviour, child: const Text('New Behaviour Here…')),
      ],
    );
    if (!mounted) return;
    final here = folder ? rel : (rel.contains('/') ? rel.substring(0, rel.lastIndexOf('/')) : '');
    switch (choice) {
      case 'open':
        folder ? model.showScripts(rel) : await _openScript(rel);
      case 'reveal':
        final err = await revealInFileBrowser(rel.isEmpty ? model.scriptsDir : model.scriptFile(rel), run: widget.run);
        if (err != null && mounted) Notices.maybeOf(this.context)?.show('Reveal in Finder failed: $err');
      case 'copyPath':
        await Clipboard.setData(ClipboardData(text: 'lib/$rel'));
      case 'behaviour':
        await _newBehaviour(inFolder: here);
    }
  }

  Future<void> _reveal(String rel) async {
    final err = await revealInFileBrowser(model.project.assetFile(rel), run: widget.run);
    if (err != null && mounted) {
      Notices.maybeOf(context)?.show('Reveal in Finder failed: $err');
    }
  }

  /// Unity's Import Package (ADR-0029): asks first, as the copies keep the
  /// package's ids and take over every reference to its assets.
  Future<void> _importPackageAssets(String package) async {
    final yes = await showDialog<bool>(
      animationStyle: AnimationStyle.noAnimation,
      context: context,
      builder: (context) => AlertDialog(
        key: const Key('project.importPackageConfirm'),
        title: Text('Import $package assets?'),
        content: Text(
          'Every asset of $package is copied into assets/$package with its id: '
          'scenes and prefabs then use the copies, which you can change. '
          'Removing the copies gives the package\'s back.',
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Cancel')),
          FilledButton(
            key: const Key('project.importPackageConfirm.yes'),
            autofocus: true,
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Import'),
          ),
        ],
      ),
    );
    if (yes == true && mounted) model.importPackageAssets(package);
  }

  /// Unity's "Delete selected asset?" — Enter deletes, Esc keeps.
  Future<void> _askDelete(List<String> ids) async {
    if (!mounted) return;
    final names = [for (final id in ids) model.entry(id)?.path ?? id];
    final yes = await showDialog<bool>(
      animationStyle: AnimationStyle.noAnimation,
      context: context,
      builder: (context) => AlertDialog(
        key: const Key('project.deleteConfirm'),
        title: Text(names.length == 1 ? 'Delete selected asset?' : 'Delete ${names.length} selected assets?'),
        content: SizedBox(
          width: 420,
          child: Text(
            '${names.take(8).join('\n')}${names.length > 8 ? '\n…' : ''}\n\n'
            'The files move to .fynkel/trash; Undo Asset Operation (⌥⌘Z) brings them back.',
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Cancel')),
          FilledButton(
            key: const Key('project.deleteConfirm.delete'),
            autofocus: true,
            style: FilledButton.styleFrom(backgroundColor: Theme.of(context).colorScheme.error),
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (!mounted) return;
    model.answerDelete(yes == true);
  }

  Future<void> _askDeleteAnyway(DeleteRefusal refusal) async {
    if (!mounted) return;
    final names = [for (final id in refusal.ids) model.entry(id)?.path ?? id];
    final byDocument = <String, List<Diagnostic>>{};
    for (final d in refusal.dependents) {
      byDocument.putIfAbsent(model.assetPath(d.document ?? '?'), () => []).add(d);
    }
    final force = await showDialog<bool>(
      animationStyle: AnimationStyle.noAnimation,
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
      animationStyle: AnimationStyle.noAnimation,
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
                  Icon(f.isEmpty ? FIcons.homeOutlined : FIcons.folderOutlined, size: 16),
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
    // A package's assets stay as the package has them (ADR-0029).
    final editable = ids.every((id) => model.entry(id) != null && model.canEditEntry(model.entry(id)!));
    final here = e.path.contains('/') ? e.path.substring(0, e.path.lastIndexOf('/')) : '';
    final canHere = model.canEditFolder(here);
    final overlay = Overlay.of(context).context.findRenderObject() as RenderBox;
    // The extensions' entries (ADR-0028), for the selected assets.
    final extensionEntries = EditorExtensionScope.maybeOf(context)?.projectEntries(ids) ?? const <ContextEntry>[];
    final choice = await showMenu<String>(
      popUpAnimationStyle: AnimationStyle.noAnimation,
      context: context,
      position: RelativeRect.fromRect(Rect.fromPoints(at, at), Offset.zero & overlay.size),
      items: [
        if (isDoc) PopupMenuItem(value: 'open', enabled: !many, child: const Text('Open  ⇧⌥⌘O')),
        if (e.document == 'scene' && widget.onOpenScene != null)
          PopupMenuItem(
            key: const Key('project.menu.openAdditive'),
            value: 'openAdditive',
            enabled: !many,
            child: const Text('Open Scene Additive'),
          ),
        const PopupMenuItem(value: 'show', child: Text('Show in Inspector')),
        const PopupMenuItem(value: 'reveal', child: Text('Reveal in Finder  ⇧⌥⌘E')),
        if (widget.onFindReferences != null && e.document != 'scene')
          PopupMenuItem(
            key: const Key('project.menu.findReferences'),
            value: 'findReferences',
            enabled: !many,
            child: const Text('Find References In Scene'),
          ),
        const PopupMenuDivider(),
        if (model.canCopyIntoProject)
          PopupMenuItem(
            key: const Key('project.menu.copyIntoProject'),
            value: 'copyIntoProject',
            enabled: model.canEdit,
            child: Text(many ? 'Copy Selection into Project' : 'Copy into Project'),
          ),
        PopupMenuItem(value: 'rename', enabled: editable && !many, child: const Text('Rename  ⇧⌥⌘R')),
        PopupMenuItem(
          value: 'move',
          enabled: editable,
          child: Text(many ? 'Move Selection to…  ⇧⌥⌘V' : 'Move to…  ⇧⌥⌘V'),
        ),
        PopupMenuItem(
          value: 'delete',
          enabled: editable,
          child: Text(many ? 'Delete Selection  ⇧⌥⌘⌫' : 'Delete  ⇧⌥⌘⌫'),
        ),
        if (e.isSource)
          PopupMenuItem(value: 'reimport', enabled: editable && !many, child: const Text('Reimport  ⇧⌥⌘I')),
        if (e.document == 'prefab')
          PopupMenuItem(
            key: const Key('project.menu.createVariant'),
            value: 'createVariant',
            enabled: canHere && !many,
            child: const Text('Create Prefab Variant'),
          ),
        if (e.document == 'scene')
          PopupMenuItem(value: 'startup', enabled: model.canEdit && !many, child: const Text('Set as Startup Scene')),
        const PopupMenuDivider(),
        PopupMenuItem(value: 'copyId', child: Text('Copy id  ${e.id.substring(0, 13)}…')),
        PopupMenuItem(value: 'copyPath', child: Text('Copy path  ${e.path}')),
        const PopupMenuDivider(),
        PopupMenuItem(value: 'new:scene', enabled: canHere, child: const Text('New Scene Here')),
        PopupMenuItem(value: 'new:prefab', enabled: canHere, child: const Text('New Prefab Here')),
        PopupMenuItem(value: 'new:material', enabled: canHere, child: const Text('New Material Here')),
        PopupMenuItem(value: 'new:volume', enabled: canHere, child: const Text('New Volume Profile Here')),
        PopupMenuItem(value: 'new:physics_material', enabled: canHere, child: const Text('New Physics Material Here')),
        ..._dataItems(here: true, enabled: canHere),
        PopupMenuItem(value: 'new:folder', enabled: canHere, child: const Text('New Folder Here')),
        if (extensionEntries.isNotEmpty) ...[
          const PopupMenuDivider(),
          for (final (i, e) in extensionEntries.indexed)
            PopupMenuItem(key: e.key, value: 'ext:$i', enabled: e.enabled, child: Text(e.label)),
        ],
      ],
    );
    if (!mounted) return;
    if (choice != null && choice.startsWith('ext:')) {
      extensionEntries[int.parse(choice.substring(4))].run();
      return;
    }
    switch (choice) {
      case 'open':
        _activate(e);
      case 'createVariant':
        model.newPrefabVariant(e.id);
      case 'openAdditive':
        widget.onOpenScene?.call(e.id, additive: true);
      case 'show':
        widget.onShowAsset();
      case 'reveal':
        await _reveal(e.path);
      case 'findReferences':
        widget.onFindReferences?.call(e.id, e.name);
      case 'copyIntoProject':
        model.copyIntoProject();
      case 'rename':
        model.beginRename(e.id);
      case 'move':
        await _moveTo(ids);
      case 'delete':
        model.delete(ids: ids);
      case 'reimport':
        final r = model.reimport(e.id);
        if (r != null && !r.ok && mounted) {
          Notices.maybeOf(this.context)?.show('Reimport refused: ${r.diagnostics.map((d) => d.message).join('; ')}');
        }
      case 'startup':
        if (model.setStartupScene(e.id) != null && mounted) {
          Notices.maybeOf(this.context)?.show('${e.name} is the startup scene: fynkel run starts with it');
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
        if (choice.startsWith('data:')) _newData(choice.substring(5), inFolder: here);
    }
  }

  /// Create ▸ one entry per data type (ADR-0027 §7), its `menu` path as
  /// the title (`Game › Weapon`); `data:<type id>`.
  List<PopupMenuEntry<String>> _dataItems({bool here = false, bool? enabled}) {
    final types = model.project.dataTypes();
    if (types.isEmpty) return const [];
    return [
      const PopupMenuDivider(),
      for (final t in types)
        PopupMenuItem(
          key: Key('project.new.data.${t.id}'),
          value: 'data:${t.id}',
          enabled: enabled ?? model.canEdit,
          child: here
              ? Text('New ${t.menuPath.join(' › ')} Here')
              : ListTile(
                  dense: true,
                  leading: Icon(materialIconNamed(t.icon) ?? assetKindIcon('data')),
                  title: Text('New ${t.menuPath.join(' › ')}'),
                ),
        ),
    ];
  }

  void _newData(String typeId, {String? inFolder}) {
    final type = model.project.dataTypes().where((t) => t.id == typeId).firstOrNull;
    if (type != null) model.newData(type, inFolder: inFolder);
  }

  Future<void> _folderMenu(BuildContext context, String rel, Offset at) async {
    final overlay = Overlay.of(context).context.findRenderObject() as RenderBox;
    final isRoot = rel.isEmpty;
    final canHere = model.canEditFolder(rel);
    // Unity's Import Package on a package's own folder (ADR-0029).
    final package = rel.split('/').length == 2 ? ProjectBrowserModel.packageOfFolder(rel) : null;
    final choice = await showMenu<String>(
      popUpAnimationStyle: AnimationStyle.noAnimation,
      context: context,
      position: RelativeRect.fromRect(Rect.fromPoints(at, at), Offset.zero & overlay.size),
      items: [
        const PopupMenuItem(value: 'open', child: Text('Show Folder')),
        PopupMenuItem(value: 'reveal', enabled: rel != packagesRoot, child: const Text('Reveal in Finder')),
        if (package != null) ...[
          const PopupMenuDivider(),
          PopupMenuItem(
            key: const Key('project.menu.importPackageAssets'),
            value: 'importPackageAssets',
            enabled: model.canEdit,
            child: Text('Import $package Assets into Project…'),
          ),
        ],
        const PopupMenuDivider(),
        PopupMenuItem(value: 'new:scene', enabled: canHere, child: const Text('New Scene Here')),
        PopupMenuItem(value: 'new:prefab', enabled: canHere, child: const Text('New Prefab Here')),
        PopupMenuItem(value: 'new:material', enabled: canHere, child: const Text('New Material Here')),
        PopupMenuItem(value: 'new:volume', enabled: canHere, child: const Text('New Volume Profile Here')),
        PopupMenuItem(value: 'new:physics_material', enabled: canHere, child: const Text('New Physics Material Here')),
        ..._dataItems(here: true, enabled: canHere),
        PopupMenuItem(value: 'new:folder', enabled: canHere, child: const Text('New Folder Here')),
        if (!isRoot && canHere) ...[
          const PopupMenuDivider(),
          PopupMenuItem(value: 'rename', child: const Text('Rename Folder')),
          PopupMenuItem(value: 'delete', child: const Text('Delete Empty Folder')),
        ],
      ],
    );
    if (!mounted) return;
    switch (choice) {
      case 'open':
        model.setFolder(rel);
      case 'reveal':
        await _reveal(rel);
      case 'importPackageAssets':
        await _importPackageAssets(package!);
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
        if (choice.startsWith('data:')) _newData(choice.substring(5), inFolder: rel);
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
    if (model.inScripts) return _onScriptKey(k);
    final grid = model.grid;
    if (k == LogicalKeyboardKey.arrowDown && meta) {
      // Unity (macOS): ⌘↓ opens; Enter renames.
      final lead = model.leadEntry;
      if (lead != null) _activate(lead);
    } else if (grid && (k == LogicalKeyboardKey.arrowDown || k == LogicalKeyboardKey.arrowUp)) {
      // The grid: a row down or up, and across with ← →.
      _gridStep(k == LogicalKeyboardKey.arrowDown ? _gridColumns : -_gridColumns, extend: shift);
    } else if (grid && (k == LogicalKeyboardKey.arrowRight || k == LogicalKeyboardKey.arrowLeft)) {
      _gridStep(k == LogicalKeyboardKey.arrowRight ? 1 : -1, extend: shift);
    } else if (k == LogicalKeyboardKey.arrowDown) {
      model.step(1, extend: shift);
      WidgetsBinding.instance.addPostFrameCallback((_) => _scrollToShown());
    } else if (k == LogicalKeyboardKey.arrowUp) {
      model.step(-1, extend: shift);
      WidgetsBinding.instance.addPostFrameCallback((_) => _scrollToShown());
    } else if (k == LogicalKeyboardKey.enter || k == LogicalKeyboardKey.numpadEnter || k == LogicalKeyboardKey.f2) {
      model.beginRename();
    } else if (k == LogicalKeyboardKey.backspace || k == LogicalKeyboardKey.delete) {
      // ⌘⌫ as in Unity, and bare ⌫ / ⌦; both ask first.
      model.requestDelete();
    } else if (k == LogicalKeyboardKey.keyD && meta && !shift) {
      model.duplicate();
    } else if (k == LogicalKeyboardKey.keyA && meta) {
      model.selectAll();
    } else if (k == LogicalKeyboardKey.escape) {
      if (model.filtering) {
        _search.clear();
        model.clearFilters();
      } else {
        model.clearSelection();
      }
    } else if (k == LogicalKeyboardKey.arrowRight && _leadExpandable(expanded: false)) {
      // Unity: → opens a model's parts, ← closes them.
      model.toggleExpanded(model.lead!);
    } else if (k == LogicalKeyboardKey.arrowLeft && _leadExpandable(expanded: true)) {
      model.toggleExpanded(model.lead!);
    } else if (k == LogicalKeyboardKey.arrowLeft && model.view == BrowserView.folders && !model.filtering) {
      model.goUp();
    } else {
      return KeyEventResult.ignored;
    }
    return KeyEventResult.handled;
  }

  bool _leadExpandable({required bool expanded}) {
    final e = model.lead == null ? null : model.entry(model.lead!);
    return e != null && model.canExpand(e) && model.expandedSources.contains(e.id) == expanded;
  }

  KeyEventResult _onScriptKey(LogicalKeyboardKey k) {
    final grid = model.grid;
    if (k == LogicalKeyboardKey.arrowDown) {
      model.stepScript(grid ? _gridColumns : 1);
    } else if (k == LogicalKeyboardKey.arrowUp) {
      model.stepScript(grid ? -_gridColumns : -1);
    } else if (grid && k == LogicalKeyboardKey.arrowRight) {
      model.stepScript(1);
    } else if (grid && k == LogicalKeyboardKey.arrowLeft) {
      model.stepScript(-1);
    } else if (k == LogicalKeyboardKey.enter) {
      final lead = model.scriptLead;
      if (lead != null) _openScript(lead);
    } else if (k == LogicalKeyboardKey.escape) {
      if (model.query.isNotEmpty) {
        _search.clear();
        model.setQuery('');
      } else {
        model.selectScript(null);
      }
    } else if (k == LogicalKeyboardKey.arrowLeft && model.view == BrowserView.folders && model.query.isEmpty) {
      model.scriptsUp();
    } else {
      return KeyEventResult.ignored;
    }
    return KeyEventResult.handled;
  }

  // ── Build ─────────────────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    PanelFocus.primary(context, _listFocus);
    // A browser squeezed narrower than its toolbar, tree and columns (a
    // copy beside another, item 9e; a dragged divider) keeps its layout and
    // is cut at the right edge instead of crushing its rows.
    return LayoutBuilder(
      builder: (context, c) {
        final body = _body(context);
        if (c.maxWidth >= _minWidth) return body;
        return ClipRect(
          child: OverflowBox(alignment: Alignment.topLeft, minWidth: _minWidth, maxWidth: _minWidth, child: body),
        );
      },
    );
  }

  /// The narrowest the browser lays out at.
  static const _minWidth = 400.0;

  Widget _body(BuildContext context) {
    final theme = Theme.of(context);
    final scripts = model.inScripts;
    final folders = scripts ? const <String>[] : model.visibleFolders;
    final assets = scripts ? const <AssetEntry>[] : model.visibleAssets;
    return Column(
      children: [
        _toolbar(context, theme),
        Expanded(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (model.view == BrowserView.folders) ...[
                SizedBox(width: 180, child: _tree(theme)),
                VerticalDivider(width: 1, color: context.tokens.line),
              ],
              Expanded(
                child: LayoutBuilder(
                  builder: (context, c) {
                    final columns = c.maxWidth >= _columnsFrom;
                    return Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        if (!model.grid) _columnHeader(context, theme, columns: columns),
                        Expanded(
                          child: scripts
                              ? _scriptList(context, theme, columns: columns)
                              : _prefabDrop(_list(context, theme, folders, assets, columns: columns)),
                        ),
                      ],
                    );
                  },
                ),
              ),
            ],
          ),
        ),
        if (model.lastError != null) _errorBar(context, theme, model.lastError!),
        scripts ? _scriptStatus(context, theme) : _status(context, theme, assets.length),
      ],
    );
  }

  /// The list takes a Hierarchy object as a new prefab in the shown
  /// folder (Unity's drag from the Hierarchy to the Project window).
  Widget _prefabDrop(Widget list) => DragTarget<List<String>>(
    key: const Key('project.prefabDrop'),
    onWillAcceptWithDetails: (d) => model.canMakePrefab(d.data),
    onAcceptWithDetails: (d) => model.dropObjects(d.data, model.folder),
    builder: (context, candidates, _) => candidates.isEmpty && !_filesOver
        ? list
        : DecoratedBox(
            position: DecorationPosition.foreground,
            decoration: BoxDecoration(border: Border.all(color: context.tokens.accent)),
            child: list,
          ),
  );

  /// Below this width the list shows names and kinds only.
  static const _columnsFrom = 380.0;

  Widget _list(
    BuildContext context,
    ThemeData theme,
    List<String> folders,
    List<AssetEntry> assets, {
    required bool columns,
  }) {
    return Focus(
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
                  model.filtering ? 'Nothing matches.' : 'This folder is empty. Use New ▾ or drop files here.',
                  key: const Key('project.noRows'),
                  textAlign: TextAlign.center,
                  style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
                ),
              )
            : Builder(
                builder: (context) {
                  // An expanded model lists its parts under it (Unity's ▸).
                  final rows = _rows();
                  final tile = model.grid ? model.tileSize : null;
                  Widget item(BuildContext context, int i) => switch (rows[i]) {
                    final String f => _folderRow(context, theme, f, inList: true, columns: columns, tile: tile),
                    final AssetEntry a => _assetRow(context, theme, a, columns: columns, tile: tile),
                    (final AssetEntry a, final String sub) => _PartRow(
                      key: Key('project.part.${a.id}.$sub'),
                      model: model,
                      entry: a,
                      sub: sub,
                      selected: model.lead == a.id && model.leadPart == sub,
                      tile: tile,
                      onTap: () {
                        _listFocus.requestFocus();
                        model.selectPart(a.id, sub);
                      },
                    ),
                    _ => const SizedBox.shrink(),
                  };
                  if (tile != null) {
                    return _grid(
                      key: const Key('project.grid'),
                      controller: _listScroll,
                      count: rows.length,
                      item: item,
                    );
                  }
                  return ListView.builder(
                    key: const Key('project.list'),
                    controller: _listScroll,
                    itemCount: rows.length,
                    itemExtent: projectRowHeight(context),
                    itemBuilder: item,
                  );
                },
              ),
      ),
    );
  }

  Widget _scriptList(BuildContext context, ThemeData theme, {required bool columns}) {
    final folders = model.visibleScriptFolders;
    final files = model.visibleScripts;
    return Focus(
      focusNode: _listFocus,
      onKeyEvent: _onKey,
      child: GestureDetector(
        behavior: HitTestBehavior.translucent,
        excludeFromSemantics: true,
        onTap: () {
          _listFocus.requestFocus();
          model.selectScript(null);
        },
        onSecondaryTapUp: (d) => _scriptMenu(context, model.scriptFolder, d.globalPosition, folder: true),
        child: folders.isEmpty && files.isEmpty
            ? Center(
                child: Text(
                  model.query.trim().isNotEmpty ? 'Nothing matches.' : 'No Dart files here. Use New ▾ → Behaviour.',
                  key: const Key('project.noRows'),
                  textAlign: TextAlign.center,
                  style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
                ),
              )
            : _scriptItems(model.grid, folders.length + files.length, (context, i) {
                if (i < folders.length) {
                  final f = folders[i];
                  return _ScriptRow(
                    key: Key('project.scripts.row.$f'),
                    model: model,
                    rel: f,
                    folder: true,
                    depth: 0,
                    current: false,
                    columns: columns,
                    tile: model.grid ? model.tileSize : null,
                    onTap: () {
                      _listFocus.requestFocus();
                      model.showScripts(f);
                    },
                    onSecondaryTap: (at) => _scriptMenu(context, f, at, folder: true),
                  );
                }
                final f = files[i - folders.length];
                return _ScriptRow(
                  key: Key('project.script.$f'),
                  model: model,
                  rel: f,
                  folder: false,
                  depth: 0,
                  current: model.scriptLead == f,
                  showPath: model.view == BrowserView.list || model.query.trim().isNotEmpty,
                  columns: columns,
                  tile: model.grid ? model.tileSize : null,
                  onTap: () => _tapScript(f),
                  onSecondaryTap: (at) => _scriptMenu(context, f, at, folder: false),
                );
              }),
      ),
    );
  }

  /// The Scripts list, or its grid.
  Widget _scriptItems(bool grid, int count, IndexedWidgetBuilder item) => grid
      ? _grid(key: const Key('project.scripts.grid'), count: count, item: item)
      : ListView.builder(
          key: const Key('project.scripts.list'),
          itemCount: count,
          itemExtent: projectRowHeight(context),
          itemBuilder: item,
        );

  /// Tiles as many to a row as fit, the zoom slider's size; rendered
  /// thumbnails fill in as they are drawn.
  Widget _grid({
    required Key key,
    ScrollController? controller,
    required int count,
    required IndexedWidgetBuilder item,
  }) {
    final size = model.tileSize;
    // A new grid each time: a thumbnail drawn rebuilds the tiles.
    Widget grid() => LayoutBuilder(
      builder: (context, c) {
        final columns = ((c.maxWidth - 2 * _gridPad) / (size + _tileGutter)).floor().clamp(1, 1 << 16);
        _gridColumns = columns;
        return GridView.builder(
          key: key,
          controller: controller,
          padding: const EdgeInsets.all(_gridPad),
          gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
            crossAxisCount: columns,
            mainAxisExtent: _tileExtent(size),
          ),
          itemCount: count,
          itemBuilder: item,
        );
      },
    );
    final thumbnails = model.thumbnails;
    return thumbnails == null ? grid() : ListenableBuilder(listenable: thumbnails, builder: (_, _) => grid());
  }

  /// Unity's slider at the status bar's right: its left end is the list.
  Widget _zoomSlider(BuildContext context) {
    final t = context.tokens;
    return Tooltip(
      message: model.grid ? 'Tile size (all the way left: the list)' : 'Drag right for a grid of thumbnails',
      waitDuration: const Duration(milliseconds: 600),
      child: SizedBox(
        width: 96,
        child: SliderTheme(
          data: SliderTheme.of(context).copyWith(
            trackHeight: 2,
            thumbShape: const RoundSliderThumbShape(enabledThumbRadius: 5),
            overlayShape: const RoundSliderOverlayShape(overlayRadius: 10),
            activeTrackColor: t.textSecondary,
            inactiveTrackColor: t.line,
            thumbColor: t.text,
          ),
          child: Slider(key: const Key('project.zoom'), value: model.zoom, onChanged: model.setZoom),
        ),
      ),
    );
  }

  Widget _scriptStatus(BuildContext context, ThemeData theme) {
    final t = context.tokens;
    final total = model.scripts.files.length;
    final shown = model.visibleScripts.length;
    final where = model.view == BrowserView.list
        ? 'lib'
        : model.scriptFolder.isEmpty
        ? 'lib'
        : 'lib/${model.scriptFolder}';
    final text =
        model.scriptsBusy ??
        [shown == total ? '$total script${total == 1 ? '' : 's'}' : '$shown of $total scripts', 'in $where'].join(' ');
    return Container(
      height: t.row + 2,
      alignment: Alignment.centerLeft,
      padding: EdgeInsets.symmetric(horizontal: t.pad),
      decoration: BoxDecoration(
        color: t.header,
        border: Border(top: t.hairline),
      ),
      child: Row(
        children: [
          if (model.view == BrowserView.folders && model.scriptFolder.isNotEmpty)
            InkWell(
              key: const Key('project.up'),
              onTap: model.scriptsUp,
              child: const Padding(padding: EdgeInsets.only(right: 6), child: Icon(FIcons.arrowUpward, size: 13)),
            ),
          if (model.scriptsBusy != null) ...[
            const SizedBox(width: 10, height: 10, child: CircularProgressIndicator(strokeWidth: 1.5)),
            const SizedBox(width: 6),
          ],
          Expanded(
            child: Text(
              text,
              key: const Key('project.status'),
              style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
              overflow: TextOverflow.ellipsis,
            ),
          ),
          _zoomSlider(context),
        ],
      ),
    );
  }

  /// Name | Kind | Contents | Size. Name and Kind sort (again to flip).
  Widget _columnHeader(BuildContext context, ThemeData theme, {required bool columns}) {
    final t = context.tokens;
    final style = InspectorCaption.style(context);
    Widget sortable(AssetSort s, String label, {double? width}) {
      final on = model.sort == s;
      final cell = InkWell(
        key: Key('project.column.${s.name}'),
        onTap: () => model.setSort(s),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(label.toUpperCase(), style: on ? style.copyWith(color: t.textSecondary) : style),
            if (on) ...[
              const SizedBox(width: 3),
              Icon(model.descending ? FIcons.arrowDownward : FIcons.arrowUpward, size: 10, color: t.textSecondary),
            ],
          ],
        ),
      );
      return width == null
          ? Expanded(
              child: Align(alignment: Alignment.centerLeft, child: cell),
            )
          : SizedBox(
              width: width,
              child: Align(alignment: Alignment.centerLeft, child: cell),
            );
    }

    return Container(
      key: const Key('project.columns'),
      height: t.row,
      // Lines the captions up with the names after each row's picture.
      padding: EdgeInsets.only(left: 6 + t.row - 2 + 8),
      decoration: BoxDecoration(border: Border(bottom: t.hairline)),
      child: Row(
        children: [
          sortable(AssetSort.name, 'Name'),
          sortable(AssetSort.kind, 'Kind', width: _AssetRow.kindWidth),
          if (columns) ...[
            SizedBox(
              width: _AssetRow.contentsWidth,
              child: Text('CONTENTS', style: style),
            ),
            SizedBox(
              width: _AssetRow.sizeWidth,
              child: Text('SIZE', style: style, textAlign: TextAlign.right),
            ),
          ],
          const SizedBox(width: _AssetRow.endGap),
        ],
      ),
    );
  }

  Widget _toolbar(BuildContext context, ThemeData theme) {
    final history = model.history;
    final kinds = model.kinds;
    final counts = model.countsByKind;
    final t = context.tokens;
    return Container(
      decoration: BoxDecoration(
        color: t.header,
        border: Border(bottom: t.hairline),
      ),
      padding: const EdgeInsets.fromLTRB(3, 3, 3, 3),
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
                icon: Icon(FIcons.folderOutlined, size: 14),
                tooltip: 'Folders',
              ),
              ButtonSegment(
                value: BrowserView.list,
                icon: Icon(FIcons.viewListOutlined, size: 14),
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
              style: theme.textTheme.bodySmall?.copyWith(color: t.text),
              decoration: InputDecoration(
                prefixIcon: Icon(FIcons.search, size: 13, color: t.textDisabled),
                prefixIconConstraints: BoxConstraints(minWidth: t.row, minHeight: t.row),
                hintText: model.inScripts
                    ? 'Search scripts and behaviour ids…'
                    : model.view == BrowserView.folders && model.folder.isNotEmpty
                    ? 'Search under ${model.folder}…'
                    : 'Search name, path, id, sub-asset…',
                suffixIcon: model.query.isEmpty
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
            isLabelVisible: kinds.isNotEmpty,
            label: Text('${kinds.length}'),
            child: PopupMenuButton<String>(
              popUpAnimationStyle: AnimationStyle.noAnimation,
              key: const Key('project.filter'),
              tooltip: 'Show only these kinds',
              icon: Icon(FIcons.filterList, color: kinds.isNotEmpty ? t.accent : null),
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
            popUpAnimationStyle: AnimationStyle.noAnimation,
            key: const Key('project.sort'),
            tooltip: 'Sort (again to flip)',
            icon: Icon(model.descending ? FIcons.arrowDownward : FIcons.arrowUpward),
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
          LockButton(
            key: const Key('project.lock'),
            locked: model.locked,
            lockTip: 'Lock: stay in this folder when an asset is shown elsewhere',
            unlockTip: 'Unlock: follow "Show in Project" again',
            onChanged: model.setLocked,
          ),
          PopupMenuButton<String>(
            popUpAnimationStyle: AnimationStyle.noAnimation,
            key: const Key('project.new'),
            tooltip: 'New scene, prefab, material, volume profile, physics material, data asset, folder or behaviour',
            icon: const Icon(FIcons.add),
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
              PopupMenuItem(
                key: const Key('project.new.volume'),
                value: 'volume',
                child: ListTile(
                  dense: true,
                  leading: Icon(assetKindIcon('volume')),
                  title: const Text('New Volume Profile'),
                ),
              ),
              PopupMenuItem(
                key: const Key('project.new.physics_material'),
                value: 'physics_material',
                child: ListTile(
                  dense: true,
                  leading: Icon(assetKindIcon('physics_material')),
                  title: const Text('New Physics Material'),
                ),
              ),
              ..._dataItems(),
              const PopupMenuDivider(),
              const PopupMenuItem(
                key: Key('project.new.folder'),
                value: 'folder',
                child: ListTile(dense: true, leading: Icon(FIcons.createNewFolderOutlined), title: Text('New Folder')),
              ),
              if (widget.generateBehaviours != null && model.hasScripts) ...[
                const PopupMenuDivider(),
                PopupMenuItem(
                  key: const Key('project.new.behaviour'),
                  value: 'behaviour',
                  enabled: _canNewBehaviour,
                  child: ListTile(
                    dense: true,
                    leading: const Icon(FIcons.code),
                    title: const Text('New Behaviour…'),
                    subtitle: Text('in lib/${model.newBehaviourFolder}', overflow: TextOverflow.ellipsis),
                  ),
                ),
              ],
            ],
            onSelected: (v) => switch (v) {
              'folder' => model.newFolder(),
              'behaviour' => _newBehaviour(),
              _ when v.startsWith('data:') => _newData(v.substring(5)),
              _ => model.newDocument(v),
            },
          ),
          IconButton(
            key: const Key('project.undo'),
            tooltip: history.canUndo ? 'Undo ${history.undoLabel}  ⌥⌘Z' : 'Nothing to undo',
            icon: const Icon(FIcons.undo),
            onPressed: history.canUndo && model.canEdit ? model.undo : null,
          ),
          IconButton(
            key: const Key('project.redo'),
            tooltip: history.canRedo ? 'Redo ${history.redoLabel}  ⇧⌥⌘Z' : 'Nothing to redo',
            icon: const Icon(FIcons.redo),
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
      itemExtent: projectRowHeight(context),
      children: [
        _folderRow(context, theme, '', inList: false),
        for (final f in folders)
          if (!ProjectBrowserModel.inPackages(f)) _folderRow(context, theme, f, inList: false),
        // Unity's Packages: a root of its own (ADR-0029).
        for (final f in folders)
          if (ProjectBrowserModel.inPackages(f)) _folderRow(context, theme, f, inList: false),
        if (model.hasScripts)
          for (final f in ['', ...model.scripts.folders])
            _ScriptRow(
              key: Key('project.scripts.tree.${f.isEmpty ? 'root' : f}'),
              model: model,
              rel: f,
              folder: true,
              depth: f.isEmpty ? 0 : f.split('/').length,
              current: model.inScripts && model.scriptFolder == f,
              tree: true,
              onTap: () {
                _listFocus.requestFocus();
                model.showScripts(f);
              },
              onSecondaryTap: (at) => _scriptMenu(context, f, at, folder: true),
            ),
      ],
    );
  }

  Widget _folderRow(
    BuildContext context,
    ThemeData theme,
    String rel, {
    required bool inList,
    bool columns = false,
    double? tile,
  }) {
    // Packages is a root beside assets, so its folders are a level up.
    final depth = inList || rel.isEmpty ? 0 : rel.split('/').length - (ProjectBrowserModel.inPackages(rel) ? 1 : 0);
    final name = rel.isEmpty ? 'assets' : p.basename(rel);
    final current = !inList && !model.inScripts && rel == model.folder;
    final renaming = model.renamingFolder == rel && rel.isNotEmpty;
    return _FolderRow(
      key: Key('project.folder.${inList ? 'row' : 'tree'}.${rel.isEmpty ? 'root' : rel}'),
      model: model,
      rel: rel,
      name: name,
      depth: depth,
      current: current,
      inList: inList,
      columns: columns,
      tile: tile,
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

  Widget _assetRow(BuildContext context, ThemeData theme, AssetEntry e, {required bool columns, double? tile}) {
    final renaming = model.renamingAsset == e.id;
    return _AssetRow(
      key: Key('project.asset.${e.id}'),
      model: model,
      entry: e,
      selected: model.isSelected(e.id),
      lead: model.lead == e.id,
      pinged: model.pinged == e.id,
      showPath: model.view == BrowserView.list || model.filtering,
      columns: columns,
      tile: tile,
      renaming: renaming,
      renameController: renaming ? _rename : null,
      onTap: () => _tap(e),
      onSecondaryTap: (at) => _assetMenu(context, e, at),
      onRenameSubmit: (text) => model.rename(e.id, text),
      onRenameCancel: model.cancelRename,
    );
  }

  Widget _errorBar(BuildContext context, ThemeData theme, String message) {
    final t = context.tokens;
    return Container(
      key: const Key('project.error'),
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
            child: Text(
              message,
              style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onErrorContainer),
              maxLines: 3,
              overflow: TextOverflow.ellipsis,
            ),
          ),
          IconButton(tooltip: 'Dismiss', iconSize: 12, icon: const Icon(FIcons.close), onPressed: model.clearError),
        ],
      ),
    );
  }

  Widget _status(BuildContext context, ThemeData theme, int shown) {
    final total = model.entries.length;
    final selected = model.selection.length;
    final where = model.view == BrowserView.list
        ? 'every folder'
        : model.folder.isEmpty
        ? 'assets'
        : ProjectBrowserModel.inPackages(model.folder)
        ? model.folder
        : 'assets/${model.folder}';
    final text = [
      shown == total ? '$total asset${total == 1 ? '' : 's'}' : '$shown of $total assets',
      'in $where',
      if (selected > 0) '· $selected selected',
    ].join(' ');
    final t = context.tokens;
    return Container(
      height: t.row + 2,
      alignment: Alignment.centerLeft,
      padding: EdgeInsets.symmetric(horizontal: t.pad),
      decoration: BoxDecoration(
        color: t.header,
        border: Border(top: t.hairline),
      ),
      child: Row(
        children: [
          if (model.view == BrowserView.folders && model.folder.isNotEmpty)
            InkWell(
              key: const Key('project.up'),
              onTap: model.goUp,
              child: const Padding(padding: EdgeInsets.only(right: 6), child: Icon(FIcons.arrowUpward, size: 13)),
            ),
          Expanded(
            child: Text(
              text,
              key: const Key('project.status'),
              style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
              overflow: TextOverflow.ellipsis,
            ),
          ),
          _zoomSlider(context),
        ],
      ),
    );
  }
}

// ── Tiles ───────────────────────────────────────────────────────────────

/// The grid's padding around its tiles.
const _gridPad = 6.0;

/// A tile's width beyond its picture.
const _tileGutter = 16.0;

/// A tile's height below its picture: two lines of name.
const _tileLabel = 34.0;

/// A tile of [size] (its picture's side), from the zoom slider.
double _tileExtent(double size) => size + 8 + _tileLabel;

/// A grid tile (Unity's Project grid): the picture over the name. The
/// selected tile's name is filled, the rest of a multi-selection a step
/// quieter; a model's parts sit on a darker ground.
class _Tile extends StatelessWidget {
  const _Tile({
    required this.size,
    required this.picture,
    required this.name,
    this.selected = false,
    this.lead = true,
    this.outlined = false,
    this.part = false,
    this.badge,
    this.label,
  });

  final double size;
  final Widget picture;
  final String name;
  final bool selected;
  final bool lead;

  /// An accent frame: a ping, or a folder with a drag over it.
  final bool outlined;

  /// One of an expanded model's parts.
  final bool part;

  /// At the picture's right edge (a model's ▸).
  final Widget? badge;

  /// In place of the name (the inline rename).
  final Widget? label;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    final focused = PanelFocus.of(context);
    final fill = t.selectedRow(focused: focused);
    return Container(
      margin: const EdgeInsets.all(1),
      decoration: BoxDecoration(
        color: part ? t.header : (outlined ? t.surfaceOf(t.accent, .18) : null),
        borderRadius: BorderRadius.circular(6),
        border: outlined ? Border.all(color: t.accent, width: 1.5) : null,
      ),
      padding: const EdgeInsets.only(top: 4),
      child: Column(
        children: [
          SizedBox(
            width: size + _tileGutter - 4,
            height: size,
            child: Stack(
              clipBehavior: Clip.none,
              children: [
                Center(
                  child: Container(
                    decoration: BoxDecoration(
                      color: selected ? t.surfaceOf(fill, lead ? .45 : .3) : null,
                      borderRadius: BorderRadius.circular(6),
                    ),
                    child: SizedBox.square(
                      dimension: size,
                      child: Center(child: picture),
                    ),
                  ),
                ),
                if (badge != null) Positioned(right: -2, top: 0, bottom: 0, child: Center(child: badge)),
              ],
            ),
          ),
          const SizedBox(height: 4),
          SizedBox(
            height: _tileLabel - 4,
            child:
                label ??
                Align(
                  alignment: Alignment.topCenter,
                  child: Container(
                    padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
                    decoration: BoxDecoration(
                      color: selected ? (lead ? fill : t.surfaceOf(fill, .7)) : null,
                      borderRadius: BorderRadius.circular(3),
                    ),
                    child: Text(
                      name,
                      textAlign: TextAlign.center,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: theme.textTheme.bodySmall?.copyWith(
                        color: selected ? t.onSelectedRow(focused: focused) : t.text,
                        height: 1.2,
                      ),
                    ),
                  ),
                ),
          ),
        ],
      ),
    );
  }
}

/// The rendered thumbnail of [entry] (its part [sub]) when there is one,
/// else the row's preview (a texture, a swatch, the kind's icon).
class _TilePicture extends StatelessWidget {
  const _TilePicture({required this.model, required this.entry, this.sub, required this.size});

  final ProjectBrowserModel model;
  final AssetEntry entry;
  final String? sub;
  final double size;

  @override
  Widget build(BuildContext context) {
    final fallback = sub == null
        ? AssetPreviewWidget(preview: model.preview(entry), size: size)
        : AssetPreviewWidget(preview: IconPreview(AssetEntry.subKind(sub!)), size: size);
    final image = model.thumbnail(entry, sub: sub);
    if (image == null) return fallback;
    return Image(
      key: ValueKey('project.thumbnail.${entry.id}${sub == null ? '' : '#$sub'}'),
      image: image,
      width: size,
      height: size,
      fit: BoxFit.contain,
      gaplessPlayback: true,
      filterQuality: FilterQuality.medium,
      errorBuilder: (_, _, _) => fallback,
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
    this.inList = false,
    this.columns = false,
    this.tile,
    required this.renaming,
    required this.renameController,
    required this.onTap,
    required this.onSecondaryTap,
    required this.onRenameSubmit,
  });

  final ProjectBrowserModel model;
  final String rel;
  final String name;

  /// A grid tile of this size instead of a row.
  final double? tile;
  final int depth;
  final bool current;

  /// A row of the list (not the tree): it fills the list's columns.
  final bool inList;

  /// Contents and Size too (a wide enough panel).
  final bool columns;
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
    final t = context.tokens;
    final focused = PanelFocus.of(context);
    final tile = widget.tile;
    final content = tile != null
        ? _Tile(
            size: tile,
            outlined: _hot,
            name: widget.name,
            picture: Icon(FIcons.folderOutlined, size: tile * .8, color: t.textSecondary),
            label: widget.renaming
                ? _InlineRename(
                    key: Key('project.renameFolder.${widget.rel}'),
                    controller: widget.renameController!,
                    onSubmit: widget.onRenameSubmit,
                    onCancel: widget.model.cancelRename,
                  )
                : null,
          )
        : Container(
            height: projectRowHeight(context),
            padding: EdgeInsets.only(left: 6.0 + widget.depth * 14),
            decoration: BoxDecoration(
              color: widget.current ? t.selectedRow(focused: focused) : (_hot ? t.surfaceOf(t.accent, .2) : null),
              border: _hot ? Border.all(color: t.accent) : null,
            ),
            child: Row(
              children: [
                Icon(
                  widget.rel.isEmpty
                      ? FIcons.homeOutlined
                      : (widget.current ? FIcons.folderOpen : FIcons.folderOutlined),
                  size: 14,
                  color: widget.current ? t.onSelectedRowSecondary(focused: focused) : t.textSecondary,
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
                      : Text(
                          widget.name,
                          style: theme.textTheme.bodyMedium?.copyWith(
                            color: widget.current ? t.onSelectedRow(focused: focused) : t.text,
                          ),
                          overflow: TextOverflow.ellipsis,
                        ),
                ),
                if (widget.inList && !widget.renaming) ...[
                  SizedBox(
                    width: _AssetRow.kindWidth,
                    child: Text('folder', style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled)),
                  ),
                  if (widget.columns) ...[
                    SizedBox(
                      width: _AssetRow.contentsWidth,
                      child: Text(
                        switch (widget.model.folderCount(widget.rel)) {
                          0 => 'empty',
                          1 => '1 asset',
                          final n => '$n assets',
                        },
                        key: Key('project.contents.folder.${widget.rel}'),
                        style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
                      ),
                    ),
                    const SizedBox(width: _AssetRow.sizeWidth),
                  ],
                  const SizedBox(width: _AssetRow.endGap),
                ],
              ],
            ),
          );
    if (widget.renaming) return content;
    final assets = DragTarget<AssetDragPayload>(
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
    // A Hierarchy object dropped here becomes a prefab in this folder.
    return DragTarget<List<String>>(
      onWillAcceptWithDetails: (d) {
        final ok = widget.model.canMakePrefab(d.data);
        if (ok != _hot) setState(() => _hot = ok);
        return ok;
      },
      onLeave: (_) => setState(() => _hot = false),
      onAcceptWithDetails: (d) {
        setState(() => _hot = false);
        widget.model.dropObjects(d.data, widget.rel);
      },
      builder: (context, _, _) => assets,
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
    this.pinged = false,
    required this.showPath,
    required this.columns,
    this.tile,
    required this.renaming,
    required this.renameController,
    required this.onTap,
    required this.onSecondaryTap,
    required this.onRenameSubmit,
    required this.onRenameCancel,
  });

  // The fixed columns, shared with the header and the folder rows.
  static const kindWidth = 72.0;
  static const contentsWidth = 70.0;
  static const sizeWidth = 60.0;
  static const endGap = 10.0;

  final ProjectBrowserModel model;
  final AssetEntry entry;
  final bool selected;
  final bool lead;

  /// Outlined by a ping (an asset field clicked in the Inspector).
  final bool pinged;
  final bool showPath;

  /// Contents and Size too (a wide enough panel).
  final bool columns;

  /// A grid tile of this size instead of a row.
  final double? tile;
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
    final t = context.tokens;
    final focused = PanelFocus.of(context);
    // The lead row in full; the rest of a multi-selection a step quieter.
    final fill = t.selectedRow(focused: focused);
    final on = selected ? t.onSelectedRow(focused: focused) : t.text;
    final dim = selected ? t.onSelectedRowSecondary(focused: focused) : t.textDisabled;
    final tile = this.tile;
    final content = tile != null
        ? _Tile(
            size: tile,
            selected: selected,
            lead: lead,
            outlined: pinged,
            name: stem,
            picture: _TilePicture(model: model, entry: e, size: tile),
            badge: model.canExpand(e)
                ? GestureDetector(
                    key: Key('project.expand.${e.id}'),
                    onTap: () => model.toggleExpanded(e.id),
                    child: Container(
                      decoration: BoxDecoration(color: t.header, borderRadius: BorderRadius.circular(3)),
                      child: Icon(
                        model.expandedSources.contains(e.id) ? FIcons.chevronLeft : FIcons.chevronRight,
                        size: 14,
                        color: t.textSecondary,
                      ),
                    ),
                  )
                : null,
            label: renaming
                ? _InlineRename(
                    key: Key('project.rename.${e.id}'),
                    controller: renameController!,
                    onSubmit: onRenameSubmit,
                    onCancel: onRenameCancel,
                  )
                : null,
          )
        : Container(
            height: projectRowHeight(context),
            padding: const EdgeInsets.only(left: 6),
            decoration: BoxDecoration(
              color: selected ? (lead ? fill : t.surfaceOf(fill, .7)) : (pinged ? t.surfaceOf(t.accent, .18) : null),
              border: pinged ? Border.all(color: t.accent, width: 1.5) : null,
            ),
            child: Row(
              children: [
                // Unity's ▸ on a model: its parts under it.
                SizedBox(
                  width: 14,
                  child: model.canExpand(e)
                      ? GestureDetector(
                          key: Key('project.expand.${e.id}'),
                          onTap: () => model.toggleExpanded(e.id),
                          child: Icon(
                            model.expandedSources.contains(e.id) ? FIcons.expandMore : FIcons.chevronRight,
                            size: 14,
                            color: dim,
                          ),
                        )
                      : null,
                ),
                AssetPreviewWidget(preview: model.preview(e), size: t.row - 2),
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
                  Expanded(
                    child: Row(
                      children: [
                        Flexible(
                          flex: 3,
                          child: Text(
                            file,
                            style: theme.textTheme.bodyMedium?.copyWith(color: on),
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                        if (showName) ...[
                          const SizedBox(width: 8),
                          Flexible(
                            flex: 2,
                            child: Text(
                              '“${e.name}”',
                              style: theme.textTheme.bodySmall?.copyWith(color: dim),
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
                              style: theme.textTheme.bodySmall?.copyWith(color: dim),
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                        ],
                      ],
                    ),
                  ),
                  SizedBox(
                    width: kindWidth,
                    child: Text(
                      kind,
                      style: theme.textTheme.bodySmall?.copyWith(color: dim),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  if (columns) ...[
                    SizedBox(
                      width: contentsWidth,
                      child: e.isSource && e.subAssets.isNotEmpty
                          ? Tooltip(
                              message: e.subAssets.keys.join(', '),
                              child: Text(
                                e.subAssets.length == 1 ? '1 part' : '${e.subAssets.length} parts',
                                key: Key('project.contents.${e.id}'),
                                style: theme.textTheme.bodySmall?.copyWith(color: dim),
                              ),
                            )
                          : null,
                    ),
                    SizedBox(
                      width: sizeWidth,
                      child: Text(
                        switch (model.size(e)) {
                          final n? => formatBytes(n),
                          null => '',
                        },
                        key: Key('project.size.${e.id}'),
                        textAlign: TextAlign.right,
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: dim,
                          fontFeatures: const [FontFeature.tabularFigures()],
                        ),
                      ),
                    ),
                  ],
                  const SizedBox(width: endGap),
                ],
              ],
            ),
          );
    Widget interactive = GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: onTap,
      onSecondaryTapUp: (d) => onSecondaryTap(d.globalPosition),
      child: content,
    );
    if (renaming) return interactive;
    if (tile != null) {
      // A tile shows the name only: the file and the kind on hover.
      interactive = Tooltip(
        message: '${e.path}\n$kind',
        waitDuration: const Duration(milliseconds: 600),
        child: interactive,
      );
    }
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

/// A part of an expanded model (`mesh/Cube`, `material/Glass`): its name
/// indented under the model, dragged onto an asset field or the Hierarchy
/// like any asset, shown in the Inspector when clicked.
class _PartRow extends StatelessWidget {
  const _PartRow({
    super.key,
    required this.model,
    required this.entry,
    required this.sub,
    required this.selected,
    this.tile,
    required this.onTap,
  });

  final ProjectBrowserModel model;
  final AssetEntry entry;
  final String sub;
  final bool selected;

  /// A grid tile of this size instead of a row.
  final double? tile;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    final focused = PanelFocus.of(context);
    final on = selected ? t.onSelectedRow(focused: focused) : t.text;
    final dim = selected ? t.onSelectedRowSecondary(focused: focused) : t.textDisabled;
    final kind = AssetEntry.subKind(sub);
    final name = sub.contains('/') ? sub.substring(sub.indexOf('/') + 1) : sub;
    final tile = this.tile;
    final content = tile != null
        ? Tooltip(
            message: '$name\n$kind in ${p.basename(entry.path)}',
            waitDuration: const Duration(milliseconds: 600),
            child: _Tile(
              size: tile,
              part: true,
              selected: selected,
              name: name,
              picture: _TilePicture(model: model, entry: entry, sub: sub, size: tile),
            ),
          )
        : Container(
            height: projectRowHeight(context),
            padding: const EdgeInsets.only(left: 6 + 14 + 16),
            color: selected ? t.selectedRow(focused: focused) : null,
            child: Row(
              children: [
                Icon(assetKindIcon(kind), size: 14, color: dim),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    name,
                    style: theme.textTheme.bodyMedium?.copyWith(color: on),
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                SizedBox(
                  width: _AssetRow.kindWidth,
                  child: Text(
                    kind,
                    style: theme.textTheme.bodySmall?.copyWith(color: dim),
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                const SizedBox(width: _AssetRow.endGap),
              ],
            ),
          );
    final interactive = GestureDetector(behavior: HitTestBehavior.opaque, onTap: onTap, child: content);
    final payload = model.dragPayload(entry, sub: sub);
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
              Text(name, style: theme.textTheme.bodySmall),
            ],
          ),
        ),
      ),
      childWhenDragging: Opacity(opacity: .4, child: content),
      child: interactive,
    );
  }
}

/// A Scripts row: `lib/` and its folders in the tree, a folder or a `.dart`
/// file in the list (kind `behaviour` when it declares one, its ids under
/// Contents).
class _ScriptRow extends StatelessWidget {
  const _ScriptRow({
    super.key,
    required this.model,
    required this.rel,
    required this.folder,
    required this.depth,
    required this.current,
    this.tree = false,
    this.showPath = false,
    this.columns = false,
    this.tile,
    required this.onTap,
    required this.onSecondaryTap,
  });

  /// A grid tile of this size instead of a row.
  final double? tile;

  final ProjectBrowserModel model;
  final String rel;
  final bool folder;
  final int depth;
  final bool current;

  /// A row of the folder tree (no columns).
  final bool tree;
  final bool showPath;
  final bool columns;
  final VoidCallback onTap;
  final void Function(Offset at) onSecondaryTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    final focused = PanelFocus.of(context);
    final on = current ? t.onSelectedRow(focused: focused) : t.text;
    final dim = current ? t.onSelectedRowSecondary(focused: focused) : t.textDisabled;
    final kind = folder ? 'folder' : model.scriptKind(rel);
    final ids = folder ? const <String>[] : model.behaviourIds[rel] ?? const <String>[];
    final name = rel.isEmpty ? 'Scripts' : p.basename(rel);
    final icon = folder
        ? (rel.isEmpty ? FIcons.code : (current ? FIcons.folderOpen : FIcons.folderOutlined))
        : FIcons.descriptionOutlined;
    final row = GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: onTap,
      onSecondaryTapUp: (d) => onSecondaryTap(d.globalPosition),
      child: Tooltip(
        message: rel.isEmpty ? 'lib/ — the game\'s Dart source' : 'lib/$rel',
        waitDuration: const Duration(milliseconds: 600),
        child: tile != null
            ? _Tile(
                size: tile!,
                selected: current,
                name: name,
                picture: Icon(icon, size: tile! * .8, color: t.textSecondary),
              )
            : Container(
                height: projectRowHeight(context),
                padding: EdgeInsets.only(left: 6.0 + depth * 14),
                color: current ? t.selectedRow(focused: focused) : null,
                child: Row(
                  children: [
                    SizedBox(
                      width: tree ? 14 : t.row - 2,
                      child: Icon(icon, size: 14, color: current ? dim : t.textSecondary),
                    ),
                    SizedBox(width: tree ? 6 : 8),
                    Expanded(
                      child: Row(
                        children: [
                          Flexible(
                            flex: 3,
                            child: Text(
                              name,
                              style: theme.textTheme.bodyMedium?.copyWith(color: on),
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                          if (showPath && rel.contains('/')) ...[
                            const SizedBox(width: 8),
                            Flexible(
                              flex: 3,
                              child: Text(
                                rel.substring(0, rel.lastIndexOf('/')),
                                style: theme.textTheme.bodySmall?.copyWith(color: dim),
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                          ],
                        ],
                      ),
                    ),
                    if (!tree) ...[
                      SizedBox(
                        width: _AssetRow.kindWidth,
                        child: Text(kind, style: theme.textTheme.bodySmall?.copyWith(color: dim)),
                      ),
                      if (columns) ...[
                        SizedBox(
                          width: _AssetRow.contentsWidth,
                          child: Text(
                            ids.map((id) => id.substring(id.lastIndexOf('.') + 1)).join(', '),
                            key: Key('project.script.contents.$rel'),
                            style: theme.textTheme.bodySmall?.copyWith(color: dim),
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                        const SizedBox(width: _AssetRow.sizeWidth),
                      ],
                      const SizedBox(width: _AssetRow.endGap),
                    ],
                  ],
                ),
              ),
      ),
    );
    // A behaviour's file drags the behaviour: onto an object in the
    // Hierarchy, the Scene view or the Inspector it is added (Unity's script drop).
    final payload = folder ? null : model.scriptDragPayload(rel);
    if (payload == null) return row;
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
              Text(payload.name, style: theme.textTheme.bodySmall),
            ],
          ),
        ),
      ),
      childWhenDragging: Opacity(opacity: .4, child: row),
      child: row,
    );
  }
}

/// New Behaviour's name: an UpperCamelCase class name, checked as typed.
class _NewBehaviourDialog extends StatefulWidget {
  const _NewBehaviourDialog({required this.folder});

  /// Where the file goes, under `lib/`.
  final String folder;

  @override
  State<_NewBehaviourDialog> createState() => _NewBehaviourDialogState();
}

class _NewBehaviourDialogState extends State<_NewBehaviourDialog> {
  final _name = TextEditingController(text: 'NewBehaviour')
    ..selection = const TextSelection(baseOffset: 0, extentOffset: 12);

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final name = _name.text.trim();
    final ok = isBehaviourClassName(name);
    final file = ['lib', if (widget.folder.isNotEmpty) widget.folder, '${ok ? snakeCase(name) : '…'}.dart'].join('/');
    void submit() {
      if (ok) Navigator.pop(context, name);
    }

    return AlertDialog(
      key: const Key('project.newBehaviour'),
      title: const Text('New Behaviour'),
      content: SizedBox(
        width: 380,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            TextField(
              key: const Key('project.newBehaviour.name'),
              controller: _name,
              autofocus: true,
              decoration: InputDecoration(
                labelText: 'Class name',
                errorText: ok || name.isEmpty ? null : 'A capital letter, then letters and digits',
              ),
              onChanged: (_) => setState(() {}),
              onSubmitted: (_) => submit(),
            ),
            const SizedBox(height: 10),
            Text(
              '$file opens in your IDE; fynkel codegen then generates its schema (seconds).',
              style: Theme.of(context).textTheme.bodySmall?.copyWith(color: Theme.of(context).hintColor),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
        FilledButton(
          key: const Key('project.newBehaviour.create'),
          onPressed: ok ? submit : null,
          child: const Text('Create'),
        ),
      ],
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
