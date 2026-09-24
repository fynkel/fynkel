// The app root: preferences → theme; launcher until a project is open, then
// the main window; the command registry, its shortcuts and the menu bar
// live here because they outlive any one project.
import 'dart:async';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
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
import 'hierarchy/presets.dart';
import 'hierarchy/hierarchy_panel.dart';
import 'hierarchy/hierarchy_tree.dart';
import 'documents/editor_document.dart';
import 'inspector/codegen_check.dart';
import 'l10n/strings.dart';
import 'inspector/inspector_model.dart';
import 'inspector/inspector_panel.dart';
import 'inspector/material_edits.dart';
import 'inspector/prefab_actions.dart' as prefab;
import 'launcher/launcher.dart';
import 'launcher/opening_screen.dart';
import 'picker/picker.dart';
import 'play/code_reload.dart';
import 'play/dart_stack.dart';
import 'play/editor_game.dart';
import 'play/game_console.dart';
import 'play/game_panel.dart';
import 'play/game_preview.dart';
import 'play/game_record.dart';
import 'play/live_play.dart';
import 'play/play_prompt.dart';
import 'play/play_session.dart';
import 'prefs/preferences.dart';
import 'prefs/preferences_dialog.dart';
import 'profiler/profiler_model.dart';
import 'profiler/profiler_panel.dart';
import 'project/editor_handover.dart';
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
import 'theme/tokens.dart' show FynkelTokens;

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
    this.startCodegen = startProcess,
    this.nativeMenuBar,
    this.quit,
    this.initialProject,
    this.sceneViewport = true,
    this.game,
    this.codeReloadStatus,
    this.startEditor = startDetachedEditor,
    this.isGame = isGameProject,
  });

  final Preferences prefs;
  final LayoutStore layoutStore;
  final OpenProject openProject;
  final ChooseDirectory chooseDirectory;
  final ChooseSavePath chooseSavePath;

  /// The project's game when this is its project editor (ADR-0021, built
  /// by `fynkel editor <dir>`); null in the plain editor.
  final EditorGame? game;

  /// Starts a game project's own editor when one is opened (P3c); null
  /// opens every project here.
  final StartEditorProcess? startEditor;
  final bool Function(String dir) isGame;

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

  /// Starts `fynkel codegen --watch` (a saved script updates the Inspector).
  final StartProcess startCodegen;
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
            startCodegen: startCodegen,
            sceneViewport: sceneViewport,
            nativeMenuBar: nativeMenuBar,
            quit: quit ?? () => exit(0),
            initialProject: initialProject,
            game: game,
            codeReloadStatus: codeReloadStatus ?? CodeReloadStatus.environmentPath,
            startEditor: startEditor,
            isGame: isGame,
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
    this.startCodegen = startProcess,
    this.nativeMenuBar,
    this.initialProject,
    this.sceneViewport = true,
    this.game,
    this.codeReloadStatus,
    this.startEditor,
    this.isGame = isGameProject,
  });

  final Preferences prefs;
  final LayoutStore layoutStore;
  final OpenProject openProject;
  final ChooseDirectory chooseDirectory;
  final ChooseSavePath chooseSavePath;
  final EditorGame? game;
  final StartEditorProcess? startEditor;
  final bool Function(String dir) isGame;
  final String? codeReloadStatus;
  final bool sceneViewport;
  final Future<Toolchain> Function(Preferences prefs) detectToolchain;
  final RunProcess run;
  final StartPlayProcess startPlayProcess;
  final StartProcess startCodegen;
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
  ProfilerModel? profiler;
  DocumentTabs? tabs;

  /// The material Inspector's edits, project-wide (Tier 2).
  MaterialEdits? materials;
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

  /// The project this editor was started on, until its open is over: the
  /// window says *Opening …* meanwhile, not the launcher.
  String? _startingOn;

  /// `fynkel editor`'s hot reloads (ADR-0021): each outcome is a notice.
  CodeReloadStatus? codeReload;

  /// Play inside the editor, in the Game view (ADR-0021 P4) — only when
  /// the project's game is compiled into this editor (`fynkel editor`).
  GameRun? gameRun;

  /// The Game view between runs: the active camera's picture (P4b).
  GamePreview? _gamePreview;

  /// Live Play mode (ADR-0021 P7): the Hierarchy and the Inspector on the
  /// running game's world while it runs.
  LivePlay? _livePlay;

  void _onPlayWorlds() {
    final t = tabs;
    if (!identical(_livePlay?.tabs, t)) {
      _livePlay?.dispose();
      _livePlay = t == null ? null : LivePlay(tabs: t, written: () => gameRun?.control.redraw());
    }
    final run = gameRun;
    final running = run != null && run.running;
    _livePlay?.follow(running ? run.control.world : null);
    // The play-mode tint wraps the window: build again when Play starts or ends.
    if (running != _playing && mounted) setState(() => _playing = running);
  }

  bool _playing = false;

  void _onCodeReload() {
    final r = codeReload?.last;
    if (r == null) return;
    final files = r.files.isEmpty ? '' : ' (${r.files.join(', ')})';
    switch (r.state) {
      case 'reloaded':
        notices.show('Code reloaded in ${r.ms ?? '?'} ms$files', severity: NoticeSeverity.info);
      case 'reloadFailed':
        notices.show('Code did not reload$files: ${r.message ?? 'no reason given'}');
      case 'reloadRejected':
        // It compiles, but only a restart applies it (P3b). With nothing
        // to lose the editor restarts at once, as the ADR says; with
        // unsaved documents or a running game it asks first.
        final unsaved = tabs?.anyDirty ?? false;
        final playing = gameRun?.running ?? false;
        if (!unsaved && !playing) {
          notices.show('A code change needs a restart$files: restarting…', severity: NoticeSeverity.info);
          unawaited(restartToApply());
        } else {
          notices.show(
            'A code change needs a restart$files${playing ? ', which ends Play' : ''}: ${r.message ?? ''}',
            severity: NoticeSeverity.warning,
            action: NoticeAction(unsaved ? 'Save and Restart' : 'Restart', () => unawaited(restartToApply())),
          );
        }
      case 'restarted':
        notices.show('Restarted to apply the code changes in ${r.ms ?? '?'} ms', severity: NoticeSeverity.info);
    }
  }

  /// Applies code changes a hot reload cannot (P3b): saves every document,
  /// stops Play, remembers the open tabs and selection, and asks
  /// `fynkel editor` for a hot restart. The restarted editor opens the
  /// project again and puts the tabs back.
  Future<void> restartToApply() async {
    final status = codeReload;
    if (status == null) return;
    final failed = tabs?.saveAll() ?? const <OpenDocument>[];
    if (failed.isNotEmpty) {
      _showError('Did not restart: ${failed.map((d) => d.title).join(', ')} could not be saved.');
      return;
    }
    await stopPlaying();
    workspace?.save();
    _rememberSession();
    editorState?.flush();
    if (status.requestRestart()) {
      notices.show('Restarting to apply the code changes…', severity: NoticeSeverity.info);
    } else {
      _showError('Could not ask `fynkel editor` to restart.');
    }
  }

  /// Game records that came in while a frame was building (a game makes
  /// its world in `initState`, and the world's first record comes at once):
  /// held, in order, until the build is over — the Play log notifies.
  List<String>? _heldRecords;

  void _receiveRecord(String line) {
    final held = _heldRecords;
    if (held != null) {
      held.add(line);
      return;
    }
    if (SchedulerBinding.instance.schedulerPhase != SchedulerPhase.persistentCallbacks) {
      play?.receive(line);
      return;
    }
    _heldRecords = [line];
    scheduleMicrotask(() {
      final lines = _heldRecords ?? const <String>[];
      _heldRecords = null;
      for (final l in lines) {
        play?.receive(l);
      }
    });
  }

  @override
  void initState() {
    super.initState();
    ide = IdeLauncher(widget.prefs);
    final status = widget.codeReloadStatus;
    if (status != null) codeReload = CodeReloadStatus(status)..addListener(_onCodeReload);
    final game = widget.game;
    if (game != null) {
      gameRun = GameRun(
        game: game,
        onRecord: _receiveRecord,
        onQuit: () => unawaited(stopPlaying()),
      )..addListener(registry.refresh);
      gameRun!.addListener(_onPlayWorlds);
      gameRun!.control.worldsChanged.addListener(_onPlayWorlds);
    }
    _panels = PanelRegistry(_panelSpecs());
    _registerCommands();
    widget.prefs.addListener(_onPrefsChanged);
    final detected = _detect();
    // A project named at launch opens once the tools are known: its open runs
    // the codegen check, which needs the CLI the detection is still finding.
    final initial = widget.initialProject;
    if (initial != null) {
      _startingOn = initial;
      detected.then((_) async {
        // The project it was started on stays here (`fynkel editor` chose).
        if (mounted) await openProjectDir(initial, handOver: false);
        if (mounted) setState(() => _startingOn = null);
      });
    }
  }

  @override
  void dispose() {
    widget.prefs.removeListener(_onPrefsChanged);
    codeReload?.dispose();
    _livePlay?.dispose();
    gameRun?.dispose();
    workspace?.dispose();
    _disposeCopies();
    editorState?.dispose();
    codegen?.dispose();
    tabs?.dispose();
    materials?.dispose();
    project?.removeListener(_onProjectChanged);
    project?.close();
    gameConsole?.dispose();
    profiler?.dispose();
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
      title: EditorStrings.panelHierarchy,
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
      title: EditorStrings.panelScene,
      icon: FIcons.viewInArOutlined,
      fullTitle: EditorStrings.panelSceneFull,
      home: PanelHome.center,
      copies: true,
      builder: (_, id) {
        final s = _scenes[id];
        if (s == null) return const SizedBox.shrink();
        return ScenePanel(model: s, showViewport: widget.sceneViewport);
      },
    ),
    PanelSpec(
      id: PanelRegistry.game,
      title: EditorStrings.panelGame,
      icon: FIcons.sportsEsports,
      fullTitle: EditorStrings.panelGameFull,
      home: PanelHome.center,
      builder: (_, _) => GamePanel(run: gameRun, preview: _gamePreview, showViewport: widget.sceneViewport),
    ),
    PanelSpec(
      id: PanelRegistry.inspector,
      title: EditorStrings.panelInspector,
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
          materials: materials,
        );
      },
    ),
    PanelSpec(
      id: PanelRegistry.project,
      title: EditorStrings.panelProject,
      icon: FIcons.folderOutlined,
      fullTitle: EditorStrings.panelProjectFull,
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
          onFindReferences: findReferencesInScene,
          run: widget.run,
          searchFocus: PanelIds.number(id) == 1 ? projectSearchFocus : null,
          onOpenScript: (file) => ide.open(file),
          generateBehaviours: codegen == null ? null : () => codegen!.generate(),
        );
      },
    ),
    PanelSpec(
      id: PanelRegistry.console,
      title: EditorStrings.panelConsole,
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
      title: EditorStrings.panelProfiler,
      icon: FIcons.speed,
      home: PanelHome.bottom,
      builder: (_, _) {
        final m = profiler;
        if (m == null) return const SizedBox.shrink();
        return ProfilerPanel(key: const Key('profiler.panel'), model: m);
      },
    ),
    PanelSpec(
      id: PanelRegistry.animation,
      title: EditorStrings.panelAnimation,
      icon: FIcons.animation,
      home: PanelHome.bottom,
      builder: (_, _) => const ComingPanel(
        title: EditorStrings.panelAnimation,
        item: 6,
        what: 'Clips and the pose of the selection.',
        icon: FIcons.animation,
      ),
    ),
  ];

  // ── Commands ────────────────────────────────────────────────────────────

  bool get _hasProject => project != null;
  OpenDocument? get _focusedTab => tabs?.focused;

  /// The material ⌘Z (⇧⌘Z) undoes instead of the focused tab: the one the
  /// Inspector shows, while the Inspector or the Project has the keyboard.
  EditorDocument? _materialUndo({bool redo = false}) {
    final panel = PanelIds.base(workspace?.focusedPanel.value ?? '');
    return materials?.undoTarget(
      inspectorFocused: panel == PanelRegistry.inspector || panel == PanelRegistry.project,
      redo: redo,
    );
  }
  bool get _hasTab => _focusedTab != null;
  bool get _canEdit => hierarchy?.canEdit ?? false;
  bool get _hasSelection => hierarchy?.hasSelection ?? false;
  bool get _hasView => scene?.hasView ?? false;
  bool get _canEditAssets => browser?.canEdit ?? false;

  /// Prefab mode: the one prefab instance the selection is about (the
  /// instance itself or its derived entities), or null.
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
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyO, shift: true),
        group: 1,
        order: 0,
        enabled: () => _hasProject,
        run: openDocumentDialog,
      ),
      EditorCommand(
        id: 'document.save',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyS),
        group: 1,
        order: 1,
        enabled: () => _focusedTab?.dirty ?? false,
        run: () => saveTab(_focusedTab!),
      ),
      EditorCommand(
        id: 'document.saveAs',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyS, shift: true),
        group: 1,
        order: 2,
        enabled: () => _hasTab,
        run: () => saveTabAs(_focusedTab!),
      ),
      EditorCommand(
        id: 'document.saveAll',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyS, alt: true),
        group: 1,
        order: 3,
        enabled: () => tabs?.anyDirty ?? false,
        run: saveAllTabs,
      ),
      EditorCommand(
        id: 'document.revert',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyR, alt: true),
        group: 1,
        order: 4,
        enabled: () => _focusedTab?.dirty ?? false,
        run: () => revertTab(_focusedTab!),
      ),
      EditorCommand(
        id: 'document.close',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyW),
        group: 1,
        order: 5,
        enabled: () => _hasTab,
        run: () => closeTab(_focusedTab!),
      ),
      EditorCommand(
        id: 'document.next',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.arrowRight, alt: true),
        group: 1,
        order: 6,
        enabled: () => (tabs?.open.length ?? 0) > 1,
        run: () => tabs?.focusNext(),
      ),
      EditorCommand(
        id: 'document.previous',
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
        labelBuilder: () => _materialUndo() != null ? 'Undo Material Change' : _focusedTab?.canUndoLabel ?? 'Undo',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyZ),
        group: 0,
        order: 0,
        enabled: () => _materialUndo() != null || (_focusedTab?.document.canUndo ?? false),
        textIntent: const UndoTextIntent(SelectionChangedCause.keyboard),
        run: () {
          final m = _materialUndo();
          m != null ? materials!.undo(m) : _focusedTab?.undo();
        },
      ),
      EditorCommand(
        id: 'document.redo',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyZ, shift: true),
        group: 0,
        order: 1,
        enabled: () => _materialUndo(redo: true) != null || (_focusedTab?.document.canRedo ?? false),
        textIntent: const RedoTextIntent(SelectionChangedCause.keyboard),
        run: () {
          final m = _materialUndo(redo: true);
          m != null ? materials!.undo(m, redo: true) : _focusedTab?.redo();
        },
      ),
      EditorCommand(
        id: 'assets.undo',
        labelBuilder: () {
          final l = browser?.history.undoLabel;
          return l == null ? 'Undo Asset Operation' : 'Undo $l';
        },
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyZ, alt: true),
        group: 0,
        order: 2,
        enabled: () => _canEditAssets && (browser?.history.canUndo ?? false),
        run: () => browser?.undo(),
      ),
      EditorCommand(
        id: 'assets.redo',
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
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyR),
        group: 1,
        order: 0,
        enabled: () => _canEdit && (hierarchy?.selection.length ?? 0) == 1,
        run: () => hierarchy?.beginRename(),
      ),
      EditorCommand(
        id: 'hierarchy.duplicate',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyD),
        group: 1,
        order: 1,
        enabled: () => _canEdit && _hasSelection,
        run: () => hierarchy?.duplicate(),
      ),
      // Unity's object clipboard: a text field keeps its own ⌘C/⌘X/⌘V.
      EditorCommand(
        id: 'hierarchy.copy',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyC),
        group: 1,
        order: 0,
        enabled: () => _hasTab && _hasSelection,
        textIntent: CopySelectionTextIntent.copy,
        run: () => hierarchy?.copy(),
      ),
      EditorCommand(
        id: 'hierarchy.cut',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyX),
        group: 1,
        order: 0,
        enabled: () => _canEdit && _hasSelection,
        textIntent: CopySelectionTextIntent.cut(SelectionChangedCause.keyboard),
        run: () => hierarchy?.cut(),
      ),
      EditorCommand(
        id: 'hierarchy.paste',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyV),
        group: 1,
        order: 0,
        enabled: () => hierarchy?.canPaste ?? false,
        textIntent: const PasteTextIntent(SelectionChangedCause.keyboard),
        run: () => hierarchy?.paste(),
      ),
      EditorCommand(
        id: 'hierarchy.pasteAsChild',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyV, shift: true),
        group: 1,
        order: 0,
        enabled: () => (hierarchy?.canPaste ?? false) && _hasSelection,
        run: () => hierarchy?.paste(asChild: true),
      ),
      EditorCommand(
        id: 'hierarchy.delete',
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
        menu: EditorMenu.gameObject,
        // Unity's GameObject → Create Empty: at the root.
        shortcut: k(LogicalKeyboardKey.keyN, shift: true),
        group: 1,
        order: 3,
        enabled: () => _canEdit,
        run: () => hierarchy?.createEmpty(underSelection: false),
      ),
      EditorCommand(
        id: 'hierarchy.createEmptyChild',
        menu: EditorMenu.gameObject,
        // Unity's ⌥⇧N: under the selected object (no ⌘, so never while typing).
        key: const SingleActivator(LogicalKeyboardKey.keyN, shift: true, alt: true, includeRepeats: false),
        group: 1,
        order: 3,
        enabled: () => _canEdit && _hasSelection,
        run: () => hierarchy?.createEmpty(),
      ),
      EditorCommand(
        id: 'hierarchy.createWith',
        menu: EditorMenu.gameObject,
        shortcut: k(LogicalKeyboardKey.keyN, shift: true, alt: true),
        group: 1,
        order: 4,
        enabled: () => _canEdit,
        run: createWithDialog,
      ),
      // Unity's GameObject → 3D Object / Light / Camera (ADR-0023): at the
      // root, as the menu bar has no row to put them under.
      for (final p in GameObjectPreset.values)
        EditorCommand(
          id: p.commandId,
          menu: EditorMenu.gameObject,
          submenu: switch (p.group) {
            PresetGroup.object3d => EditorStrings.menu3dObject,
            PresetGroup.light => EditorStrings.menuLight,
            PresetGroup.camera => null,
          },
          group: 2 + p.group.index,
          order: p.index,
          enabled: () => _canEdit,
          run: () => hierarchy?.createPreset(p, underSelection: false),
        ),
      EditorCommand(
        id: 'hierarchy.keepWorldTransform',
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
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.bracketRight, alt: true),
        group: 2,
        order: 1,
        enabled: () => _hasTab,
        run: () => hierarchy?.expandAll(),
      ),
      EditorCommand(
        id: 'hierarchy.collapseAll',
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
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyS, shift: true, alt: true),
        group: 4,
        order: 0,
        enabled: () => _canEditAssets,
        run: () => _newAsset('scene'),
      ),
      EditorCommand(
        id: 'assets.newPrefab',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyP, shift: true, alt: true),
        group: 4,
        order: 1,
        enabled: () => _canEditAssets,
        run: () => _newAsset('prefab'),
      ),
      EditorCommand(
        id: 'assets.newMaterial',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyM, shift: true, alt: true),
        group: 4,
        order: 2,
        enabled: () => _canEditAssets,
        run: () => _newAsset('material'),
      ),
      // Unity's Create → Volume Profile (no shortcut there either).
      EditorCommand(
        id: 'assets.newVolume',
        menu: EditorMenu.project,
        group: 4,
        order: 2,
        enabled: () => _canEditAssets,
        run: () => _newAsset('volume'),
      ),
      EditorCommand(
        id: 'assets.newFolder',
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
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyV, shift: true, alt: true),
        group: 4,
        order: 6,
        enabled: () => _canEditAssets && _hasAssetSelection,
        run: moveAssetsDialog,
      ),
      EditorCommand(
        id: 'assets.duplicate',
        menu: EditorMenu.project,
        // ⌘D in the Project browser (the Hierarchy's ⌘D is the global one).
        group: 4,
        order: 6,
        enabled: () => _canEditAssets && _hasAssetSelection,
        run: () {
          workspace?.show(PanelRegistry.project);
          browser?.duplicate();
        },
      ),
      EditorCommand(
        id: 'assets.delete',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.backspace, shift: true, alt: true),
        group: 4,
        order: 7,
        enabled: () => _canEditAssets && _hasAssetSelection,
        run: () {
          workspace?.show(PanelRegistry.project);
          browser?.requestDelete();
        },
      ),
      EditorCommand(
        id: 'assets.reimport',
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
        menu: EditorMenu.project,
        // ⌥⌘P went to Step, as in Unity.
        shortcut: k(LogicalKeyboardKey.keyB, alt: true),
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
        menu: EditorMenu.project,
        group: 0,
        order: 0,
        run: newProject,
      ),
      EditorCommand(
        id: 'project.open',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyO),
        group: 0,
        order: 1,
        run: openProjectDialog,
      ),
      EditorCommand(
        id: 'project.close',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyW, shift: true),
        group: 2,
        order: 0,
        enabled: () => _hasProject,
        run: closeProject,
      ),
      EditorCommand(
        id: 'project.revalidate',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyV, alt: true),
        group: 2,
        order: 1,
        enabled: () => _hasProject && !(project?.validating ?? true),
        run: () => project?.revalidate(),
      ),
      EditorCommand(
        id: 'project.migrations',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyM, alt: true),
        group: 2,
        order: 3,
        enabled: () => _hasProject,
        run: () => showMigrationPreview(context, project!),
      ),
      EditorCommand(
        id: 'project.reveal',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyR, shift: true),
        group: 2,
        order: 2,
        enabled: () => _hasProject,
        run: () => revealInFileBrowser(p.join(project!.dir, 'fynkel.project.json'), run: widget.run),
      ),
      EditorCommand(
        id: 'app.quit',
        menu: EditorMenu.project,
        shortcut: k(LogicalKeyboardKey.keyQ),
        group: 9,
        run: () {
          _rememberSession();
          editorState?.flush();
          widget.quit();
        },
      ),
      // ── Scene view: tools and the editor-only flags (Edit menu, group 2) ──
      for (final (i, tool) in SceneTools.all.indexed)
        EditorCommand(
          id: 'scene.tool.${tool.id}',
          baseLabel: EditorStrings.toolCommand(tool.label),
          description: EditorStrings.toolDescription(tool.label, tool.key?.keyLabel),
          menu: EditorMenu.edit,
          // Unity's bare Q W E R Y only: ⌘1… are Go to <panel>, as in Unity.
          key: tool.key == null ? null : bare(tool.key!),
          group: 2,
          order: i,
          enabled: () => _hasView,
          run: () => scene?.tool = tool,
        ),
      EditorCommand(
        id: 'scene.toggleSpace',
        labelBuilder: () => scene?.space == GizmoSpace.local ? 'Use World Axes' : 'Use Local Axes',
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
        labelBuilder: () => scene?.snap.enabled ?? false ? 'Snapping Off' : 'Snapping On',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyG, alt: true),
        group: 2,
        order: 5,
        enabled: () => _hasView,
        run: () => scene?.toggleSnap(),
      ),
      EditorCommand(
        id: 'scene.toggleHidden',
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
        menu: EditorMenu.edit,
        // ⌘K is the Command Palette (Unity's Search).
        key: bare(LogicalKeyboardKey.keyL),
        group: 3,
        order: 2,
        enabled: () => _hasView && _hasSelection,
        run: () => scene?.toggleLockedSelection(),
      ),
      EditorCommand(
        id: 'scene.alignCameraToView',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.keyL, shift: true),
        group: 3,
        order: 3,
        enabled: () => _canEdit && (scene?.canAlignToSelectedCamera ?? false),
        run: () => scene?.alignSelectedCameraToView(),
      ),
      EditorCommand(
        id: 'app.preferences',
        menu: EditorMenu.edit,
        shortcut: k(LogicalKeyboardKey.comma),
        group: 9,
        run: showPreferences,
      ),
      // ── View menu: the Scene view's camera and overlays ──
      EditorCommand(
        id: 'view.frameSelected',
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
        labelBuilder: () => scene?.camera?.orthographic ?? false ? 'Perspective' : 'Orthographic',
        menu: EditorMenu.view,
        // Bare 5 only: ⌘5 is Go to Project, as in Unity.
        key: bare(LogicalKeyboardKey.digit5),
        group: 0,
        order: 2,
        enabled: () => _hasView,
        run: () => scene?.toggleProjection(),
      ),
      EditorCommand(
        id: 'view.alignToCamera',
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.keyL, alt: true),
        group: 0,
        order: 3,
        enabled: () => scene?.canAlignToSelectedCamera ?? false,
        run: () => scene?.alignViewToSelectedCamera(),
      ),
      EditorCommand(
        id: 'view.reset',
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.digit0, shift: true),
        group: 0,
        order: 4,
        enabled: () => _hasView,
        run: () => scene?.resetView(),
      ),
      EditorCommand(
        id: 'view.toggleGrid',
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
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.keyR, control: true),
        group: 2,
        order: 0,
        enabled: () => _hasTab && (_focusedTab!.kind == 'scene' || _focusedTab!.kind == 'prefab'),
        run: () => scene?.reload(),
      ),
      EditorCommand(
        id: 'view.recook',
        menu: EditorMenu.view,
        shortcut: k(LogicalKeyboardKey.keyR, control: true, shift: true),
        group: 2,
        order: 1,
        enabled: () => _hasView,
        run: () => scene?.recook(),
      ),
      EditorCommand(
        id: 'view.saveFrame',
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
        labelBuilder: () => play?.isActive ?? false ? 'Stop Playing' : 'Play',
        menu: EditorMenu.play,
        // Unity: ⌘P toggles Play and Stop.
        shortcut: k(LogicalKeyboardKey.keyP),
        group: 0,
        order: 0,
        enabled: () => _hasProject,
        run: () => (play?.isActive ?? false) ? stopPlaying() : playProject(),
      ),
      EditorCommand(
        id: 'play.buildAndRun',
        menu: EditorMenu.play,
        shortcut: k(LogicalKeyboardKey.keyB),
        group: 0,
        order: 5,
        enabled: () => _hasProject && !(play?.isActive ?? false),
        run: buildAndRun,
      ),
      EditorCommand(
        id: 'play.stop',
        menu: EditorMenu.play,
        shortcut: k(LogicalKeyboardKey.period),
        group: 0,
        order: 1,
        enabled: () => play?.isActive ?? false,
        run: stopPlaying,
      ),
      EditorCommand(
        id: 'play.pause',
        labelBuilder: () => (gameRun?.running ?? false) && gameRun!.paused ? 'Resume' : 'Pause',
        menu: EditorMenu.play,
        shortcut: k(LogicalKeyboardKey.keyP, shift: true),
        group: 0,
        order: 3,
        // Unity: Pause may be on before Play; the game starts paused.
        enabled: () => gameRun != null && _hasProject,
        run: () => gameRun?.togglePause(),
      ),
      EditorCommand(
        id: 'play.step',
        menu: EditorMenu.play,
        shortcut: k(LogicalKeyboardKey.keyP, alt: true),
        group: 0,
        order: 4,
        enabled: () => gameRun?.running ?? false,
        run: () => gameRun?.step(),
      ),
      EditorCommand(
        id: 'play.gizmos',
        labelBuilder: () => editorState?.playGizmos ?? false ? 'Hide Gizmos in Play' : 'Show Gizmos in Play',
        menu: EditorMenu.play,
        shortcut: k(LogicalKeyboardKey.keyG, shift: true, alt: true),
        group: 0,
        order: 2,
        enabled: () => _hasProject,
        run: togglePlayGizmos,
      ),
      EditorCommand(
        id: 'play.restart',
        menu: EditorMenu.play,
        shortcut: k(LogicalKeyboardKey.keyR, control: true, alt: true),
        group: 1,
        order: 10,
        enabled: () => codeReload != null && _hasProject,
        run: () => unawaited(restartToApply()),
      ),
      EditorCommand(
        id: 'play.showConsole',
        menu: EditorMenu.play,
        shortcut: k(LogicalKeyboardKey.keyJ, alt: true),
        group: 1,
        order: 0,
        enabled: () => _hasProject,
        run: showGameConsole,
      ),
      EditorCommand(
        id: 'play.clearConsole',
        menu: EditorMenu.play,
        shortcut: k(LogicalKeyboardKey.keyK, shift: true),
        group: 1,
        order: 1,
        enabled: () => (play?.records.isNotEmpty ?? false),
        run: () => play?.clear(),
      ),
      EditorCommand(
        id: 'window.commandPalette',
        menu: EditorMenu.window,
        shortcut: k(LogicalKeyboardKey.keyK),
        group: 0,
        run: showPalette,
      ),
      EditorCommand(
        id: 'window.resetLayout',
        menu: EditorMenu.window,
        shortcut: k(LogicalKeyboardKey.digit0, alt: true),
        group: 1,
        enabled: () => _hasProject,
        run: resetLayout,
      ),
      for (final (i, id) in PanelRegistry.defaultOrder.indexed)
        EditorCommand(
          id: 'window.panel.$id',
          baseLabel: EditorStrings.togglePanel(_panels.byId(id)!.title),
          labelBuilder: () =>
              EditorStrings.showOrHidePanel(_panels.byId(id)!.title, shown: workspace?.isShown(id) ?? true),
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
          baseLabel: EditorStrings.newPanel(_panels.byId(id)!.name),
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
      // Unity's keys for its windows: ⌘1 Scene, ⌘2 Game, ⌘3 Inspector,
      // ⌘4 Hierarchy, ⌘5 Project, ⌘7 Profiler, ⇧⌘C Console.
      for (final (i, (id, shortcut)) in [
        (PanelRegistry.scene, k(LogicalKeyboardKey.digit1)),
        (PanelRegistry.game, k(LogicalKeyboardKey.digit2)),
        (PanelRegistry.inspector, k(LogicalKeyboardKey.digit3)),
        (PanelRegistry.hierarchy, k(LogicalKeyboardKey.digit4)),
        (PanelRegistry.project, k(LogicalKeyboardKey.digit5)),
        (PanelRegistry.profiler, k(LogicalKeyboardKey.digit7)),
        (PanelRegistry.console, k(LogicalKeyboardKey.keyC, shift: true)),
      ].indexed)
        if (_panels.byId(id) != null)
          EditorCommand(
            id: 'window.goto.$id',
            baseLabel: EditorStrings.goToPanel(_panels.byId(id)!.name),
            description: EditorStrings.goToPanelDescription(_panels.byId(id)!.name),
            menu: EditorMenu.window,
            shortcut: shortcut,
            group: 5,
            order: i,
            enabled: () => _hasProject,
            run: () => workspace?.focusKeyboard(id),
          ),
      EditorCommand(
        id: 'console.focusSearch',
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
        menu: EditorMenu.window,
        shortcut: k(LogicalKeyboardKey.keyC, alt: true),
        group: 3,
        order: 1,
        enabled: () => _hasProject,
        run: () => console?.copy(),
      ),
      EditorCommand(
        id: 'console.openInIde',
        menu: EditorMenu.window,
        shortcut: k(LogicalKeyboardKey.keyO, alt: true),
        group: 3,
        order: 2,
        enabled: () => console?.canOpenSelected ?? false,
        run: () => console?.openSelected(),
      ),
      EditorCommand(
        id: 'console.openInEditor',
        menu: EditorMenu.window,
        shortcut: k(LogicalKeyboardKey.keyE, alt: true),
        group: 3,
        order: 3,
        enabled: () => console?.canOpenSelectedInEditor ?? false,
        run: () => console?.openSelectedInEditor(),
      ),
      EditorCommand(
        id: 'help.documentation',
        menu: EditorMenu.help,
        shortcut: k(LogicalKeyboardKey.keyD, alt: true),
        enabled: () => toolchain?.fynkelRoot != null,
        run: () => revealInFileBrowser(p.join(toolchain!.fynkelRoot!, 'docs', 'editor.md'), run: widget.run),
      ),
      EditorCommand(
        id: 'help.about',
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
          baseLabel: EditorStrings.openRecent(r.name.isEmpty ? p.basename(r.dir) : r.name),
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

  /// Opens [dir] here — or, for a game project that is not this editor's
  /// own, in the project's editor, where Play runs inside ([handOver], P3c).
  Future<void> openProjectDir(String dir, {bool handOver = true}) async {
    if (_opening) return;
    if (handOver && _handsOver(dir)) {
      _opening = true;
      var openHere = false;
      try {
        openHere = await _handOver(dir);
      } finally {
        _opening = false;
      }
      if (openHere && mounted) await openProjectDir(dir, handOver: false);
      return;
    }
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
      materials?.dispose();
      final ws = Workspace(registry: _panels, store: widget.layoutStore);
      final t = DocumentTabs(project: opened);
      final me = MaterialEdits(opened)..preview = (id, fields) => scene?.previewMaterial(id, fields);
      me.addListener(registry.refresh);
      final h = HierarchyModel(tabs: t, project: opened);
      final c = ConsoleModel(project: opened, openInIde: ide.open, openInEditor: openDiagnosticInEditor);
      final ps = PlaySession(projectDir: opened.dir, cli: () => toolchain?.fynkelCli, start: widget.startPlayProcess);
      final gc = GameConsoleModel(
        session: ps,
        relative: opened.relative,
        openRecord: openGameRecord,
        projectDir: opened.dir,
        ownPackage: () => opened.behaviourMetadata()?['package'] as String? ?? packageNameOf(opened.dir),
        openInIde: ide.open,
      );
      final pm = ProfilerModel(session: ps);
      final cg = CodegenCheck(
        projectDir: opened.dir,
        run: widget.run,
        cli: () => toolchain?.fynkelCli,
        repoRoot: () => toolchain?.fynkelRoot,
        start: widget.startCodegen,
      );
      final es = EditorState.load(opened.dir);
      ps.gizmos = es.playGizmos;
      opened.addListener(_onProjectChanged);
      c.addListener(registry.refresh);
      ps.addListener(registry.refresh);
      t.addListener(_onTabsChanged);
      gameConsole?.dispose();
      profiler?.dispose();
      play?.dispose();
      project = opened;
      tabs = t;
      materials = me;
      codegen = cg;
      editorState = es;
      _sceneOps = h;
      _sceneViews = SceneViews();
      _gamePreview = GamePreview(tabs: t, scene: () => scene, views: _sceneViews);
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
        profiler = pm;
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
      _restoreSession();
      widget.prefs.addRecent(opened.dir, opened.name);
      registry.refresh();
      // A saved script regenerates its metadata and the Inspector follows
      // (Unity's recompile): `fynkel codegen --watch` from open, whose first
      // build is the check; a CLI without it checks once and runs per save.
      if (cg.isDartPackage) {
        unawaited(
          cg
              .watch(
                opened.fileChanges,
                reload: () {
                  if (!identical(project, opened)) return;
                  try {
                    opened.reloadBehaviours();
                  } on AuthoringException catch (e) {
                    _showError('The behaviour metadata could not be read again: $e');
                  }
                },
              )
              .then((_) {
                if (identical(codegen, cg) && !cg.watching && opened.behaviourMetadata() != null) {
                  unawaited(cg.check());
                }
              }),
        );
      }
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
    _rememberSession();
    registry.refresh();
  }

  /// What reopens [tab]: its asset id, or `<id>#prefab`.
  static String _tabKey(OpenDocument tab) {
    final sub = tab.document.sub;
    return sub == null ? tab.document.id : '${tab.document.id}#$sub';
  }

  /// True while the tabs are put back, or closed because the project is
  /// (the session is what was open, not what closing left).
  bool _sessionFrozen = false;

  /// The open tabs, the one in front and each one's selection, into
  /// `.fynkel/editor.json` (P3b); written only when they changed.
  void _rememberSession() {
    final t = tabs, es = editorState;
    if (t == null || es == null || _sessionFrozen) return;
    es.setSession(
      tabs: [for (final tab in t.open) _tabKey(tab)],
      focused: t.focused == null ? null : _tabKey(t.focused!),
      selection: {
        for (final tab in t.open)
          if (tab.selection.isNotEmpty) _tabKey(tab): tab.selection.toList(),
      },
    );
  }

  /// Puts back the tabs [_rememberSession] recorded, as Unity reopens the
  /// scene you were in. A document that no longer opens is left out.
  void _restoreSession() {
    final t = tabs, es = editorState;
    if (t == null || es == null || es.openTabs.isEmpty) return;
    _sessionFrozen = true;
    try {
      for (final key in es.openTabs) {
        final tab = t.openDocument(key, focus: false);
        final ids = es.selections[key];
        if (tab != null && ids != null) tab.setSelection(ids.where(tab.tree.nodes.containsKey));
      }
      final front = es.focusedTab == null ? null : t.open.where((o) => _tabKey(o) == es.focusedTab).firstOrNull;
      t.focus(front ?? t.open.lastOrNull);
      t.openError = null;
    } finally {
      _sessionFrozen = false;
    }
    _rememberSession();
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

  /// Play: in the Game view when the game is compiled into this editor
  /// (ADR-0021), else as its own app ([buildAndRun]). A project with errors
  /// never runs — `fynkel run` would refuse it anyway, and the Console is
  /// where the reasons are.
  Future<void> playProject() async {
    if (gameRun == null) return buildAndRun();
    final opened = project;
    final session = play;
    if (opened == null || session == null || session.isActive) return;
    if (_refusesErrors(opened)) return;
    _lastRunBuilt = false;
    final dirty = List.of(tabs?.dirtyDocuments ?? const <OpenDocument>[]);
    // In the Game view Play runs what the editor holds — unsaved edits
    // included, the files left alone (ADR-0021 P5) — whenever it can cook
    // in process (a Scene view has its preview open). The prompt is the
    // separate process's: it reads files.
    final playsWhatIsHeld = gameRun != null && (scene?.current?.hasPreview ?? false);
    if (dirty.isNotEmpty && !playsWhatIsHeld) {
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
    final run = gameRun;
    if (run != null) await _playInEditor(opened, session, run);
  }

  /// Whether the last run was [buildAndRun] (Play again repeats it).
  bool _lastRunBuilt = false;

  bool _refusesErrors(EditorProject opened) {
    // A script that does not compile has no generated files while the
    // watch runs: Unity's "All compiler errors have to be fixed before you
    // can enter Play mode".
    final cg = codegen;
    if (cg != null && cg.watching && cg.status == CodegenStatus.failed) {
      _showError(
        'All script errors have to be fixed before you can enter Play mode. '
        '${cg.error ?? ''}'.trim(),
      );
      return true;
    }
    if (opened.errorCount == 0) return false;
    showGameConsole(project: true);
    _showError(
      'The project has ${opened.errorCount} error${opened.errorCount == 1 ? '' : 's'}; `fynkel run` refuses to '
      'run it. The Console\'s Project tab lists them — double-click a row to go to it.',
    );
    return true;
  }

  /// Build and Run (ADR-0021 P6, ADR-0016): the game as its own app in its
  /// own window, through `fynkel run` — for release-like speed, or a crash
  /// that must not take the editor down. It reads files, not the editor's
  /// memory, so dirty documents get the save prompt first.
  Future<void> buildAndRun() async {
    final opened = project;
    final session = play;
    if (opened == null || session == null || session.isActive) return;
    if (_refusesErrors(opened)) return;
    _lastRunBuilt = true;
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
    await (_lastRunBuilt ? buildAndRun() : playProject());
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

  /// Play in the Game view (ADR-0021): the cache is cooked from the files
  /// on disk, then the game is mounted playing the open scene, the Game
  /// view comes to the front and gets the keyboard.
  Future<void> _playInEditor(EditorProject opened, PlaySession session, GameRun run) async {
    final tab = _focusedTab;
    final sceneId = tab != null && tab.kind == 'scene' ? tab.document.id : null;
    final cooked = await _cookForPlay(opened);
    if (!mounted) return;
    if (cooked != null) {
      _showError('Play could not cook the project: $cooked');
      return;
    }
    workspace?.show(PanelRegistry.game);
    _playEditBaseline = _editSignature();
    run.start(projectDir: opened.dir, startScene: sceneId);
    session.beginInEditor(scene: tab?.kind == 'scene' ? tab!.document.name : null);
    run.focusNode.requestFocus();
    _noteEditDuringPlay();
    registry.refresh();
  }

  /// Brings `.fynkel/cache` up to date for Play. In process through the
  /// Scene view's preview when one is open, with every unsaved document
  /// as the editor holds it (a copy of each goes to `.fynkel/recovery/`
  /// first; the files are untouched); else `fynkel cook` from the files.
  /// Null when it worked, else why not.
  Future<String?> _cookForPlay(EditorProject opened) async {
    final sv = scene;
    if (sv != null && sv.current?.hasPreview == true) {
      final r = sv.recook(live: true, announce: false);
      if (r == null) return sv.notice ?? 'the cook did not run';
      if (!r.ok) return '${r.failed} asset(s) failed to cook — ⌃⇧⌘R in the Scene view shows the report';
      return null;
    }
    final cli = toolchain?.fynkelCli;
    if (cli == null) return 'the `fynkel` CLI was not found';
    final r = await widget.run(cli, ['cook', '--project', opened.dir], workingDirectory: opened.dir);
    if (r.exitCode == 0) return null;
    final err = '${r.stderr}'.trim();
    return err.isEmpty ? '`fynkel cook` exited with code ${r.exitCode}' : err;
  }

  Future<void> stopPlaying() async {
    final session = play;
    final run = gameRun;
    if (run != null && run.running) {
      _livePlay?.follow(null);
      run.stop();
      session?.endInEditor();
      if (mounted) registry.refresh();
      return;
    }
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

  bool _handsOver(String dir) {
    if (widget.startEditor == null || !widget.isGame(dir)) return false;
    final own = widget.game?.projectDir;
    return own == null || !p.equals(p.canonicalize(own), p.canonicalize(dir));
  }

  /// Hands [dir] over to its project editor; true: open it here instead.
  /// This editor quits once the other is up.
  Future<bool> _handOver(String dir) async {
    final tc = toolchain;
    final cli = tc?.fynkelCli, root = tc?.fynkelRoot, flutter = tc?.flutter;
    if (cli == null || root == null || flutter == null) {
      notices.show(
        'Opened in this editor: playing inside the editor needs the Fynkel CLI, the checkout and Flutter '
        '(the launcher\'s checks say what is missing).',
      );
      return true;
    }
    await stopPlaying();
    // A selection alone does not reach the tab listeners: record it now.
    _rememberSession();
    _sessionFrozen = true;
    try {
      for (final tab in List.of(tabs?.dirtyDocuments ?? const <OpenDocument>[])) {
        if (!await closeTab(tab)) return false;
      }
    } finally {
      _sessionFrozen = false;
    }
    workspace?.save();
    if (!mounted) return false;
    final handover = EditorHandover(dir: dir, cli: cli, root: root, flutter: flutter, start: widget.startEditor!);
    unawaited(handover.begin());
    final outcome = await showHandoverDialog(context, handover, name: p.basename(dir));
    handover.dispose();
    switch (outcome) {
      case HandoverOutcome.handedOver:
        // The project editor adds it to the recent projects as it opens.
        widget.quit();
        return false;
      case HandoverOutcome.openHere:
        return true;
      case HandoverOutcome.cancelled:
        return false;
    }
  }

  /// Closes the project; every dirty tab gets its save prompt first, and a
  /// Cancel there keeps the project open.
  Future<void> closeProject() async {
    final closing = project;
    if (closing == null) return;
    await stopPlaying();
    // A selection alone does not reach the tab listeners: record it now.
    _rememberSession();
    _sessionFrozen = true;
    try {
      for (final tab in List.of(tabs?.dirtyDocuments ?? const <OpenDocument>[])) {
        if (!await closeTab(tab)) return;
      }
    } finally {
      _sessionFrozen = false;
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

  /// Unity's ping from an asset field: the asset outlined in the Project
  /// browser (the copy [revealInProject] would use), nothing selected.
  void pingInProject(String assetId) {
    final ws = workspace;
    if (ws == null) return;
    final id = _unlockedCopy(PanelRegistry.project);
    ws.show(id);
    _browsers[id]?.ping(assetId);
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
  String? _revealEntity(OpenDocument tab, String? id) {
    final ws = workspace;
    if (ws == null) return null;
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
    return target;
  }

  /// Unity's Find References In Scene: the focused document's Hierarchy
  /// comes forward searching `ref:<asset>:<name>` — the objects that
  /// reference the asset (× clears it).
  void findReferencesInScene(String asset, String name) {
    final tab = tabs?.focused;
    if (tab == null) return;
    final target = _revealEntity(tab, null);
    if (target == null) return;
    _hierarchies[target]?.setQuery(HierarchyFilter.referencesQuery(asset, name));
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
        ins.pingAsset = pingInProject;
        // A Transform value dragged in the Inspector moves in every Scene view.
        ins.previewTransforms = (tab, locals, ids) {
          for (final sv in _scenes.values) {
            sv.previewLocals(tab, locals, ids);
          }
        };
        // So does a colour being picked.
        ins.previewComponents = (tab, type, data, ids) {
          for (final sv in _scenes.values) {
            sv.previewComponents(tab, type, data, ids);
          }
        };
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
        // Hierarchy → Project makes a prefab of the focused scene's object.
        // (Null from the Hierarchy means "allowed": no `??` fallback here.)
        b.cannotMakePrefab = (ids) => _sceneOps == null ? 'No scene is open.' : _sceneOps!.cannotCreatePrefab(ids);
        b.makePrefab = (ids, folder) => _sceneOps?.createPrefab(ids.single, folder);
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
    _gamePreview?.dispose();
    _gamePreview = null;
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
    Widget body;
    if ((p0 == null || workspace == null) && _startingOn != null) {
      body = OpeningScreen(dir: _startingOn!, prefs: widget.prefs);
    } else if (p0 == null || workspace == null) {
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
        gameRun: gameRun,
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
      if ((gameRun?.running ?? false) && widget.prefs.playModeTint) {
        // Play mode's tint (ADR-0021 P7): the editor's surfaces, not the
        // game's picture.
        final theme = Theme.of(context);
        body = Theme(
          key: const Key('shell.playTint'),
          data: fynkelTheme(brightness: theme.brightness, density: widget.prefs.density, tint: FynkelTokens.playTint),
          child: body,
        );
      }
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
