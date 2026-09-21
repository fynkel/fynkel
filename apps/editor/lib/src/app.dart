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
import 'launcher/launcher.dart';
import 'prefs/preferences.dart';
import 'prefs/preferences_dialog.dart';
import 'project/editor_project.dart';
import 'project/migration_dialog.dart';
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

class FynkelEditorApp extends StatelessWidget {
  const FynkelEditorApp({
    super.key,
    required this.prefs,
    required this.layoutStore,
    this.openProject = _openSessionProject,
    this.chooseDirectory = pickDirectory,
    this.detectToolchain,
    this.run = runProcess,
    this.nativeMenuBar,
    this.quit,
    this.initialProject,
  });

  final Preferences prefs;
  final LayoutStore layoutStore;
  final OpenProject openProject;
  final ChooseDirectory chooseDirectory;
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
            detectToolchain: detectToolchain ?? (prefs) => Toolchain.detect(prefs: prefs, run: run),
            run: run,
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
    required this.detectToolchain,
    required this.run,
    required this.quit,
    this.nativeMenuBar,
    this.initialProject,
  });

  final Preferences prefs;
  final LayoutStore layoutStore;
  final OpenProject openProject;
  final ChooseDirectory chooseDirectory;
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
  final consoleSearchFocus = FocusNode(debugLabel: 'console.search');
  final hierarchySearchFocus = FocusNode(debugLabel: 'hierarchy.search');
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
    inspector?.dispose();
    codegen?.dispose();
    hierarchy?.dispose();
    tabs?.dispose();
    project?.removeListener(_onProjectChanged);
    project?.close();
    consoleSearchFocus.dispose();
    hierarchySearchFocus.dispose();
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
        return HierarchyPanel(model: h, onOpenDocument: openDocumentDialog, searchFocus: hierarchySearchFocus);
      },
    ),
    PanelSpec(
      id: PanelRegistry.scene,
      title: 'Scene',
      icon: Icons.view_in_ar_outlined,
      home: PanelHome.center,
      builder: (_) => const ComingPanel(
        title: 'Scene',
        item: 6,
        what: 'The edit-preview world: camera, picking, gizmos.',
        icon: Icons.view_in_ar_outlined,
      ),
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
      builder: (_) => const ComingPanel(
        title: 'Project',
        item: 5,
        what: 'The asset database as folders and a searchable list.',
        icon: Icons.folder_outlined,
      ),
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
      title: 'Import',
      icon: Icons.download_outlined,
      home: PanelHome.bottom,
      builder: (_) => const ComingPanel(
        title: 'Import',
        item: 5,
        what: 'Import settings of the selected source.',
        icon: Icons.download_outlined,
      ),
    ),
  ];

  // ── Commands ────────────────────────────────────────────────────────────

  bool get _hasProject => project != null;
  OpenDocument? get _focusedTab => tabs?.focused;
  bool get _hasTab => _focusedTab != null;
  bool get _canEdit => hierarchy?.canEdit ?? false;
  bool get _hasSelection => hierarchy?.hasSelection ?? false;

  void _registerCommands() {
    SingleActivator k(LogicalKeyboardKey key, {bool shift = false, bool alt = false}) =>
        SingleActivator(key, meta: true, shift: shift, alt: alt);
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
      EditorCommand(
        id: 'app.preferences',
        baseLabel: 'Preferences…',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.comma),
        group: 9,
        run: showPreferences,
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
      inspector?.dispose();
      codegen?.dispose();
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
      opened.addListener(_onProjectChanged);
      c.addListener(registry.refresh);
      t.addListener(registry.refresh);
      h.addListener(registry.refresh);
      ins.addListener(registry.refresh);
      setState(() {
        project = opened;
        workspace = ws;
        tabs = t;
        hierarchy = h;
        inspector = ins;
        codegen = cg;
        console = c;
        _layoutNotice = ws.restoreProblem == null
            ? null
            : 'The saved layout was replaced by the default: ${ws.restoreProblem}.';
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
    setState(() {
      project = null;
      console = null;
      hierarchy = null;
      inspector = null;
      codegen = null;
      tabs = null;
      workspace?.dispose();
      workspace = null;
    });
    ins?.dispose();
    cg?.dispose();
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
