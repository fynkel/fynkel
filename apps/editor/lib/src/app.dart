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
import 'env/tools.dart';
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
  final consoleSearchFocus = FocusNode(debugLabel: 'console.search');
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
    project?.removeListener(_onProjectChanged);
    project?.close();
    consoleSearchFocus.dispose();
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
      builder: (_) => const ComingPanel(
        title: 'Hierarchy',
        item: 3,
        what: 'The scene tree of the focused document.',
        icon: Icons.account_tree_outlined,
      ),
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
      builder: (_) => const ComingPanel(
        title: 'Inspector',
        item: 4,
        what: 'Components and fields of the selection.',
        icon: Icons.tune,
      ),
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

  void _registerCommands() {
    SingleActivator k(LogicalKeyboardKey key, {bool shift = false, bool alt = false}) =>
        SingleActivator(key, meta: true, shift: shift, alt: alt);
    registry.registerAll([
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
        group: 1,
        order: 0,
        enabled: () => _hasProject,
        run: closeProject,
      ),
      EditorCommand(
        id: 'project.revalidate',
        baseLabel: 'Validate Project',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyV, alt: true),
        group: 1,
        order: 1,
        enabled: () => _hasProject && !(project?.validating ?? true),
        run: () => project?.revalidate(),
      ),
      EditorCommand(
        id: 'project.migrations',
        baseLabel: 'Migration Preview…',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyM, alt: true),
        group: 1,
        order: 3,
        enabled: () => _hasProject,
        run: () => showMigrationPreview(context, project!),
      ),
      EditorCommand(
        id: 'project.reveal',
        baseLabel: 'Reveal in Finder',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyR, shift: true),
        group: 1,
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
      final ws = Workspace(registry: _panels, store: widget.layoutStore);
      final c = ConsoleModel(project: opened, openInIde: ide.open);
      opened.addListener(_onProjectChanged);
      c.addListener(registry.refresh);
      setState(() {
        project = opened;
        workspace = ws;
        console = c;
        _layoutNotice = ws.restoreProblem == null
            ? null
            : 'The saved layout was replaced by the default: ${ws.restoreProblem}.';
        _openError = null;
      });
      widget.prefs.addRecent(opened.dir, opened.name);
      registry.refresh();
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

  Future<void> closeProject() async {
    final closing = project;
    if (closing == null) return;
    workspace?.save();
    closing.removeListener(_onProjectChanged);
    setState(() {
      project = null;
      console = null;
      workspace?.dispose();
      workspace = null;
    });
    await closing.close();
    registry.refresh();
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
      builder: (context, _) => CallbackShortcuts(
        bindings: registry.shortcutBindings,
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
    );
  }
}
