// The app root: preferences → theme; launcher until a project is open, then
// the main window; the command registry, its shortcuts and the menu bar
// live here because they outlive any one project.
import 'dart:async';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:path/path.dart' as p;

import 'commands/commands.dart';
import 'commands/palette.dart';
import 'console/console_panel.dart';
import 'documents/document_tab_bar.dart';
import 'documents/document_tabs.dart';
import 'documents/open_document.dart';
import 'env/tools.dart';
import 'hierarchy/hierarchy_model.dart';
import 'hierarchy/hierarchy_panel.dart';
import 'inspector/codegen_check.dart';
import 'inspector/inspector_model.dart';
import 'inspector/inspector_panel.dart';
import 'inspector/prefab_actions.dart' as prefab;
import 'launcher/launcher.dart';
import 'prefs/preferences.dart';
import 'prefs/preferences_dialog.dart';
import 'project/editor_project.dart';
import 'project/migration_dialog.dart';
import 'project_browser/asset_panel.dart';
import 'project_browser/project_browser_model.dart';
import 'project_browser/project_panel.dart';
import 'scene_view/editor_state.dart';
import 'scene_view/scene_panel.dart';
import 'scene_view/scene_view_model.dart';
import 'shell/main_window.dart';
import 'shell/menu_bar.dart';
import 'shell/panels.dart';
import 'shell/workspace.dart';

const editorVersion = '0.1.0';

/// Opens a project directory into an [EditorProject]; the real one is
/// [SessionProject.open], tests inject a fake.
typedef OpenProject = Future<EditorProject> Function(String dir);

Future<String?> pickDirectory(BuildContext context, {String? title, String? initialDirectory}) =>
    FilePicker.platform.getDirectoryPath(dialogTitle: title, initialDirectory: initialDirectory);

/// Where "Save Frame…" writes its PNG; the tests inject a path.
typedef ChooseSavePath = Future<String?> Function(String suggestedName);

Future<String?> pickSavePath(String suggestedName) => FilePicker.platform.saveFile(
  dialogTitle: 'Save Frame',
  fileName: suggestedName,
  type: FileType.custom,
  allowedExtensions: const ['png'],
);

class FynkelEditorApp extends StatelessWidget {
  const FynkelEditorApp({
    super.key,
    required this.prefs,
    required this.layoutStore,
    this.openProject = _openSessionProject,
    this.chooseDirectory = pickDirectory,
    this.chooseSavePath = pickSavePath,
    this.detectToolchain,
    this.run = runProcess,
    this.nativeMenuBar,
    this.quit,
    this.initialProject,
    this.sceneViewport = true,
  });

  final Preferences prefs;
  final LayoutStore layoutStore;
  final OpenProject openProject;
  final ChooseDirectory chooseDirectory;
  final ChooseSavePath chooseSavePath;

  /// False leaves the native texture out of the Scene view (widget tests
  /// without a Metal device drive the model through the painted layer).
  final bool sceneViewport;
  final Future<Toolchain> Function(Preferences prefs)? detectToolchain;
  final RunProcess run;
  final bool? nativeMenuBar;
  final VoidCallback? quit;

  /// Open this project at start (`fynkel editor <dir>` later; the CLI arg now).
  final String? initialProject;

  static Future<EditorProject> _openSessionProject(String dir) => SessionProject.open(dir);

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: prefs,
      builder: (context, _) {
        ThemeData themeOf(Brightness b) => ThemeData(
          colorScheme: ColorScheme.fromSeed(seedColor: const Color(0xFF3B82F6), brightness: b),
          visualDensity: VisualDensity.compact,
          useMaterial3: true,
        );
        return MaterialApp(
          title: 'Fynkel Editor',
          debugShowCheckedModeBanner: false,
          theme: themeOf(Brightness.light),
          darkTheme: themeOf(Brightness.dark),
          themeMode: switch (prefs.theme) {
            EditorTheme.system => ThemeMode.system,
            EditorTheme.light => ThemeMode.light,
            EditorTheme.dark => ThemeMode.dark,
          },
          home: EditorShell(
            prefs: prefs,
            layoutStore: layoutStore,
            openProject: openProject,
            chooseDirectory: chooseDirectory,
            chooseSavePath: chooseSavePath,
            detectToolchain: detectToolchain ?? (prefs) => Toolchain.detect(prefs: prefs, run: run),
            run: run,
            sceneViewport: sceneViewport,
            nativeMenuBar: nativeMenuBar,
            quit: quit ?? () => exit(0),
            initialProject: initialProject,
          ),
        );
      },
    );
  }
}

class EditorShell extends StatefulWidget {
  const EditorShell({
    super.key,
    required this.prefs,
    required this.layoutStore,
    required this.openProject,
    required this.chooseDirectory,
    required this.chooseSavePath,
    required this.detectToolchain,
    required this.run,
    required this.quit,
    this.nativeMenuBar,
    this.initialProject,
    this.sceneViewport = true,
  });

  final Preferences prefs;
  final LayoutStore layoutStore;
  final OpenProject openProject;
  final ChooseDirectory chooseDirectory;
  final ChooseSavePath chooseSavePath;
  final bool sceneViewport;
  final Future<Toolchain> Function(Preferences prefs) detectToolchain;
  final RunProcess run;
  final VoidCallback quit;
  final bool? nativeMenuBar;
  final String? initialProject;

  @override
  State<EditorShell> createState() => EditorShellState();
}

class EditorShellState extends State<EditorShell> {
  final registry = CommandRegistry();
  final panels = PanelRegistry(const []);
  late PanelRegistry _panels;
  Toolchain? toolchain;
  EditorProject? project;
  Workspace? workspace;
  ConsoleModel? console;
  DocumentTabs? tabs;
  HierarchyModel? hierarchy;
  InspectorModel? inspector;
  CodegenCheck? codegen;
  ProjectBrowserModel? browser;
  SceneViewModel? scene;
  EditorState? editorState;
  final consoleSearchFocus = FocusNode(debugLabel: 'console.search');
  final hierarchySearchFocus = FocusNode(debugLabel: 'hierarchy.search');
  final projectSearchFocus = FocusNode(debugLabel: 'project.search');
  late IdeLauncher ide;
  String? _layoutNotice;
  String? _openError;
  bool _opening = false;

  @override
  void initState() {
    super.initState();
    ide = IdeLauncher(widget.prefs);
    _panels = PanelRegistry(_panelSpecs());
    _registerCommands();
    widget.prefs.addListener(_onPrefsChanged);
    _detect();
    final initial = widget.initialProject;
    if (initial != null) WidgetsBinding.instance.addPostFrameCallback((_) => openProjectDir(initial));
  }

  @override
  void dispose() {
    widget.prefs.removeListener(_onPrefsChanged);
    workspace?.dispose();
    scene?.dispose();
    editorState?.dispose();
    inspector?.dispose();
    codegen?.dispose();
    browser?.dispose();
    hierarchy?.dispose();
    tabs?.dispose();
    project?.removeListener(_onProjectChanged);
    project?.close();
    consoleSearchFocus.dispose();
    hierarchySearchFocus.dispose();
    projectSearchFocus.dispose();
    registry.dispose();
    super.dispose();
  }

