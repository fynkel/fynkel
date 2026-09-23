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
import 'picker/picker.dart';
import 'play/code_reload.dart';
import 'play/editor_game.dart';
import 'play/game_console.dart';
import 'play/game_record.dart';
import 'play/play_prompt.dart';
import 'play/play_session.dart';
import 'prefs/preferences.dart';
import 'prefs/preferences_dialog.dart';
import 'project/editor_project.dart';
import 'project/migration_dialog.dart';
import 'project_browser/project_browser_model.dart';
import 'project_browser/project_panel.dart';
import 'scene_view/editor_state.dart';
import 'scene_view/scene_panel.dart';
import 'scene_view/scene_view_model.dart';
import 'shell/editor_selection.dart';
import 'shell/main_window.dart';
import 'shell/menu_bar.dart';
import 'shell/notices.dart';
import 'shell/panels.dart';
import 'shell/workspace.dart';
import 'theme/focus_ring.dart';
import 'theme/icons.dart';
import 'theme/theme.dart';

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
    this.startPlayProcess = spawnPlayProcess,
    this.nativeMenuBar,
    this.quit,
    this.initialProject,
    this.sceneViewport = true,
    this.game,
    this.codeReloadStatus,
  });

  final Preferences prefs;
  final LayoutStore layoutStore;
  final OpenProject openProject;
  final ChooseDirectory chooseDirectory;
  final ChooseSavePath chooseSavePath;

  /// The project's game when this is its project editor (ADR-0021, built
  /// by `fynkel editor <dir>`); null in the plain editor.
  final EditorGame? game;

  /// Where `fynkel editor` reports hot reloads; null: `FYNKEL_EDITOR_STATUS`.
  final String? codeReloadStatus;

  /// False leaves the native texture out of the Scene view (widget tests
  /// without a Metal device drive the model through the painted layer).
  final bool sceneViewport;
  final Future<Toolchain> Function(Preferences prefs)? detectToolchain;
  final RunProcess run;

  /// Starts the game process for Play (`fynkel run`); a test injects a
  /// fake so no window ever opens.
  final StartPlayProcess startPlayProcess;
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
        return MaterialApp(
          title: 'Fynkel Editor',
          debugShowCheckedModeBanner: false,
          // A theme or density change lands at once, like everything else.
          themeAnimationDuration: Duration.zero,
          builder: (context, child) => FocusRingLayer(child: child ?? const SizedBox.shrink()),
          theme: fynkelTheme(brightness: Brightness.light, density: prefs.density),
          darkTheme: fynkelTheme(brightness: Brightness.dark, density: prefs.density),
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
            startPlayProcess: startPlayProcess,
            sceneViewport: sceneViewport,
            nativeMenuBar: nativeMenuBar,
            quit: quit ?? () => exit(0),
            initialProject: initialProject,
            game: game,
            codeReloadStatus: codeReloadStatus ?? CodeReloadStatus.environmentPath,
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
    this.startPlayProcess = spawnPlayProcess,
    this.nativeMenuBar,
    this.initialProject,
    this.sceneViewport = true,
    this.game,
    this.codeReloadStatus,
  });

  final Preferences prefs;
  final LayoutStore layoutStore;
  final OpenProject openProject;
  final ChooseDirectory chooseDirectory;
  final ChooseSavePath chooseSavePath;
  final EditorGame? game;
  final String? codeReloadStatus;
  final bool sceneViewport;
  final Future<Toolchain> Function(Preferences prefs) detectToolchain;
  final RunProcess run;
  final StartPlayProcess startPlayProcess;
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
  PlaySession? play;
  GameConsoleModel? gameConsole;
  DocumentTabs? tabs;
  CodegenCheck? codegen;
  EditorState? editorState;

  // Item 9e: the Hierarchy, the Inspector, the Project browser and the
  // Scene view can be open more than once. Each copy has its own model, by
  // the copy's id (`inspector`, `inspector#2`, …); a first copy's model
  // lives as long as the project, another copy's as long as its panel.
  final _hierarchies = <String, HierarchyModel>{};
  final _inspectors = <String, InspectorModel>{};

  /// What each Inspector copy shows (item 9c): entities, an asset, a part, a folder.
  final _selections = <String, EditorSelection>{};
  final _browsers = <String, ProjectBrowserModel>{};
  final _scenes = <String, SceneViewModel>{};

  /// The padlocks the tabs last drew, so a tab redraws only when one moved.
  final _locksSeen = <String, bool>{};

  /// The Hierarchy operations the Scene views run (drop a prefab, Delete):
  /// always on the focused tab, whatever a Hierarchy copy is locked to.
  HierarchyModel? _sceneOps;
  SceneViews? _sceneViews;

  // The copy commands address: the one that had the keyboard last.
  HierarchyModel? get hierarchy => _active(_hierarchies, PanelRegistry.hierarchy);
  InspectorModel? get inspector => _active(_inspectors, PanelRegistry.inspector);
  EditorSelection? get selection => _active(_selections, PanelRegistry.inspector);
  ProjectBrowserModel? get browser => _active(_browsers, PanelRegistry.project);
  SceneViewModel? get scene => _active(_scenes, PanelRegistry.scene);

  // A copy's models by its id (tests).
  @visibleForTesting
  HierarchyModel? hierarchyOf(String id) => _hierarchies[id];
  @visibleForTesting
  EditorSelection? selectionOf(String id) => _selections[id];
  @visibleForTesting
  ProjectBrowserModel? browserOf(String id) => _browsers[id];
  @visibleForTesting
  SceneViewModel? sceneOf(String id) => _scenes[id];

  T? _active<T>(Map<String, T> models, String base) => models[workspace?.activeCopy(base) ?? base] ?? models[base];
  final consoleSearchFocus = FocusNode(debugLabel: 'console.search');
  final gameSearchFocus = FocusNode(debugLabel: 'console.game.search');

  /// Which Console tab is in front — the shell moves it (Play shows the
  /// Game tab, a refused Play the Project tab) and the panel follows.
  final consoleTab = ValueNotifier<int>(ConsolePanel.projectTab);
  final hierarchySearchFocus = FocusNode(debugLabel: 'hierarchy.search');
  final projectSearchFocus = FocusNode(debugLabel: 'project.search');
  late IdeLauncher ide;
  String? _layoutNotice;

  /// The status bar's message line (the launcher's strip before a project
  /// is open): what a Material app would show as a snackbar.
  final notices = Notices();
  bool _opening = false;

  /// `fynkel editor`'s hot reloads (ADR-0021): each outcome is a notice.
  CodeReloadStatus? codeReload;

  void _onCodeReload() {
    final r = codeReload?.last;
    if (r == null) return;
    final files = r.files.isEmpty ? '' : ' (${r.files.join(', ')})';
    switch (r.state) {
      case 'reloaded':
        notices.show('Code reloaded in ${r.ms ?? '?'} ms$files', severity: NoticeSeverity.info);
      case 'reloadFailed':
        notices.show('Code did not reload$files: ${r.message ?? 'no reason given'}');
    }
  }

  @override
  void initState() {
    super.initState();
    ide = IdeLauncher(widget.prefs);
    final status = widget.codeReloadStatus;
    if (status != null) codeReload = CodeReloadStatus(status)..addListener(_onCodeReload);
    _panels = PanelRegistry(_panelSpecs());
    _registerCommands();
    widget.prefs.addListener(_onPrefsChanged);
    final detected = _detect();
    // A project named at launch opens once the tools are known: its open runs
    // the codegen check, which needs the CLI the detection is still finding.
    final initial = widget.initialProject;
    if (initial != null) {
      detected.then((_) {
        if (mounted) openProjectDir(initial);
      });
    }
  }

  @override
  void dispose() {
    widget.prefs.removeListener(_onPrefsChanged);
    codeReload?.dispose();
    workspace?.dispose();
    _disposeCopies();
    editorState?.dispose();
    codegen?.dispose();
    tabs?.dispose();
    project?.removeListener(_onProjectChanged);
    project?.close();
    gameConsole?.dispose();
    play?.dispose();
    consoleTab.dispose();
    consoleSearchFocus.dispose();
    gameSearchFocus.dispose();
    hierarchySearchFocus.dispose();
    projectSearchFocus.dispose();
    registry.dispose();
    notices.dispose();
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
      icon: FIcons.accountTreeOutlined,
      home: PanelHome.left,
      copies: true,
      locked: _isLocked,
      builder: (_, id) {
        final h = _hierarchies[id];
        if (h == null) return const SizedBox.shrink();
        return HierarchyPanel(
          model: h,
          onOpenDocument: openDocumentDialog,
          searchFocus: PanelIds.number(id) == 1 ? hierarchySearchFocus : null,
          commands: registry,
        );
      },
    ),
    PanelSpec(
      id: PanelRegistry.scene,
      title: 'Scene',
      icon: FIcons.viewInArOutlined,
      fullTitle: 'Scene View',
      home: PanelHome.center,
      copies: true,
      builder: (_, id) {
        final s = _scenes[id];
        if (s == null) return const SizedBox.shrink();
        return ScenePanel(model: s, showViewport: widget.sceneViewport);
      },
    ),
    PanelSpec(
      id: PanelRegistry.inspector,
      title: 'Inspector',
      icon: FIcons.tune,
      home: PanelHome.right,
      copies: true,
      locked: _isLocked,
      builder: (_, id) {
        final i = _inspectors[id];
        if (i == null) return const SizedBox.shrink();
        return InspectorPanel(
          model: i,
          selection: _selections[id],
          browser: _browsers[PanelRegistry.project],
          onOpenDocument: openDocument,
          onOpenReference: openReference,
          run: widget.run,
        );
      },
    ),
    PanelSpec(
      id: PanelRegistry.project,
      title: 'Project',
      icon: FIcons.folderOutlined,
      fullTitle: 'Project Browser',
      home: PanelHome.bottom,
      copies: true,
      locked: _isLocked,
      builder: (_, id) {
        final b = _browsers[id];
        if (b == null) return const SizedBox.shrink();
        return ProjectPanel(
          model: b,
          onOpenDocument: openDocument,
          onShowAsset: showAssetInInspector,
          run: widget.run,
          searchFocus: PanelIds.number(id) == 1 ? projectSearchFocus : null,
        );
      },
    ),
    PanelSpec(
      id: PanelRegistry.console,
      title: 'Console',
      icon: FIcons.terminal,
      home: PanelHome.bottom,
      builder: (_, _) {
        final c = console;
        if (c == null) return const SizedBox.shrink();
        return ConsolePanel(
          key: const Key('console.panel'),
          model: c,
          tab: consoleTab,
          searchFocus: consoleSearchFocus,
          game: gameConsole,
          gameSearchFocus: gameSearchFocus,
          onPlay: playProject,
          onStop: stopPlaying,
          onMigrationPreview: () => showMigrationPreview(context, project!),
        );
      },
    ),
    PanelSpec(
      id: PanelRegistry.profiler,
      title: 'Profiler',
      icon: FIcons.speed,
      home: PanelHome.bottom,
      builder: (_, _) =>
          const ComingPanel(title: 'Profiler', item: 9, what: 'Frame time, ticks, bridge bytes.', icon: FIcons.speed),
    ),
    PanelSpec(
      id: PanelRegistry.animation,
      title: 'Animation',
      icon: FIcons.animation,
      home: PanelHome.bottom,
      builder: (_, _) => const ComingPanel(
        title: 'Animation',
        item: 6,
        what: 'Clips and the pose of the selection.',
        icon: FIcons.animation,
      ),
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
    // A command's bare key: from any panel but a text field (a held key does not repeat it).
    SingleActivator bare(LogicalKeyboardKey key, {bool shift = false}) =>
        SingleActivator(key, shift: shift, includeRepeats: false);
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
        description: 'The selected scene or prefab into a tab; any other asset into the Inspector',
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
            showAssetInInspector();
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
          showAssetInInspector();
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
      for (final (i, tool) in SceneTools.all.indexed)
        EditorCommand(
          id: 'scene.tool.${tool.id}',
          baseLabel: '${tool.label} Tool',
          description:
              'The Scene view\'s ${tool.label.toLowerCase()} tool'
              '${tool.key == null ? '' : ' (also ${tool.key!.keyLabel} from any panel)'}',
          menu: EditorMenu.edit,
          // ⌘1…⌘4, then ⌘6… (⌘5 is the projection); past ⌘9, the key in
          // the view and the palette only.
          shortcut: switch (i < 4 ? i + 1 : i + 2) {
            final n when n <= 9 => k(LogicalKeyboardKey(LogicalKeyboardKey.digit0.keyId + n)),
            _ => null,
          },
          key: tool.key == null ? null : bare(tool.key!),
          group: 2,
          order: i,
          enabled: () => _hasView,
          run: () => scene?.tool = tool,
        ),
      EditorCommand(
        id: 'scene.toggleSpace',
        baseLabel: 'Toggle Local / World Axes',
        labelBuilder: () => scene?.space == GizmoSpace.local ? 'Use World Axes' : 'Use Local Axes',
        description: 'The gizmo\'s axes: the entity\'s own or the world\'s',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyL),
        key: bare(LogicalKeyboardKey.keyX),
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
        description: 'Editor-only: out of the picture and of picking, never in the scene file',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyH, shift: true),
        key: bare(LogicalKeyboardKey.keyH),
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
        key: bare(LogicalKeyboardKey.keyH, shift: true),
        group: 3,
        order: 1,
        enabled: () => scene?.hasHidden ?? false,
        run: () => scene?.showAll(),
      ),
      EditorCommand(
        id: 'scene.toggleLocked',
        baseLabel: 'Lock / Unlock Selected',
        description: 'Editor-only: a locked entity is not picked or dragged in the Scene view',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyK),
        key: bare(LogicalKeyboardKey.keyL),
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
        description: 'The camera swings to fit the selection (everything when nothing is selected)',
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.keyF),
        key: bare(LogicalKeyboardKey.keyF),
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
        key: bare(LogicalKeyboardKey.keyF, shift: true),
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
        key: bare(LogicalKeyboardKey.digit5),
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
      // ── Play (Play menu) ──
      EditorCommand(
        id: 'play.start',
        baseLabel: 'Play',
        labelBuilder: () => play?.isActive ?? false ? 'Playing…' : 'Play',
        description: 'Run the project as its own app (`fynkel run`) and show what it says in the Console',
        menu: EditorMenu.play,
        shortcut: k(LogicalKeyboardKey.keyP),
        group: 0,
        order: 0,
        enabled: () => _hasProject && !(play?.isActive ?? false),
        run: playProject,
      ),
      EditorCommand(
        id: 'play.stop',
        baseLabel: 'Stop',
        description: 'Kill the game and everything it started',
        menu: EditorMenu.play,
        shortcut: k(LogicalKeyboardKey.period),
        group: 0,
        order: 1,
        enabled: () => play?.isActive ?? false,
        run: stopPlaying,
      ),
      EditorCommand(
        id: 'play.gizmos',
        baseLabel: 'Show Gizmos in Play',
        labelBuilder: () => editorState?.playGizmos ?? false ? 'Hide Gizmos in Play' : 'Show Gizmos in Play',
        description:
            'The next Play draws the game\'s `Gizmos` calls in its window (`FYNKEL_GIZMOS=1`; debug and profile builds)',
        menu: EditorMenu.play,
        shortcut: k(LogicalKeyboardKey.keyG, shift: true, alt: true),
        group: 0,
        order: 2,
        enabled: () => _hasProject,
        run: togglePlayGizmos,
      ),
      EditorCommand(
        id: 'play.showConsole',
        baseLabel: 'Game Console',
        description: 'The Console\'s Game tab: the running game\'s logs, diagnostics and state',
        menu: EditorMenu.play,
        shortcut: k(LogicalKeyboardKey.keyJ, alt: true),
        group: 1,
        order: 0,
        enabled: () => _hasProject,
        run: showGameConsole,
      ),
      EditorCommand(
        id: 'play.clearConsole',
        baseLabel: 'Clear Game Console',
        menu: EditorMenu.play,
        shortcut: k(LogicalKeyboardKey.keyK, shift: true),
        group: 1,
        order: 1,
        enabled: () => (play?.records.isNotEmpty ?? false),
        run: () => play?.clear(),
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
          labelBuilder: () => '${workspace?.isShown(id) ?? true ? 'Hide' : 'Show'} ${_panels.byId(id)!.title}',
          menu: EditorMenu.window,
          shortcut: k(_digit(i + 1), alt: true),
          group: 2,
          order: i,
          enabled: () => _hasProject,
          run: () => togglePanel(id),
        ),
      for (final (i, id) in copyablePanels.indexed)
        EditorCommand(
          id: 'window.newCopy.$id',
          baseLabel: 'New ${_panels.byId(id)!.name}',
          description:
              'Another ${_panels.byId(id)!.name} beside the one used last, with its own ${switch (id) {
                PanelRegistry.inspector => 'selection and padlock',
                PanelRegistry.project => 'folder and padlock',
                PanelRegistry.hierarchy => 'padlock',
                _ => 'camera',
              }}',
          menu: EditorMenu.window,
          // ⇧ with the panel's own Show / Hide: ⇧⌥⌘1 a new Hierarchy.
          shortcut: k(_digit(PanelRegistry.defaultOrder.indexOf(id) + 1), alt: true, shift: true),
          group: 4,
          order: i,
          enabled: () => _hasProject,
          run: () => newPanelCopy(id),
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
      animationStyle: AnimationStyle.noAnimation,
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
      _disposeCopies();
      editorState?.dispose();
      codegen?.dispose();
      tabs?.dispose();
      final ws = Workspace(registry: _panels, store: widget.layoutStore);
      final t = DocumentTabs(project: opened);
      final h = HierarchyModel(tabs: t, project: opened);
      final c = ConsoleModel(project: opened, openInIde: ide.open, openInEditor: openDiagnosticInEditor);
      final ps = PlaySession(projectDir: opened.dir, cli: () => toolchain?.fynkelCli, start: widget.startPlayProcess);
      final gc = GameConsoleModel(session: ps, relative: opened.relative, openRecord: openGameRecord);
      final cg = CodegenCheck(
        projectDir: opened.dir,
        run: widget.run,
        cli: () => toolchain?.fynkelCli,
        repoRoot: () => toolchain?.fynkelRoot,
      );
      final es = EditorState.load(opened.dir);
      ps.gizmos = es.playGizmos;
      opened.addListener(_onProjectChanged);
      c.addListener(registry.refresh);
      ps.addListener(registry.refresh);
      t.addListener(_onTabsChanged);
      gameConsole?.dispose();
      play?.dispose();
      project = opened;
      tabs = t;
      codegen = cg;
      editorState = es;
      _sceneOps = h;
      _sceneViews = SceneViews();
      // The first copies, then any other the saved layout has.
      for (final base in _creationOrder) {
        _newCopyModel(base);
      }
      workspace = ws;
      _syncCopies();
      ws.addListener(_syncCopies);
      ws.focusedPanel.addListener(_onPanelFocused);
      setState(() {
        project = opened;
        workspace = ws;
        play = ps;
        gameConsole = gc;
        tabs = t;
        codegen = cg;
        console = c;
        editorState = es;
        _layoutNotice = ws.restoreProblem == null
            ? null
            : 'The saved layout was replaced by the default: ${ws.restoreProblem}.';
        if (es.loadProblem != null) {
          _layoutNotice =
              '${_layoutNotice ?? ''}${_layoutNotice == null ? '' : ' '}'
              '.fynkel/editor.json did not parse and starts fresh: ${es.loadProblem}.';
        }
      });
      notices.dismiss(); // a launcher error is about the project that did not open
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

  void _onProjectChanged() {
    _noteEditDuringPlay();
    registry.refresh();
  }

  void _onTabsChanged() {
    _noteEditDuringPlay();
    registry.refresh();
  }

  /// What the documents on screen look like: the project's asset revision
  /// and, per open tab that was edited at all, its history depth and dirty
  /// flag. Play remembers it; anything else means the game is running older
  /// bytes. Merely opening (or closing, or reordering) a tab is not an
  /// edit, so an untouched tab is left out and the list is sorted.
  String _editSignature() {
    final parts = <String>[];
    for (final tab in tabs?.open ?? const <OpenDocument>[]) {
      if (tab.document.history == 0 && !tab.dirty) continue;
      parts.add('${tab.path}:${tab.document.history}:${tab.dirty}');
    }
    parts.sort();
    return '${project?.assetsRevision ?? 0}|${parts.join('|')}';
  }

  String? _playEditBaseline;

  void _noteEditDuringPlay() {
    final session = play;
    if (session == null || !session.isActive || _playEditBaseline == null) return;
    if (_editSignature() != _playEditBaseline) session.markEdited();
  }

  // ── Play ────────────────────────────────────────────────────────────────

  /// Runs the project in its own process (ADR-0016). A project with errors
  /// never runs — `fynkel run` would refuse it anyway, and the Console is
  /// where the reasons are. Dirty documents get the save prompt first: the
  /// game reads files, not the editor's memory.
  Future<void> playProject() async {
    final opened = project;
    final session = play;
    if (opened == null || session == null || session.isActive) return;
    if (opened.errorCount > 0) {
      showGameConsole(project: true);
      _showError(
        'The project has ${opened.errorCount} error${opened.errorCount == 1 ? '' : 's'}; `fynkel run` refuses to '
        'run it. The Console\'s Project tab lists them — double-click a row to go to it.',
      );
      return;
    }
    final dirty = List.of(tabs?.dirtyDocuments ?? const <OpenDocument>[]);
    if (dirty.isNotEmpty) {
      final choice = await showPlaySavePrompt(context, dirty);
      if (!mounted) return;
      switch (choice) {
        case PlaySaveChoice.cancel:
          return;
        case PlaySaveChoice.save:
          for (final tab in dirty) {
            if (!await saveTab(tab)) return;
          }
        case PlaySaveChoice.playAnyway:
          break;
      }
      if (!mounted) return;
    }
    showGameConsole();
    _playEditBaseline = _editSignature();
    await session.play();
    if (!mounted) return;
    _noteEditDuringPlay();
    registry.refresh();
  }

  /// Play again while a run is up (the edited-during-play banner): the
  /// running game is stopped first, then the project starts fresh from the
  /// files on disk. [playProject] alone refuses while a run is active.
  Future<void> replayProject() async {
    final session = play;
    if (session == null) return;
    if (session.isActive) {
      if (session.state == PlayState.stopping) return;
      await stopPlaying();
      if (!mounted) return;
    }
    await playProject();
  }

  /// Play with or without the game's gizmos (remembered in editor.json).
  void togglePlayGizmos() {
    final es = editorState, ps = play;
    if (es == null || ps == null) return;
    es.playGizmos = !es.playGizmos;
    ps.gizmos = es.playGizmos;
    es.changed();
    registry.refresh();
  }

  Future<void> stopPlaying() async {
    final session = play;
    if (session == null || !session.isActive) return;
    await session.stop();
    if (mounted) registry.refresh();
  }

  /// Shows the Console with the Game tab in front (or the Project tab).
  void showGameConsole({bool project = false}) {
    workspace?.show(PanelRegistry.console);
    consoleTab.value = project ? ConsolePanel.projectTab : ConsolePanel.gameTab;
  }

  /// Where a record's document is on disk, or null when it is nowhere.
  ///
  /// A game names a document the way the **cooked** project does — from
  /// the assets root, `scenes/menu.scene.json` — while the editor works
  /// in project paths (`assets/scenes/menu.scene.json`). Both are tried,
  /// the game's spelling first; an absolute path (a game started from the
  /// source tree) is taken as it is.
  String? _recordDocument(EditorProject opened, String document) {
    final parts = p.joinAll(document.split('/'));
    for (final candidate in [
      if (p.isAbsolute(document)) document else ...[p.join(opened.assetsDir, parts), p.join(opened.dir, parts)],
    ]) {
      if (File(candidate).existsSync()) return candidate;
    }
    return null;
  }

  /// A Game row's target: the document it names (with the entity selected),
  /// or the asset it names in the Project panel.
  void openGameRecord(GameRecord record) {
    final opened = project;
    if (opened == null) return;
    final document = record.document;
    if (document != null) {
      final path = _recordDocument(opened, document);
      if (path != null) {
        final tab = openDocument(path);
        if (tab != null) {
          _revealEntity(tab, record.id ?? RegExp(r'^/entities/([^/]+)').firstMatch(record.pointer ?? '')?.group(1));
          return;
        }
      }
    }
    final id = record.id;
    if (id != null && opened.assetExists(id)) revealInProject(id);
  }

  /// Closes the project; every dirty tab gets its save prompt first, and a
  /// Cancel there keeps the project open.
  Future<void> closeProject() async {
    final closing = project;
    if (closing == null) return;
    await stopPlaying();
    for (final tab in List.of(tabs?.dirtyDocuments ?? const <OpenDocument>[])) {
      if (!await closeTab(tab)) return;
    }
    workspace?.save();
    closing.removeListener(_onProjectChanged);
    final t = tabs;
    final cg = codegen;
    final es = editorState;
    final ws = workspace;
    ws?.removeListener(_syncCopies);
    setState(() {
      project = null;
      console = null;
      codegen = null;
      tabs = null;
      editorState = null;
      workspace = null;
    });
    ws?.dispose();
    _disposeCopies();
    es?.dispose();
    cg?.dispose();
    t?.dispose();
    await closing.close();
    registry.refresh();
  }

  // ── Documents ───────────────────────────────────────────────────────────

  Future<void> openDocumentDialog({Rect? anchor}) async {
    final pr = project;
    if (pr == null) return;
    final asset = await showOpenDocumentDialog(context, pr, anchor: anchor);
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
    _revealEntity(tab, d.id ?? RegExp(r'^/entities/([^/]+)').firstMatch(d.pointer ?? '')?.group(1));
  }

  // ── Assets ──────────────────────────────────────────────────────────────

  /// The browser's lead asset in the Inspector (Open Asset on a
  /// non-document, Show in Inspector, Reimport), the Inspector brought
  /// forward: the copy used last that is not locked — a new one when every
  /// copy is (item 9e: a lock is never broken from outside the panel).
  void showAssetInInspector() {
    final ws = workspace;
    if (ws == null) return;
    final lead = browser?.lead;
    final id = _unlockedCopy(PanelRegistry.inspector);
    if (lead != null) _selections[id]?.inspect(InspectAsset(lead));
    ws.show(id);
    registry.refresh();
  }

  /// "Show in Project" — a Game row, a "Used by" row, an asset slot's arrow:
  /// the asset revealed in the Project browser copy used last that is not
  /// locked; a new copy opens when every copy is locked.
  void revealInProject(String assetId) {
    final ws = workspace;
    if (ws == null) return;
    final id = _unlockedCopy(PanelRegistry.project);
    ws.show(id);
    _browsers[id]?.reveal(assetId);
  }

  /// The copy of [base] an explicit request goes to: the one used last that
  /// is not locked, shown if hidden; else a new copy.
  String _unlockedCopy(String base) {
    final ws = workspace!;
    final free = ws.copiesByRecency(base).where((id) => !_isLocked(id)).firstOrNull;
    if (free != null) return free;
    if (!ws.isShown(base) && !_isLocked(base)) return base;
    if (!ws.isShown(base)) ws.show(base);
    return ws.openCopy(base) ?? base;
  }

  /// [tab]'s entity [id] selected and scrolled to in a Hierarchy showing
  /// [tab]: the copy used last when it does, else another that does (an
  /// unlocked copy follows the focused tab); a new copy opens when every
  /// copy is locked to another document.
  void _revealEntity(OpenDocument tab, String? id) {
    final ws = workspace;
    if (ws == null) return;
    const base = PanelRegistry.hierarchy;
    var target = ws.copiesByRecency(base).where((c) => _hierarchies[c]?.tab == tab).firstOrNull;
    target ??= (!ws.isShown(base) && _hierarchies[base]?.tab == tab) ? base : null;
    if (target == null) {
      // Every copy is locked to another document.
      if (!ws.isShown(base)) ws.show(base);
      target = ws.openCopy(base) ?? base;
    }
    ws.show(target);
    if (id != null) _hierarchies[target]?.revealAndSelect(id);
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
      animationStyle: AnimationStyle.noAnimation,
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

  /// A "Used by" row of an asset's Inspector page: a scene or prefab opens at
  /// the entity the pointer names; a material is shown in the browser.
  void openReference(String documentPath, String? pointer) {
    final b = browser;
    if (b == null) return;
    final target = b.entries.where((e) => e.path == documentPath).firstOrNull;
    if (target == null) return;
    if (target.document == 'scene' || target.document == 'prefab') {
      final tab = openDocument(target.id);
      if (tab == null) return;
      _revealEntity(tab, RegExp(r'^/entities/([^/]+)').firstMatch(pointer ?? '')?.group(1));
    } else {
      revealInProject(target.id);
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

  /// Create Entity with Component, through the shared picker (item 9d): the
  /// authorable native components, grouped like Add Component; one without
  /// defaults is shown and cannot be chosen.
  Future<void> createWithDialog() async {
    final h = hierarchy;
    final pr = project;
    if (h == null || pr == null || !h.canEdit) return;
    final types = h.nativeComponentTypes.where((c) => c.type != 'fynkel.Transform').toList();
    final choice = await showPicker<String>(
      context,
      title: 'Create Entity with Component',
      hint: 'Search components',
      keyPrefix: 'createWith',
      countNoun: 'components',
      items: [
        for (final c in types)
          if (inspector?.specFor(c.type) case final spec?)
            PickerItem(
              value: c.type,
              id: c.type,
              title: spec.title,
              group: spec.category ?? 'Engine',
              icon: componentIcon(spec),
              enabled: pr.componentDefaults(c.type) != null,
              note: pr.componentDefaults(c.type) == null ? 'no defaults' : null,
              keywords: c.type,
              facts: [('Type', c.type)],
              description: spec.description,
            ),
      ],
    );
    if (choice == null || choice.isNone) return;
    h.createWith(choice.value);
  }

  void togglePanel(String id) {
    workspace?.toggle(id);
    registry.refresh();
  }

  // ── Panel copies (item 9e) ──────────────────────────────────────────────

  /// The panels that can be open more than once, in the Window menu's order.
  static const copyablePanels = [
    PanelRegistry.hierarchy,
    PanelRegistry.scene,
    PanelRegistry.inspector,
    PanelRegistry.project,
  ];

  /// The order copies' models are made in: the Project browsers before the
  /// Inspectors (an Inspector follows every browser and recalls its lock
  /// through one).
  static const _creationOrder = [
    PanelRegistry.hierarchy,
    PanelRegistry.project,
    PanelRegistry.inspector,
    PanelRegistry.scene,
  ];

  /// Window ▸ New Inspector (and the tab menu's): another copy beside the
  /// one used last.
  void newPanelCopy(String base) {
    workspace?.openCopy(base);
    registry.refresh();
  }

  bool _isLocked(String id) => switch (PanelIds.base(id)) {
    PanelRegistry.inspector => _selections[id]?.locked ?? false,
    PanelRegistry.project => _browsers[id]?.locked ?? false,
    PanelRegistry.hierarchy => _hierarchies[id]?.locked ?? false,
    _ => false,
  };

  /// The models of copy [id] of a copyable panel, wired like the first's.
  void _newCopyModel(String id) {
    final opened = project!;
    final t = tabs!;
    final es = editorState!;
    switch (PanelIds.base(id)) {
      case PanelRegistry.hierarchy:
        final h = HierarchyModel(tabs: t, project: opened);
        h.addListener(() => _onCopyChanged(id));
        _hierarchies[id] = h;
      case PanelRegistry.inspector:
        final ins = InspectorModel(tabs: t, project: opened, codegen: codegen!, openInIde: ide.open);
        ins.revealAsset = (asset, sub) => revealInProject(asset);
        // A preview depends on the asset only; any browser draws it.
        ins.assetPreview = (asset) => _browsers.values.first.preview(asset);
        final sel = EditorSelection(inspector: ins);
        for (final b in _browsers.values) {
          sel.addBrowser(b);
        }
        sel.recall(es.panel(id));
        ins.addListener(registry.refresh);
        sel.addListener(() => _onCopyChanged(id));
        _inspectors[id] = ins;
        _selections[id] = sel;
      case PanelRegistry.project:
        final b = ProjectBrowserModel(project: opened);
        b.recall(es.panel(id));
        for (final sel in _selections.values) {
          sel.addBrowser(b);
        }
        b.addListener(() => _onCopyChanged(id));
        _browsers[id] = b;
      case PanelRegistry.scene:
        final sv = SceneViewModel(
          tabs: t,
          project: opened,
          hierarchy: _sceneOps!,
          editorState: es,
          chooseSavePath: widget.chooseSavePath,
          view: id,
          views: _sceneViews,
          zoomToPointer: () => widget.prefs.zoomToPointer,
        );
        sv.addListener(registry.refresh);
        _scenes[id] = sv;
    }
  }

  /// A copy's model changed: commands re-check, its tab redraws when its
  /// padlock moved, and what it remembers is written (an Inspector's lock,
  /// a Project browser's folder).
  void _onCopyChanged(String id) {
    registry.refresh();
    final locked = _isLocked(id);
    if (_locksSeen[id] != locked) {
      _locksSeen[id] = locked;
      workspace?.refreshTabs();
    }
    final es = editorState;
    if (es == null) return;
    switch (PanelIds.base(id)) {
      case PanelRegistry.inspector:
        es.setPanel(id, _selections[id]?.remembered);
      case PanelRegistry.project:
        es.setPanel(id, _browsers[id]?.remembered);
    }
  }

  /// The layout changed: a copy that appeared gets its models, a copy that
  /// closed loses them and what it remembered (its cameras too). First
  /// copies keep theirs while the project is open.
  void _syncCopies() {
    final ws = workspace;
    final es = editorState;
    if (ws == null || es == null || project == null) return;
    for (final base in _creationOrder) {
      final shown = ws.copiesOf(base).toSet();
      for (final id in shown) {
        if (PanelIds.number(id) > 1 && !_hasModel(id)) _newCopyModel(id);
      }
      for (final id in [..._copyIds(base)]) {
        if (PanelIds.number(id) > 1 && !shown.contains(id)) {
          _disposeCopy(id);
          es.setPanel(id, null);
          if (base == PanelRegistry.scene) es.forgetView(id);
        }
      }
    }
  }

  bool _hasModel(String id) =>
      _hierarchies.containsKey(id) ||
      _inspectors.containsKey(id) ||
      _browsers.containsKey(id) ||
      _scenes.containsKey(id);

  Iterable<String> _copyIds(String base) => switch (base) {
    PanelRegistry.hierarchy => _hierarchies.keys,
    PanelRegistry.inspector => _inspectors.keys,
    PanelRegistry.project => _browsers.keys,
    _ => _scenes.keys,
  };

  /// Drops copy [id]'s models after the frame that unmounts its panel.
  void _disposeCopy(String id) {
    final h = _hierarchies.remove(id);
    final ins = _inspectors.remove(id);
    final sel = _selections.remove(id);
    final b = _browsers.remove(id);
    final sv = _scenes.remove(id);
    _locksSeen.remove(id);
    if (b != null) {
      for (final other in _selections.values) {
        other.removeBrowser(b);
      }
    }
    WidgetsBinding.instance.addPostFrameCallback((_) {
      sv?.dispose();
      sel?.dispose();
      ins?.dispose();
      b?.dispose();
      h?.dispose();
    });
  }

  /// Every copy's models, the Scene views first (they use the Hierarchy
  /// operations) — the project is closing.
  void _disposeCopies() {
    for (final sv in _scenes.values) {
      sv.dispose();
    }
    for (final sel in _selections.values) {
      sel.dispose();
    }
    for (final m in [..._inspectors.values, ..._browsers.values, ..._hierarchies.values]) {
      m.dispose();
    }
    _scenes.clear();
    _selections.clear();
    _inspectors.clear();
    _browsers.clear();
    _hierarchies.clear();
    _locksSeen.clear();
    _sceneOps?.dispose();
    _sceneOps = null;
    _sceneViews = null;
  }

  /// A panel copy took the keyboard. Every unlocked Inspector shows that
  /// panel's selection again (a click back in the Hierarchy after browsing
  /// assets); a Hierarchy locked to a document in the background brings
  /// that document forward — what you work on is what the Scene view and
  /// the Inspector show.
  void _onPanelFocused() {
    final id = workspace?.focusedPanel.value;
    if (id == null) return;
    final base = PanelIds.base(id);
    final h = _hierarchies[id];
    final t = tabs;
    if (h != null && h.locked && t != null && h.tab != null && h.tab != t.focused) t.focus(h.tab);
    for (final sel in _selections.values) {
      sel.panelFocused(base, from: _browsers[id]);
    }
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
    showDialog<void>(
      context: context,
      animationStyle: AnimationStyle.noAnimation,
      builder: (_) => const AboutDialog(
        applicationName: 'Fynkel Editor',
        applicationVersion: 'version $editorVersion · authoring ABI v$authoringAbiVersion',
        applicationLegalese:
            'The editor never runs game code; a game\'s runtime carries no editor code (ADR-0016, ADR-0018).',
      ),
    );
  }

  void _showError(String message) {
    if (!mounted) return;
    notices.show(message);
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
          onOpen: (anchor) => openDocumentDialog(anchor: anchor),
          onClose: closeTab,
          onSaveAs: saveTabAs,
          onShowConflict: resolveConflict,
          onRevert: revertTab,
        ),
        onShowConsole: showGameConsole,
        play: play,
        onPlay: playProject,
        onReplay: replayProject,
        onStop: stopPlaying,
        onClose: closeProject,
        onRevalidate: p0.revalidate,
        onReveal: () => registry.execute('project.reveal'),
        onCommandPalette: showPalette,
        recents: widget.prefs.recents,
        onOpenProject: openProjectDialog,
        onOpenRecent: openProjectDir,
        codegen: codegen,
        codegenDrift: () {
          final i = inspector;
          return i != null && i.hasMetadata ? i.committedDrift.length : 0;
        },
        onCheckCodegen: () => codegen?.check(),
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
              child: NoticesScope(
                notices: notices,
                child: p0 == null || workspace == null
                    ? Column(
                        children: [
                          Expanded(child: body),
                          NoticeStrip(key: const Key('shell.error'), notices: notices),
                        ],
                      )
                    : body,
              ),
            ),
          ),
        ),
      ),
    );
  }
}