  void _onPrefsChanged() {
    _registerRecents();
  }

  Future<void> _detect() async {
    setState(() => toolchain = null);
    final tc = await widget.detectToolchain(widget.prefs);
    if (mounted) setState(() => toolchain = tc);
  }

  // ── Panels ──────────────────────────────────────────────────────────────

  List<PanelSpec> _panelSpecs() => [
    PanelSpec(
      id: PanelRegistry.hierarchy,
      title: 'Hierarchy',
      icon: Icons.account_tree_outlined,
      home: PanelHome.left,
      builder: (_) {
        final h = hierarchy;
        if (h == null) return const SizedBox.shrink();
        return HierarchyPanel(
          model: h,
          onOpenDocument: openDocumentDialog,
          searchFocus: hierarchySearchFocus,
          commands: registry,
        );
      },
    ),
    PanelSpec(
      id: PanelRegistry.scene,
      title: 'Scene',
      icon: Icons.view_in_ar_outlined,
      home: PanelHome.center,
      builder: (_) {
        final s = scene;
        if (s == null) return const SizedBox.shrink();
        return ScenePanel(model: s, showViewport: widget.sceneViewport);
      },
    ),
    PanelSpec(
      id: PanelRegistry.inspector,
      title: 'Inspector',
      icon: Icons.tune,
      home: PanelHome.right,
      builder: (_) {
        final i = inspector;
        if (i == null) return const SizedBox.shrink();
        return InspectorPanel(model: i);
      },
    ),
    PanelSpec(
      id: PanelRegistry.project,
      title: 'Project',
      icon: Icons.folder_outlined,
      home: PanelHome.bottom,
      builder: (_) {
        final b = browser;
        if (b == null) return const SizedBox.shrink();
        return ProjectPanel(
          model: b,
          onOpenDocument: openDocument,
          onShowAsset: showAssetPanel,
          run: widget.run,
          searchFocus: projectSearchFocus,
        );
      },
    ),
    PanelSpec(
      id: PanelRegistry.console,
      title: 'Console',
      icon: Icons.terminal,
      home: PanelHome.bottom,
      builder: (_) {
        final c = console;
        if (c == null) return const SizedBox.shrink();
        return ConsolePanel(model: c, searchFocus: consoleSearchFocus);
      },
    ),
    PanelSpec(
      id: PanelRegistry.profiler,
      title: 'Profiler',
      icon: Icons.speed,
      home: PanelHome.bottom,
      builder: (_) =>
          const ComingPanel(title: 'Profiler', item: 9, what: 'Frame time, ticks, bridge bytes.', icon: Icons.speed),
    ),
    PanelSpec(
      id: PanelRegistry.animation,
      title: 'Animation',
      icon: Icons.animation,
      home: PanelHome.bottom,
      builder: (_) => const ComingPanel(
        title: 'Animation',
        item: 6,
        what: 'Clips and the pose of the selection.',
        icon: Icons.animation,
      ),
    ),
    PanelSpec(
      id: PanelRegistry.import,
      title: 'Asset',
      icon: Icons.inventory_2_outlined,
      home: PanelHome.bottom,
      builder: (_) {
        final b = browser;
        if (b == null) return const SizedBox.shrink();
        return AssetPanel(model: b, onOpenDocument: openDocument, onOpenReference: openReference, run: widget.run);
      },
    ),
  ];

  // ── Commands ────────────────────────────────────────────────────────────

  bool get _hasProject => project != null;
  OpenDocument? get _focusedTab => tabs?.focused;
  bool get _hasTab => _focusedTab != null;
  bool get _canEdit => hierarchy?.canEdit ?? false;
  bool get _hasSelection => hierarchy?.hasSelection ?? false;
  bool get _hasView => scene?.hasView ?? false;
  bool get _canEditAssets => browser?.canEdit ?? false;

  /// Prefab mode: the one prefab instance the selection is about (the
  /// anchor itself or its derived entities), or null.
  String? get _selectionAnchor => prefab.selectionAnchor(tabs?.focused);
  PrefabExpansion? get _selectionExpansion {
    final a = _selectionAnchor;
    return a == null ? null : tabs?.focused?.expansionOf(a);
  }

  AssetEntry? get _leadAsset => browser?.leadEntry;
  bool get _hasAssetSelection => browser?.selection.isNotEmpty ?? false;

  void _registerCommands() {
    SingleActivator k(LogicalKeyboardKey key, {bool shift = false, bool alt = false, bool control = false}) =>
        SingleActivator(key, meta: true, shift: shift, alt: alt, control: control);
    registry.registerAll([
      // ── Documents (Project menu, group 1) ──
      EditorCommand(
        id: 'document.open',
        baseLabel: 'Open Document…',
        description: 'A scene or prefab of the project, into a tab',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyO, shift: true),
        group: 1,
        order: 0,
        enabled: () => _hasProject,
        run: openDocumentDialog,
      ),
      EditorCommand(
        id: 'document.save',
        baseLabel: 'Save Document',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyS),
        group: 1,
        order: 1,
        enabled: () => _focusedTab?.dirty ?? false,
        run: () => saveTab(_focusedTab!),
      ),
      EditorCommand(
        id: 'document.saveAs',
        baseLabel: 'Save Document As…',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyS, shift: true),
        group: 1,
        order: 2,
        enabled: () => _hasTab,
        run: () => saveTabAs(_focusedTab!),
      ),
      EditorCommand(
        id: 'document.saveAll',
        baseLabel: 'Save All Documents',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyS, alt: true),
        group: 1,
        order: 3,
        enabled: () => tabs?.anyDirty ?? false,
        run: saveAllTabs,
      ),
      EditorCommand(
        id: 'document.revert',
        baseLabel: 'Revert Document to Saved',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyR, alt: true),
        group: 1,
        order: 4,
        enabled: () => _focusedTab?.dirty ?? false,
        run: () => revertTab(_focusedTab!),
      ),
      EditorCommand(
        id: 'document.close',
        baseLabel: 'Close Document',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyW),
        group: 1,
        order: 5,
        enabled: () => _hasTab,
        run: () => closeTab(_focusedTab!),
      ),
      EditorCommand(
        id: 'document.next',
        baseLabel: 'Next Document',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.arrowRight, alt: true),
        group: 1,
        order: 6,
        enabled: () => (tabs?.open.length ?? 0) > 1,
        run: () => tabs?.focusNext(),
      ),
      EditorCommand(
        id: 'document.previous',
        baseLabel: 'Previous Document',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.arrowLeft, alt: true),
        group: 1,
        order: 7,
        enabled: () => (tabs?.open.length ?? 0) > 1,
        run: () => tabs?.focusPrevious(),
      ),
      // ── Edit menu: undo/redo (group 0), the Hierarchy (groups 1–2) ──
      EditorCommand(
        id: 'document.undo',
        baseLabel: 'Undo',
        labelBuilder: () => _focusedTab?.canUndoLabel ?? 'Undo',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyZ),
        group: 0,
        order: 0,
        enabled: () => _focusedTab?.document.canUndo ?? false,
        textIntent: const UndoTextIntent(SelectionChangedCause.keyboard),
        run: () => _focusedTab?.undo(),
      ),
      EditorCommand(
        id: 'document.redo',
        baseLabel: 'Redo',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyZ, shift: true),
        group: 0,
        order: 1,
        enabled: () => _focusedTab?.document.canRedo ?? false,
        textIntent: const RedoTextIntent(SelectionChangedCause.keyboard),
        run: () => _focusedTab?.redo(),
      ),
      EditorCommand(
        id: 'assets.undo',
        baseLabel: 'Undo Asset Operation',
        labelBuilder: () {
          final l = browser?.history.undoLabel;
          return l == null ? 'Undo Asset Operation' : 'Undo $l';
        },
        description: 'Takes back the last rename, move, delete, new document or import-settings change',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyZ, alt: true),
        group: 0,
        order: 2,
        enabled: () => _canEditAssets && (browser?.history.canUndo ?? false),
        run: () => browser?.undo(),
      ),
      EditorCommand(
        id: 'assets.redo',
        baseLabel: 'Redo Asset Operation',
        labelBuilder: () {
          final l = browser?.history.redoLabel;
          return l == null ? 'Redo Asset Operation' : 'Redo $l';
        },
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyZ, shift: true, alt: true),
        group: 0,
        order: 3,
        enabled: () => _canEditAssets && (browser?.history.canRedo ?? false),
        run: () => browser?.redo(),
      ),
      EditorCommand(
        id: 'hierarchy.rename',
        baseLabel: 'Rename Entity',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyR),
        group: 1,
        order: 0,
        enabled: () => _canEdit && (hierarchy?.selection.length ?? 0) == 1,
        run: () => hierarchy?.beginRename(),
      ),
      EditorCommand(
        id: 'hierarchy.duplicate',
        baseLabel: 'Duplicate',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyD),
        group: 1,
        order: 1,
        enabled: () => _canEdit && _hasSelection,
        run: () => hierarchy?.duplicate(),
      ),
      EditorCommand(
        id: 'hierarchy.delete',
        baseLabel: 'Delete',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.backspace),
        group: 1,
        order: 2,
        enabled: () => _canEdit && _hasSelection,
        textIntent: const DeleteToLineBreakIntent(forward: false),
        run: () => hierarchy?.delete(),
      ),
      EditorCommand(
        id: 'hierarchy.createEmpty',
        baseLabel: 'Create Empty Entity',
        description: 'Under the selected entity, or at the root',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyN, alt: true),
        group: 1,
        order: 3,
        enabled: () => _canEdit,
        run: () => hierarchy?.createEmpty(),
      ),
      EditorCommand(
        id: 'hierarchy.createWith',
        baseLabel: 'Create Entity with Component…',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyN, shift: true, alt: true),
        group: 1,
        order: 4,
        enabled: () => _canEdit,
        run: createWithDialog,
      ),
      EditorCommand(
        id: 'hierarchy.keepWorldTransform',
        baseLabel: 'Keep World Transform when Reparenting',
        labelBuilder: () =>
            '${(hierarchy?.keepWorldTransform ?? true) ? '✓ ' : ''}Keep World Transform when Reparenting',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyK, alt: true),
        group: 1,
        order: 5,
        enabled: () => _hasProject,
        run: () => hierarchy?.toggleKeepWorldTransform(),
      ),
      EditorCommand(
        id: 'hierarchy.search',
        baseLabel: 'Search Hierarchy',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyH, alt: true),
        group: 2,
        order: 0,
        enabled: () => _hasTab,
        run: () {
          workspace?.show(PanelRegistry.hierarchy);
          hierarchy?.requestSearchFocus();
        },
      ),
      EditorCommand(
        id: 'hierarchy.expandAll',
        baseLabel: 'Expand All',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.bracketRight, alt: true),
        group: 2,
        order: 1,
        enabled: () => _hasTab,
        run: () => hierarchy?.expandAll(),
      ),
      EditorCommand(
        id: 'hierarchy.collapseAll',
        baseLabel: 'Collapse All',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.bracketLeft, alt: true),
        group: 2,
        order: 2,
        enabled: () => _hasTab,
        run: () => hierarchy?.collapseAll(),
      ),
      // ── Inspector (Edit menu, group 3) ──
      EditorCommand(
        id: 'inspector.addComponent',
        baseLabel: 'Add Component…',
        description: 'A native component or a behaviour, on every selected entity',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyA, shift: true),
        group: 3,
        order: 0,
        enabled: () => _canEdit && _hasSelection && !(inspector?.tooMany ?? true),
        run: () {
          final i = inspector;
          if (i != null) showAddComponent(context, i);
        },
      ),
      EditorCommand(
        id: 'inspector.openScript',
        baseLabel: 'Open Script',
        description: "The selected entity's first behaviour, in the IDE",
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.enter, alt: true),
        group: 3,
        order: 1,
        enabled: () => inspector?.slots.any((s) => s.spec.isBehaviour) ?? false,
        run: () async {
          final i = inspector;
          if (i == null) return;
          for (final s in i.slots) {
            if (!s.spec.isBehaviour) continue;
            final err = await i.openScript(s);
            if (err != null) _showError(err);
            return;
          }
        },
      ),
      // ── Prefab mode (Edit menu, group 4) ──
      EditorCommand(
        id: 'prefab.applyOverrides',
        baseLabel: 'Apply to Prefab',
        description: 'The selected instance\'s overrides become the prefab\'s values, then are reverted here',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyA, shift: true, alt: true),
        group: 4,
        order: 0,
        enabled: () {
          final a = _selectionAnchor;
          return _canEdit && a != null && (inspector?.cannotApply(a) == null);
        },
        run: () {
          final a = _selectionAnchor;
          if (a == null) return;
          if (!(inspector?.applyOverrides(a) ?? false) && inspector?.lastError != null) {
            _showError(inspector!.lastError!);
          }
        },
      ),
      EditorCommand(
        id: 'prefab.revertOverrides',
        baseLabel: 'Revert Prefab Overrides',
        description: 'Drops every override of the selected instance — back to the prefab',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyU, shift: true, alt: true),
        group: 4,
        order: 1,
        enabled: () => _canEdit && (_selectionExpansion?.overrides.isNotEmpty ?? false),
        run: () {
          final a = _selectionAnchor;
          if (a != null) inspector?.revertOverride(a);
        },
      ),
      EditorCommand(
        id: 'prefab.unpack',
        baseLabel: 'Unpack Prefab Instance',
        description: 'The instance becomes plain entities of this document (one undo step)',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyX, shift: true, alt: true),
        group: 4,
        order: 2,
        enabled: () {
          final a = _selectionAnchor;
          return _canEdit && a != null && (inspector?.cannotUnpack(a) == null);
        },
        run: () {
          final a = _selectionAnchor;
          if (a == null) return;
          if (!(inspector?.unpack(a) ?? false) && inspector?.lastError != null) _showError(inspector!.lastError!);
        },
      ),
      EditorCommand(
        id: 'prefab.open',
        baseLabel: 'Open Prefab',
        description: 'The selected instance\'s prefab in its own tab (an imported hierarchy read-only)',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyD, shift: true, alt: true),
        group: 4,
        order: 3,
        enabled: () => _selectionExpansion?.prefab != null,
        run: () {
          final a = _selectionAnchor;
          if (a == null) return;
          if (!(inspector?.openPrefab(a) ?? false) && inspector?.lastError != null) _showError(inspector!.lastError!);
        },
      ),
      EditorCommand(
        id: 'prefab.selectAsset',
        baseLabel: 'Select Prefab Asset',
        description: 'Reveals the selected instance\'s prefab (or model source) in the Project browser',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyL, shift: true, alt: true),
        group: 4,
        order: 4,
        enabled: () => _selectionAnchor != null && browser != null,
        run: () {
          final a = _selectionAnchor;
          if (a != null) inspector?.selectPrefabAsset(a);
        },
      ),
      EditorCommand(
        id: 'inspector.checkCodegen',
        baseLabel: 'Check Generated Behaviour Files',
        description: 'Runs `fynkel codegen --check` on the project (never writes)',
        menu: EditorMenu.project,
        group: 3,
        order: 5,
        shortcut: k(LogicalKeyboardKey.keyG, shift: true),
        enabled: () => codegen != null && codegen!.status != CodegenStatus.running,
        run: () => codegen?.check(),
      ),
      // ── Assets (Project menu, group 4) ──
      EditorCommand(
        id: 'assets.newScene',
        baseLabel: 'New Scene',
        description: 'An empty scene document in the Project browser\'s folder',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyS, shift: true, alt: true),
        group: 4,
        order: 0,
        enabled: () => _canEditAssets,
        run: () => _newAsset('scene'),
      ),
      EditorCommand(
        id: 'assets.newPrefab',
        baseLabel: 'New Prefab',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyP, shift: true, alt: true),
        group: 4,
        order: 1,
        enabled: () => _canEditAssets,
        run: () => _newAsset('prefab'),
      ),
      EditorCommand(
        id: 'assets.newMaterial',
        baseLabel: 'New Material',
        description: 'A fynkel.pbr@1 material with the shader\'s defaults',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyM, shift: true, alt: true),
        group: 4,
        order: 2,
        enabled: () => _canEditAssets,
        run: () => _newAsset('material'),
      ),
      EditorCommand(
        id: 'assets.newFolder',
        baseLabel: 'New Folder',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyF, shift: true, alt: true),
        group: 4,
        order: 3,
        enabled: () => _canEditAssets,
        run: () {
          workspace?.show(PanelRegistry.project);
          browser?.newFolder();
        },
      ),
      EditorCommand(
        id: 'assets.open',
        baseLabel: 'Open Asset',
        description: 'The selected scene or prefab into a tab; any other asset into the Asset panel',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyO, shift: true, alt: true),
        group: 4,
        order: 4,
        enabled: () => _leadAsset != null,
        run: () {
          final a = _leadAsset;
          if (a == null) return;
          if (a.document == 'scene' || a.document == 'prefab') {
            openDocument(a.id);
          } else {
            showAssetPanel();
          }
        },
      ),
      EditorCommand(
        id: 'assets.rename',
        baseLabel: 'Rename Asset',
        description: 'The file and its sidecar move together; the id and every reference survive',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyR, shift: true, alt: true),
        group: 4,
        order: 5,
        enabled: () => _canEditAssets && (browser?.selection.length ?? 0) == 1,
        run: () {
          workspace?.show(PanelRegistry.project);
          browser?.beginRename();
        },
      ),
      EditorCommand(
        id: 'assets.move',
        baseLabel: 'Move Asset to Folder…',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyV, shift: true, alt: true),
        group: 4,
        order: 6,
        enabled: () => _canEditAssets && _hasAssetSelection,
        run: moveAssetsDialog,
      ),
      EditorCommand(
        id: 'assets.delete',
        baseLabel: 'Delete Asset',
        description: 'To .fynkel/trash; refused while referenced unless you say "delete anyway"',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.backspace, shift: true, alt: true),
        group: 4,
        order: 7,
        enabled: () => _canEditAssets && _hasAssetSelection,
        run: () {
          workspace?.show(PanelRegistry.project);
          browser?.delete();
        },
      ),
      EditorCommand(
        id: 'assets.reimport',
        baseLabel: 'Reimport Asset',
        description: 'Runs the selected source through its importer with the sidecar\'s settings',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyI, shift: true, alt: true),
        group: 4,
        order: 8,
        enabled: () => _canEditAssets && (_leadAsset?.isSource ?? false),
        run: () {
          final a = _leadAsset;
          if (a == null) return;
          showAssetPanel();
          final r = browser?.reimport(a.id);
          if (r != null && !r.ok) _showError('Reimport refused: ${r.diagnostics.map((d) => d.message).join('; ')}');
        },
      ),
      EditorCommand(
        id: 'assets.reveal',
        baseLabel: 'Reveal Asset in Finder',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyE, shift: true, alt: true),
        group: 4,
        order: 9,
        enabled: () => _leadAsset != null,
        run: () async {
          final a = _leadAsset;
          if (a == null) return;
          final err = await revealInFileBrowser(project!.assetFile(a.path), run: widget.run);
          if (err != null) _showError('Reveal in Finder failed: $err');
        },
      ),
      EditorCommand(
        id: 'assets.copyId',
        baseLabel: 'Copy Asset Id',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyC, shift: true, alt: true),
        group: 4,
        order: 10,
        enabled: () => _leadAsset != null,
        run: () {
          final a = _leadAsset;
          if (a != null) Clipboard.setData(ClipboardData(text: a.id));
        },
      ),
      EditorCommand(
        id: 'assets.search',
        baseLabel: 'Search Project',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyP, alt: true),
        group: 4,
        order: 11,
        enabled: () => _hasProject,
        run: () {
          workspace?.show(PanelRegistry.project);
          browser?.requestSearchFocus();
        },
      ),
      EditorCommand(
        id: 'assets.toggleView',
        baseLabel: 'Toggle Project Folders / List',
        labelBuilder: () => browser?.view == BrowserView.list ? 'Show Project as Folders' : 'Show Project as One List',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyT, shift: true, alt: true),
        group: 4,
        order: 12,
        enabled: () => _hasProject,
        run: () {
          workspace?.show(PanelRegistry.project);
          browser?.toggleView();
        },
      ),
      EditorCommand(
        id: 'project.new',
        baseLabel: 'New Project…',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyN, shift: true),
        group: 0,
        order: 0,
        run: newProject,
      ),
      EditorCommand(
        id: 'project.open',
        baseLabel: 'Open Project…',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyO),
        group: 0,
        order: 1,
        run: openProjectDialog,
      ),
      EditorCommand(
        id: 'project.close',
        baseLabel: 'Close Project',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyW, shift: true),
        group: 2,
        order: 0,
        enabled: () => _hasProject,
        run: closeProject,
      ),
      EditorCommand(
        id: 'project.revalidate',
        baseLabel: 'Validate Project',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyV, alt: true),
        group: 2,
        order: 1,
        enabled: () => _hasProject && !(project?.validating ?? true),
        run: () => project?.revalidate(),
      ),
      EditorCommand(
        id: 'project.migrations',
        baseLabel: 'Migration Preview…',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyM, alt: true),
        group: 2,
        order: 3,
        enabled: () => _hasProject,
        run: () => showMigrationPreview(context, project!),
      ),
      EditorCommand(
        id: 'project.reveal',
        baseLabel: 'Reveal in Finder',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyR, shift: true),
        group: 2,
        order: 2,
        enabled: () => _hasProject,
        run: () => revealInFileBrowser(p.join(project!.dir, 'fynkel.project.json'), run: widget.run),
      ),
      EditorCommand(
        id: 'app.quit',
        baseLabel: 'Quit Fynkel Editor',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyQ),
        group: 9,
        run: widget.quit,
      ),
      // ── Scene view: tools and the editor-only flags (Edit menu, group 2) ──
      for (final (i, tool, label, key) in const [
        (0, SceneTool.select, 'Select Tool', LogicalKeyboardKey.digit1),
        (1, SceneTool.move, 'Move Tool', LogicalKeyboardKey.digit2),
        (2, SceneTool.rotate, 'Rotate Tool', LogicalKeyboardKey.digit3),
        (3, SceneTool.scale, 'Scale Tool', LogicalKeyboardKey.digit4),
      ])
        EditorCommand(
          id: 'scene.tool.${tool.name}',
          baseLabel: label,
          description: 'The Scene view\'s ${tool.name} tool (also ${'QWER'[i]} while the view has focus)',
          menu: EditorMenu.edit,
          shortcut: k(key),
          group: 2,
          order: i,
          enabled: () => _hasView,
          run: () => scene?.tool = tool,
        ),
      EditorCommand(
        id: 'scene.toggleSpace',
        baseLabel: 'Toggle Local / World Axes',
        labelBuilder: () => scene?.space == GizmoSpace.local ? 'Use World Axes' : 'Use Local Axes',
        description: 'The gizmo\'s axes: the entity\'s own or the world\'s (X in the view)',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyL),
        group: 2,
        order: 4,
        enabled: () => _hasView,
        run: () => scene?.toggleSpace(),
      ),
      EditorCommand(
        id: 'scene.toggleSnap',
        baseLabel: 'Toggle Snapping',
        labelBuilder: () => scene?.snap.enabled ?? false ? 'Snapping Off' : 'Snapping On',
        description: 'Move, rotate and scale in steps (hold ⌘ during a drag to invert)',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyG, alt: true),
        group: 2,
        order: 5,
        enabled: () => _hasView,
        run: () => scene?.toggleSnap(),
      ),
      EditorCommand(
        id: 'scene.toggleHidden',
        baseLabel: 'Hide / Show Selected',
        description: 'Editor-only: out of the picture and of picking, never in the scene file (H in the view)',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyH, shift: true),
        group: 3,
        order: 0,
        enabled: () => _hasView && _hasSelection,
        run: () => scene?.toggleHiddenSelection(),
      ),
      EditorCommand(
        id: 'scene.showAll',
        baseLabel: 'Show All Hidden',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyU, shift: true),
        group: 3,
        order: 1,
        enabled: () => scene?.hasHidden ?? false,
        run: () => scene?.showAll(),
      ),
      EditorCommand(
        id: 'scene.toggleLocked',
        baseLabel: 'Lock / Unlock Selected',
        description: 'Editor-only: a locked entity is not picked or dragged in the Scene view (L in the view)',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyK),
        group: 3,
        order: 2,
        enabled: () => _hasView && _hasSelection,
        run: () => scene?.toggleLockedSelection(),
      ),
      EditorCommand(
        id: 'scene.alignCameraToView',
        baseLabel: 'Align Selected Camera to View',
        description: 'The selected camera entity moves to where the view is, one transaction',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyL, shift: true),
        group: 3,
        order: 3,
        enabled: () => _canEdit && (scene?.canAlignToSelectedCamera ?? false),
        run: () => scene?.alignSelectedCameraToView(),
      ),
      EditorCommand(
        id: 'app.preferences',
        baseLabel: 'Preferences…',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.comma),
        group: 9,
        run: showPreferences,
      ),
      // ── View menu: the Scene view's camera and overlays ──
      EditorCommand(
        id: 'view.frameSelected',
        baseLabel: 'Frame Selected',
        description: 'The camera swings to fit the selection (everything when nothing is selected); F in the view',
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.keyF),
        group: 0,
        order: 0,
        enabled: () => _hasView,
        run: () => scene?.frameSelected(),
      ),
      EditorCommand(
        id: 'view.frameAll',
        baseLabel: 'Frame All',
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.keyF, shift: true),
        group: 0,
        order: 1,
        enabled: () => _hasView,
        run: () => scene?.frameAll(),
      ),
      EditorCommand(
        id: 'view.toggleProjection',
        baseLabel: 'Toggle Perspective / Orthographic',
        labelBuilder: () => scene?.camera?.orthographic ?? false ? 'Perspective' : 'Orthographic',
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.digit5),
        group: 0,
        order: 2,
        enabled: () => _hasView,
        run: () => scene?.toggleProjection(),
      ),
      EditorCommand(
        id: 'view.alignToCamera',
        baseLabel: 'Look Through Selected Camera',
        description: 'The view takes the selected camera\'s placement and lens — the picture is what it renders',
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.keyL, alt: true),
        group: 0,
        order: 3,
        enabled: () => scene?.canAlignToSelectedCamera ?? false,
        run: () => scene?.alignViewToSelectedCamera(),
      ),
      EditorCommand(
        id: 'view.reset',
        baseLabel: 'Reset View',
        description: 'The editor camera back to its starting place',
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.digit0, shift: true),
        group: 0,
        order: 4,
        enabled: () => _hasView,
        run: () => scene?.resetView(),
      ),
      EditorCommand(
        id: 'view.toggleGrid',
        baseLabel: 'Toggle Grid and Axes',
        labelBuilder: () => scene?.showGrid ?? true ? 'Hide Grid and Axes' : 'Show Grid and Axes',
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.keyG),
        group: 1,
        order: 0,
        enabled: () => _hasView,
        run: () => scene?.toggleGrid(),
      ),
      EditorCommand(
        id: 'view.toggleOverlays',
        baseLabel: 'Toggle Light and Camera Icons',
        labelBuilder: () => scene?.showOverlays ?? true ? 'Hide Light and Camera Icons' : 'Show Light and Camera Icons',
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.keyI),
        group: 1,
        order: 1,
        enabled: () => _hasView,
        run: () => scene?.toggleOverlays(),
      ),
      EditorCommand(
        id: 'view.toggleStats',
        baseLabel: 'Toggle Stats Overlay',
        labelBuilder: () => scene?.showStats ?? false ? 'Hide Stats Overlay' : 'Show Stats Overlay',
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.keyT),
        group: 1,
        order: 2,
        enabled: () => _hasView,
        run: () => scene?.toggleStats(),
      ),
      EditorCommand(
        id: 'view.reload',
        baseLabel: 'Reload Preview',
        description:
            'The preview world emptied and loaded fresh from the document (the fallback the sync uses on its own)',
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.keyR, control: true),
        group: 2,
        order: 0,
        enabled: () => _hasTab && (_focusedTab!.kind == 'scene' || _focusedTab!.kind == 'prefab'),
        run: () => scene?.reload(),
      ),
      EditorCommand(
        id: 'view.recook',
        baseLabel: 'Rebuild Asset Cache and Reload',
        description: '`fynkel cook` in process, then every preview reloaded',
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.keyR, control: true, shift: true),
        group: 2,
        order: 1,
        enabled: () => _hasView,
        run: () => scene?.recook(),
      ),
      EditorCommand(
        id: 'view.saveFrame',
        baseLabel: 'Save Frame…',
        description:
            'One frame of the Scene view as a PNG at the viewport\'s size (the pixel gate against `fynkel render`)',
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.keyX, alt: true),
        group: 2,
        order: 2,
        enabled: () => _hasView,
        run: () => unawaited(scene?.saveFrame()),
      ),
      EditorCommand(
        id: 'window.commandPalette',
        baseLabel: 'Command Palette…',
        menu: EditorMenu.window,
        shortcut: k(LogicalKeyboardKey.keyP, shift: true),
        group: 0,
        run: showPalette,
      ),
      EditorCommand(
        id: 'window.resetLayout',
        baseLabel: 'Reset Layout',
        menu: EditorMenu.window,
        shortcut: k(LogicalKeyboardKey.digit0, alt: true),
        group: 1,
        enabled: () => _hasProject,
        run: resetLayout,
      ),
      for (final (i, id) in PanelRegistry.defaultOrder.indexed)
        EditorCommand(
          id: 'window.panel.$id',
          baseLabel: 'Toggle ${_panels.byId(id)!.title}',
          labelBuilder: () => '${workspace?.isVisible(id) ?? true ? 'Hide' : 'Show'} ${_panels.byId(id)!.title}',
          menu: EditorMenu.window,
          shortcut: k(_digit(i + 1), alt: true),
          group: 2,
          order: i,
          enabled: () => _hasProject,
          run: () => togglePanel(id),
        ),
      EditorCommand(
        id: 'console.focusSearch',
        baseLabel: 'Search Console',
        menu: EditorMenu.window,
        shortcut: k(LogicalKeyboardKey.keyF, alt: true),
        group: 3,
        order: 0,
        enabled: () => _hasProject,
        run: () {
          workspace?.show(PanelRegistry.console);
          WidgetsBinding.instance.addPostFrameCallback((_) => consoleSearchFocus.requestFocus());
        },
      ),
      EditorCommand(
        id: 'console.copy',
        baseLabel: 'Copy Console',
        menu: EditorMenu.window,
        shortcut: k(LogicalKeyboardKey.keyC, alt: true),
        group: 3,
        order: 1,
        enabled: () => _hasProject,
        run: () => console?.copy(),
      ),
      EditorCommand(
        id: 'console.openInIde',
        baseLabel: 'Open Console Row in IDE',
        menu: EditorMenu.window,
        shortcut: k(LogicalKeyboardKey.keyO, alt: true),
        group: 3,
        order: 2,
        enabled: () => console?.canOpenSelected ?? false,
        run: () => console?.openSelected(),
      ),
      EditorCommand(
        id: 'console.openInEditor',
        baseLabel: 'Open Console Row in Editor',
        description: 'The diagnostic\'s scene or prefab into a tab, its entity selected',
        menu: EditorMenu.window,
        shortcut: k(LogicalKeyboardKey.keyE, alt: true),
        group: 3,
        order: 3,
        enabled: () => console?.canOpenSelectedInEditor ?? false,
        run: () => console?.openSelectedInEditor(),
      ),
      EditorCommand(
        id: 'help.documentation',
        baseLabel: 'Editor Documentation',
        menu: EditorMenu.help,
        shortcut: k(LogicalKeyboardKey.keyD, alt: true),
        enabled: () => toolchain?.fynkelRoot != null,
        run: () => revealInFileBrowser(p.join(toolchain!.fynkelRoot!, 'docs', 'editor.md'), run: widget.run),
      ),
      EditorCommand(
        id: 'help.about',
        baseLabel: 'About Fynkel Editor',
        menu: EditorMenu.help,
        shortcut: k(LogicalKeyboardKey.keyA, alt: true),
        run: showAbout,
      ),
    ]);
    _registerRecents();
  }

  static LogicalKeyboardKey _digit(int n) => switch (n) {
    1 => LogicalKeyboardKey.digit1,
    2 => LogicalKeyboardKey.digit2,
    3 => LogicalKeyboardKey.digit3,
    4 => LogicalKeyboardKey.digit4,
    5 => LogicalKeyboardKey.digit5,
    6 => LogicalKeyboardKey.digit6,
    7 => LogicalKeyboardKey.digit7,
    8 => LogicalKeyboardKey.digit8,
    _ => LogicalKeyboardKey.digit9,
  };

  /// One generated command per recent project (no shortcut by design).
  void _registerRecents() {
    registry.unregisterPrefix('project.recent.');
    registry.registerAll([
      for (final (i, r) in widget.prefs.recents.indexed)
        EditorCommand(
          id: 'project.recent.$i',
          baseLabel: 'Open Recent: ${r.name.isEmpty ? p.basename(r.dir) : r.name}',
          description: r.dir,
          menu: EditorMenu.project,
          group: 0,
          order: 10 + i,
          generated: true,
          enabled: () => !p.equals(r.dir, project?.dir ?? '') && r.exists,
          run: () => openProjectDir(r.dir),
        ),
    ]);
  }

  // ── Actions ─────────────────────────────────────────────────────────────

  Future<void> openProjectDialog() async {
    final dir = await widget.chooseDirectory(context, title: 'Open a Fynkel project');
    if (dir == null) return;
    if (!isFynkelProject(dir)) {
      _showError('$dir is not a Fynkel project (no fynkel.project.json).');
      return;
    }
    await openProjectDir(dir);
  }

  Future<void> newProject() async {
    final tc = toolchain;
    if (tc == null || !tc.canCreate) {
      _showError('New Project needs the Fynkel CLI and checkout — the launcher\'s checks say what is missing.');
      return;
    }
    final created = await showDialog<String>(
      context: context,
      builder: (_) => NewProjectDialog(toolchain: tc, chooseDirectory: widget.chooseDirectory, run: widget.run),
    );
    if (created != null) await openProjectDir(created);
  }

  Future<void> openProjectDir(String dir) async {
    if (_opening) return;
    _opening = true;
    try {
      final opened = await widget.openProject(dir);
      if (!mounted) {
        await opened.close();
        return;
      }
      await project?.close();
      project?.removeListener(_onProjectChanged);
      workspace?.dispose();
      scene?.dispose();
      editorState?.dispose();
      inspector?.dispose();
      codegen?.dispose();
      browser?.dispose();
      hierarchy?.dispose();
      tabs?.dispose();
      final ws = Workspace(registry: _panels, store: widget.layoutStore);
      final t = DocumentTabs(project: opened);
      final h = HierarchyModel(tabs: t, project: opened);
      final c = ConsoleModel(project: opened, openInIde: ide.open, openInEditor: openDiagnosticInEditor);
      final cg = CodegenCheck(
        projectDir: opened.dir,
        run: widget.run,
        cli: () => toolchain?.fynkelCli,
        repoRoot: () => toolchain?.fynkelRoot,
      );
      final ins = InspectorModel(tabs: t, project: opened, codegen: cg, openInIde: ide.open);
      final b = ProjectBrowserModel(project: opened);
      ins.revealAsset = (asset, sub) {
        workspace?.show(PanelRegistry.project);
        b.reveal(asset);
      };
      final es = EditorState.load(opened.dir);
      final sv = SceneViewModel(
        tabs: t,
        project: opened,
        hierarchy: h,
        editorState: es,
        chooseSavePath: widget.chooseSavePath,
      );
      opened.addListener(_onProjectChanged);
      c.addListener(registry.refresh);
      t.addListener(registry.refresh);
      h.addListener(registry.refresh);
      ins.addListener(registry.refresh);
      b.addListener(registry.refresh);
      sv.addListener(registry.refresh);
      setState(() {
        project = opened;
        workspace = ws;
        tabs = t;
        hierarchy = h;
        inspector = ins;
        codegen = cg;
        browser = b;
        console = c;
        editorState = es;
        scene = sv;
        _layoutNotice = ws.restoreProblem == null
            ? null
            : 'The saved layout was replaced by the default: ${ws.restoreProblem}.';
        if (es.loadProblem != null) {
          _layoutNotice =
              '${_layoutNotice ?? ''}${_layoutNotice == null ? '' : ' '}'
              '.fynkel/editor.json did not parse and starts fresh: ${es.loadProblem}.';
        }
        _openError = null;
      });
      widget.prefs.addRecent(opened.dir, opened.name);
      registry.refresh();
      // A project with Dart behaviours gets its codegen check once at open, in the background.
      if (opened.behaviourMetadata() != null && cg.isDartPackage) unawaited(cg.check());
    } on AuthoringException catch (e) {
      final reasons = e.diagnostics.map((d) => '${d.code}: ${d.message}').join('\n');
      _showError('Could not open $dir.\n${reasons.isEmpty ? e : reasons}');
    } catch (e) {
      _showError('Could not open $dir: $e');
    } finally {
      _opening = false;
    }
  }

  void _onProjectChanged() => registry.refresh();

  /// Closes the project; every dirty tab gets its save prompt first, and a
  /// Cancel there keeps the project open.
  Future<void> closeProject() async {
    final closing = project;
    if (closing == null) return;
    for (final tab in List.of(tabs?.dirtyDocuments ?? const <OpenDocument>[])) {
      if (!await closeTab(tab)) return;
    }
    workspace?.save();
    closing.removeListener(_onProjectChanged);
    final h = hierarchy;
    final t = tabs;
    final ins = inspector;
    final cg = codegen;
    final b = browser;
    final sv = scene;
    final es = editorState;
    setState(() {
      project = null;
      console = null;
      hierarchy = null;
      inspector = null;
      codegen = null;
      browser = null;
      tabs = null;
      scene = null;
      editorState = null;
      workspace?.dispose();
      workspace = null;
    });
    sv?.dispose();
    es?.dispose();
    ins?.dispose();
    cg?.dispose();
    b?.dispose();
    h?.dispose();
    t?.dispose();
    await closing.close();
    registry.refresh();
  }

  // ── Documents ───────────────────────────────────────────────────────────

  Future<void> openDocumentDialog() async {
    final pr = project;
    if (pr == null) return;
    final asset = await showOpenDocumentDialog(context, pr);
    if (asset == null) return;
    openDocument(asset.id);
  }

  /// Opens (or focuses) a document by path or id; shows the refusal if any.
  OpenDocument? openDocument(String pathOrId) {
    final t = tabs;
    if (t == null) return null;
    final tab = t.openDocument(pathOrId);
    if (tab == null && t.openError != null) _showError(t.openError!);
    registry.refresh();
    return tab;
  }

  /// A Console row's document into a tab, its entity revealed and selected.
  void openDiagnosticInEditor(Diagnostic d) {
    final doc = d.document;
    if (doc == null) return;
    final tab = openDocument(doc);
    if (tab == null) return;
    workspace?.show(PanelRegistry.hierarchy);
    final id = d.id ?? RegExp(r'^/entities/([^/]+)').firstMatch(d.pointer ?? '')?.group(1);
    if (id != null) hierarchy?.revealAndSelect(id);
  }

  // ── Assets ──────────────────────────────────────────────────────────────

  void showAssetPanel() {
    workspace?.show(PanelRegistry.import);
    registry.refresh();
  }

  void _newAsset(String kind) {
    workspace?.show(PanelRegistry.project);
    browser?.newDocument(kind);
  }

  /// The Project browser's "Move to…" for the selection.
  Future<void> moveAssetsDialog() async {
    final b = browser;
    if (b == null || b.selection.isEmpty) return;
    final ids = b.selection.toList();
    final folders = ['', ...b.folders];
    final chosen = await showDialog<String>(
      context: context,
      builder: (context) => SimpleDialog(
        key: const Key('project.moveTo'),
        title: Text(
          ids.length == 1 ? 'Move ${p.basename(b.entry(ids.single)?.path ?? '')} to…' : 'Move ${ids.length} assets to…',
        ),
        children: [
          for (final f in folders)
            SimpleDialogOption(
              key: Key('project.moveTo.${f.isEmpty ? 'root' : f}'),
              onPressed: () => Navigator.pop(context, f),
              child: Text(f.isEmpty ? 'assets (root)' : f),
            ),
        ],
      ),
    );
    if (chosen != null) b.move(ids, chosen);
  }

  /// A "Referenced by" row of the Asset panel: a scene or prefab opens at
  /// the entity the pointer names; a material is shown in the browser.
  void openReference(String documentPath, String? pointer) {
    final b = browser;
    if (b == null) return;
    final target = b.entries.where((e) => e.path == documentPath).firstOrNull;
    if (target == null) return;
    if (target.document == 'scene' || target.document == 'prefab') {
      final tab = openDocument(target.id);
      if (tab == null) return;
      workspace?.show(PanelRegistry.hierarchy);
      final id = RegExp(r'^/entities/([^/]+)').firstMatch(pointer ?? '')?.group(1);
      if (id != null) hierarchy?.revealAndSelect(id);
    } else {
      b.reveal(target.id);
      workspace?.show(PanelRegistry.project);
    }
  }

  /// Saves; a conflict (the file changed on disk) opens the conflict dialog
  /// instead — the editor never overwrites. True when saved.
  Future<bool> saveTab(OpenDocument tab) async {
    if (tab.save()) return true;
    if (tab.hasConflict) return resolveConflict(tab);
    if (tab.lastError != null) _showError(tab.lastError!);
    return false;
  }

  Future<void> saveAllTabs() async {
    final t = tabs;
    if (t == null) return;
    for (final tab in t.saveAll()) {
      if (!await saveTab(tab)) break;
    }
  }

  Future<bool> saveTabAs(OpenDocument tab) async {
    final path = await showSaveAsDialog(context, tab);
    if (path == null) return false;
    // A new sub-folder is fine: the session writes files, not directories.
    try {
      Directory(p.dirname(path)).createSync(recursive: true);
    } on FileSystemException catch (e) {
      _showError('Cannot create ${p.dirname(path)}: ${e.osError?.message ?? e.message}');
      return false;
    }
    if (tab.saveAs(path)) return true;
    if (tab.lastError != null) _showError(tab.lastError!);
    return false;
  }

  Future<void> revertTab(OpenDocument tab) async {
    if (!tab.dirty) return;
    if (!await showRevertPrompt(context, tab)) return;
    if (!tab.reload() && tab.lastError != null) _showError(tab.lastError!);
  }

  /// Closes with the save prompt when dirty. False when the user cancelled.
  Future<bool> closeTab(OpenDocument tab) async {
    final t = tabs;
    if (t == null || !mounted) return false;
    if (tab.dirty) {
      final choice = await showClosePrompt(context, tab);
      switch (choice) {
        case ClosePromptChoice.cancel:
          return false;
        case ClosePromptChoice.save:
          if (!await saveTab(tab)) return false;
        case ClosePromptChoice.discard:
          break;
      }
    }
    if (!mounted) return false;
    if (!t.open.contains(tab)) return true;
    t.closeDiscarding(tab);
    registry.refresh();
    return true;
  }

  /// The conflict dialog and its outcome. True when the tab is clean afterwards.
  Future<bool> resolveConflict(OpenDocument tab) async {
    switch (await showConflictDialog(context, tab)) {
      case ConflictChoice.reload:
        if (!tab.reload() && tab.lastError != null) _showError(tab.lastError!);
        return !tab.dirty;
      case ConflictChoice.saveAs:
        return saveTabAs(tab);
      case ConflictChoice.keep:
        tab.keepEditing();
        return false;
    }
  }

  Future<void> createWithDialog() async {
    final h = hierarchy;
    if (h == null || !h.canEdit) return;
    final types = h.nativeComponentTypes.where((c) => c.type != 'fynkel.Transform').toList();
    final type = await showDialog<String>(
      context: context,
      builder: (context) => SimpleDialog(
        key: const Key('dialog.createWith'),
        title: const Text('Create Entity with…'),
        children: [
          for (final c in types)
            SimpleDialogOption(
              key: Key('createWith.${c.type}'),
              onPressed: () => Navigator.pop(context, c.type),
              child: Text(c.type),
            ),
        ],
      ),
    );
    if (type != null) h.createWith(type);
  }

  void togglePanel(String id) {
    workspace?.toggle(id);
    registry.refresh();
  }

  void resetLayout() {
    workspace?.reset();
    setState(() => _layoutNotice = null);
    registry.refresh();
  }

  void showPalette() => showCommandPalette(context, registry);

  void showPreferences() => showPreferencesDialog(
    context,
    prefs: widget.prefs,
    onResetLayout: () {
      resetLayout();
      if (workspace == null) widget.layoutStore.clear();
    },
    onToolsChanged: _detect,
  );

  void showAbout() {
    showAboutDialog(
      context: context,
      applicationName: 'Fynkel Editor',
      applicationVersion: 'version $editorVersion · authoring ABI v$authoringAbiVersion',
      applicationLegalese:
          'The editor never runs game code; a game\'s runtime carries no editor code (ADR-0016, ADR-0018).',
    );
  }

  void _showError(String message) {
    if (!mounted) return;
    setState(() => _openError = message);
    ScaffoldMessenger.maybeOf(context)
        ?.showSnackBar(SnackBar(content: Text(message), duration: const Duration(seconds: 8)));
  }

  // ── Build ───────────────────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    final p0 = project;
    final Widget body;
    if (p0 == null || workspace == null) {
      body = LauncherScreen(
        prefs: widget.prefs,
        toolchain: toolchain,
        chooseDirectory: widget.chooseDirectory,
        onOpen: openProjectDir,
        onRefreshChecks: _detect,
        onPreferences: showPreferences,
        run: widget.run,
        version: editorVersion,
      );
    } else {
      body = MainWindow(
        project: p0,
        workspace: workspace!,
        tabBar: DocumentTabBar(
          tabs: tabs!,
          onOpen: openDocumentDialog,
          onClose: closeTab,
          onSaveAs: saveTabAs,
          onShowConflict: resolveConflict,
          onRevert: revertTab,
        ),
        onShowConsole: () => workspace!.show(PanelRegistry.console),
        onClose: closeProject,
        onRevalidate: p0.revalidate,
        onMigrationPreview: () => showMigrationPreview(context, p0),
        layoutNotice: _layoutNotice,
        onDismissNotice: () => setState(() => _layoutNotice = null),
      );
    }
    return ListenableBuilder(
      listenable: registry,
      builder: (context, _) => Shortcuts(
        shortcuts: registry.shortcutIntents,
        child: Actions(
          actions: {CommandIntent: CommandAction(registry)},
          child: FocusScope(
            autofocus: true,
            child: EditorMenuBar(
              registry: registry,
              native: widget.nativeMenuBar,
              child: Stack(
                children: [
                  Positioned.fill(child: body),
                  if (_openError != null && p0 == null)
                    Positioned(
                      left: 0,
                      right: 0,
                      bottom: 0,
                      child: MaterialBanner(
                        key: const Key('shell.error'),
                        content: Text(_openError!),
                        actions: [
                          TextButton(onPressed: () => setState(() => _openError = null), child: const Text('Dismiss')),
                        ],
                      ),
                    ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
