// The main window: one title bar (item 9b — the project menu, the document
// tabs, Play / Stop and the run's state in the middle, the codegen state and
// the command search at the right; on macOS the traffic lights sit on it and its empty space moves
// the window, window_chrome.dart), the banners, the docking workspace, the
// status bar (diagnostic counts → Console; the current notice, notices.dart).
import 'package:docking/docking.dart';
import 'package:flutter/material.dart';
import 'package:path/path.dart' as p;

import '../inspector/codegen_check.dart';
import '../play/game_panel.dart';
import '../play/play_session.dart';
import '../prefs/preferences.dart';
import '../project/editor_project.dart';
import '../theme/chrome.dart';
import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'notices.dart';
import 'window_chrome.dart';
import 'workspace.dart';

class MainWindow extends StatelessWidget {
  const MainWindow({
    super.key,
    required this.project,
    required this.workspace,
    this.tabBar,
    required this.onShowConsole,
    required this.onClose,
    required this.onRevalidate,
    this.onReveal,
    this.onCommandPalette,
    this.recents = const [],
    this.onOpenProject,
    this.onOpenRecent,
    this.codegen,
    this.codegenDrift,
    this.onCheckCodegen,
    required this.onMigrationPreview,
    this.play,
    this.onPlay,
    this.onReplay,
    this.onStop,
    this.gameRun,
    this.layoutNotice,
    this.onDismissNotice,
  });

  final EditorProject project;
  final Workspace workspace;

  /// The document tabs, in the title bar after the project menu.
  final Widget? tabBar;
  final VoidCallback onShowConsole;

  /// The run (ADR-0016): null before a project is open.
  final PlaySession? play;
  final VoidCallback? onPlay;

  /// Play again from the edited-during-play banner: stops the run that is up
  /// and starts a fresh one. [onPlay] refuses while a run is active — the
  /// banner only ever shows while one is — so the banner needs its own.
  final VoidCallback? onReplay;
  final VoidCallback? onStop;

  /// The in-editor Play (ADR-0021), when the game is compiled in: the
  /// title bar then has Pause and Step beside Play, as Unity's toolbar.
  final GameRun? gameRun;
  final VoidCallback onClose;
  final VoidCallback onRevalidate;

  /// Reveal in Finder, from the project menu.
  final VoidCallback? onReveal;

  /// The command palette, from the search field at the right of the bar.
  final VoidCallback? onCommandPalette;

  /// Switching projects from the project menu: Open Project… and the
  /// recent ones (the current one left out).
  final List<RecentProject> recents;
  final VoidCallback? onOpenProject;
  final void Function(String dir)? onOpenRecent;

  /// The generated behaviour files' state, at the right of the bar: the
  /// CLI check, and the committed drift the session sees without it.
  final CodegenCheck? codegen;

  /// How many behaviour types the committed metadata and schema files
  /// disagree on — re-read whenever the project changes.
  final int Function()? codegenDrift;
  final VoidCallback? onCheckCodegen;
  final VoidCallback onMigrationPreview;

  /// "The saved layout could not be loaded; the default replaced it."
  final String? layoutNotice;
  final VoidCallback? onDismissNotice;

  /// The title bar's height: room for the traffic lights, and for a tab
  /// that is a control rather than a strip.
  static const titleBarHeight = 38.0;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    return Scaffold(
      body: Column(
        children: [
          _titleBar(context, theme),
          if (play != null)
            ListenableBuilder(
              listenable: play!,
              builder: (context, _) => !play!.runningOlderFiles
                  ? const SizedBox.shrink()
                  : MaterialBanner(
                      key: const Key('window.playEdited'),
                      leading: Icon(FIcons.editNote, size: t.icon, color: t.warning),
                      content: const Text(
                        'The project changed since Play started — the game is running the older files. '
                        'Changes apply on the next Play.',
                      ),
                      actions: [
                        TextButton(onPressed: onStop, child: const Text('Stop')),
                        TextButton(
                          key: const Key('window.playAgain'),
                          onPressed: play!.state == PlayState.stopping ? null : onReplay,
                          child: const Text('Play again'),
                        ),
                      ],
                    ),
            ),
          if (project.isReadOnly)
            MaterialBanner(
              key: const Key('window.readOnly'),
              leading: Icon(FIcons.lockOutline, size: t.icon, color: t.warning),
              content: Text('Read-only: ${project.readOnly}. Edits are refused until this editor is updated.'),
              actions: const [SizedBox.shrink()],
            ),
          ListenableBuilder(
            listenable: project,
            builder: (context, _) => project.outdatedDocuments == 0
                ? const SizedBox.shrink()
                : MaterialBanner(
                    key: const Key('window.migrations'),
                    leading: Icon(FIcons.upgrade, size: t.icon, color: t.warning),
                    content: Text(
                      '${project.outdatedDocuments} document${project.outdatedDocuments == 1 ? '' : 's'} use an older '
                      'format and cannot be validated or loaded until migrated.',
                    ),
                    actions: [TextButton(onPressed: onMigrationPreview, child: const Text('Preview migration…'))],
                  ),
          ),
          if (layoutNotice != null)
            MaterialBanner(
              key: const Key('window.layoutNotice'),
              leading: Icon(FIcons.dashboardCustomizeOutlined, size: t.icon, color: t.textSecondary),
              content: Text(layoutNotice!),
              actions: [TextButton(onPressed: onDismissNotice, child: const Text('OK'))],
            ),
          Expanded(
            child: TabbedViewTheme(
              data: fynkelTabbedViewTheme(t),
              child: MultiSplitViewTheme(
                data: fynkelSplitTheme(t),
                child: ListenableBuilder(
                  listenable: workspace,
                  builder: (context, _) => Docking(
                    key: ValueKey(workspace.layout),
                    layout: workspace.layout,
                    onItemClose: (_) => workspace.save(),
                    // Item 9e: no maximise buttons — a tab's menu and its
                    // double click maximise (`workspace.dart`).
                    maximizableItem: false,
                    maximizableTab: false,
                    maximizableTabsArea: false,
                  ),
                ),
              ),
            ),
          ),
          _statusBar(context, theme),
        ],
      ),
    );
  }

  Widget _titleBar(BuildContext context, ThemeData theme) {
    final t = context.tokens;
    final chrome = WindowChrome.instance;
    return Container(
      key: const Key('window.titleBar'),
      height: titleBarHeight,
      decoration: BoxDecoration(
        color: t.window,
        border: Border(bottom: t.hairline),
      ),
      child: Stack(
        children: [
          // Under everything: the bar's empty space is the window's.
          Positioned.fill(
            child: GestureDetector(
              key: const Key('window.drag'),
              behavior: HitTestBehavior.opaque,
              onPanStart: (_) => chrome.startDrag(),
              onDoubleTap: chrome.doubleClick,
            ),
          ),
          ListenableBuilder(
            listenable: Listenable.merge([chrome.ownsTitleBar, chrome.fullScreen]),
            builder: (context, _) => Row(
              children: [
                SizedBox(width: chrome.leadingInset + t.gap),
                // Left and right take equal shares, so Play sits in the
                // middle of the window whatever the tabs do.
                Expanded(
                  child: Row(
                    children: [
                      _projectMenu(context, theme),
                      if (project.isReadOnly) _readOnlyChip(context, theme),
                      Container(width: 1, height: 16, margin: const EdgeInsets.symmetric(horizontal: 6), color: t.line),
                      if (tabBar != null) Flexible(child: tabBar!),
                    ],
                  ),
                ),
                if (play != null)
                  Padding(padding: const EdgeInsets.symmetric(horizontal: 12), child: _playControls(context, theme)),
                Expanded(
                  child: Row(
                    mainAxisAlignment: MainAxisAlignment.end,
                    children: [
                      if (codegen != null) _codegenState(context, theme),
                      if (onCommandPalette != null) Flexible(child: _commandField(context, theme)),
                      SizedBox(width: t.pad),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  /// The project's name; the menu holds what the old toolbar spread out:
  /// where the project is, validate again, reveal, close.
  Widget _projectMenu(BuildContext context, ThemeData theme) {
    final t = context.tokens;
    return ListenableBuilder(
      listenable: project,
      builder: (context, _) => PopupMenuButton<String>(
        key: const Key('window.project'),
        popUpAnimationStyle: AnimationStyle.noAnimation,
        tooltip: project.dir,
        position: PopupMenuPosition.under,
        onSelected: (v) {
          switch (v) {
            case 'validate':
              onRevalidate();
            case 'reveal':
              onReveal?.call();
            case 'close':
              onClose();
            case 'open':
              onOpenProject?.call();
            case final dir when dir.startsWith('recent:'):
              onOpenRecent?.call(dir.substring('recent:'.length));
          }
        },
        itemBuilder: (context) {
          final others = [
            for (final r in recents)
              if (!p.equals(r.dir, project.dir)) r,
          ].take(5).toList();
          return [
            PopupMenuItem<String>(
              enabled: false,
              height: 28,
              child: Text(
                project.dir,
                style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
                overflow: TextOverflow.ellipsis,
              ),
            ),
            const PopupMenuDivider(),
            if (onOpenProject != null)
              const PopupMenuItem<String>(
                key: Key('window.openProject'),
                value: 'open',
                height: 32,
                child: Text('Open Project…'),
              ),
            for (final (i, r) in others.indexed)
              PopupMenuItem<String>(
                key: Key('window.recent.$i'),
                value: 'recent:${r.dir}',
                height: 32,
                enabled: onOpenRecent != null && r.exists,
                child: Tooltip(
                  message: r.exists ? r.dir : '${r.dir} (missing)',
                  waitDuration: const Duration(milliseconds: 600),
                  child: Row(
                    children: [
                      Icon(FIcons.videogameAsset, size: 13, color: t.textDisabled),
                      const SizedBox(width: 8),
                      Flexible(
                        child: Text(r.name.isEmpty ? p.basename(r.dir) : r.name, overflow: TextOverflow.ellipsis),
                      ),
                    ],
                  ),
                ),
              ),
            if (onOpenProject != null || others.isNotEmpty) const PopupMenuDivider(),
            PopupMenuItem<String>(
              key: const Key('window.revalidate'),
              value: 'validate',
              height: 32,
              enabled: !project.validating,
              child: const Text('Validate Again'),
            ),
            if (onReveal != null)
              const PopupMenuItem<String>(
                key: Key('window.reveal'),
                value: 'reveal',
                height: 32,
                child: Text('Reveal in Finder'),
              ),
            const PopupMenuDivider(),
            const PopupMenuItem<String>(
              key: Key('window.close'),
              value: 'close',
              height: 32,
              child: Text('Close Project'),
            ),
          ];
        },
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 4),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(FIcons.videogameAsset, size: 15, color: t.accent),
              const SizedBox(width: 7),
              ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 180),
                child: Text(
                  project.name,
                  key: const Key('window.projectName'),
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.bodyMedium?.copyWith(color: t.text, fontWeight: FontWeight.w600),
                ),
              ),
              const SizedBox(width: 4),
              if (project.validating)
                SizedBox(
                  width: 11,
                  height: 11,
                  child: CircularProgressIndicator(strokeWidth: 1.5, color: t.textSecondary),
                )
              else
                Icon(FIcons.expandMore, size: 14, color: t.textDisabled),
            ],
          ),
        ),
      ),
    );
  }

  Widget _readOnlyChip(BuildContext context, ThemeData theme) {
    final t = context.tokens;
    return Tooltip(
      message: project.readOnly!,
      child: Container(
        key: const Key('window.readOnlyChip'),
        margin: const EdgeInsets.only(left: 4),
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
        decoration: BoxDecoration(color: t.surfaceOf(t.warning, .16), borderRadius: BorderRadius.circular(4)),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(FIcons.lockOutline, size: 11, color: t.warning),
            const SizedBox(width: 4),
            Text('read-only', style: theme.textTheme.labelSmall?.copyWith(color: t.warning)),
          ],
        ),
      ),
    );
  }

  /// The generated behaviour files: quiet while they match the source, a
  /// word when they do not. A click checks again (`fynkel codegen --check`,
  /// which never writes); the fix is the command in the tooltip.
  Widget _codegenState(BuildContext context, ThemeData theme) {
    final t = context.tokens;
    final check = codegen!;
    return ListenableBuilder(
      listenable: Listenable.merge([check, project]),
      builder: (context, _) {
        final drift = codegenDrift?.call() ?? 0;
        final (IconData icon, Color color, String? label, String tip) = switch (check.status) {
          _ when drift > 0 => (
            FIcons.syncProblem,
            t.warning,
            'Run codegen',
            'The behaviour metadata and the schema files disagree ($drift type${drift == 1 ? '' : 's'}).\n'
                'Run: ${check.command}',
          ),
          CodegenStatus.running => (
            FIcons.refresh,
            t.textSecondary,
            check.generating ? 'Updating scripts…' : 'Checking code…',
            check.generating
                ? 'fynkel codegen is updating the behaviour metadata after a script changed'
                : 'fynkel codegen --check is running',
          ),
          CodegenStatus.upToDate => (
            FIcons.checkCircleOutline,
            t.textDisabled,
            null,
            check.watching
                ? 'Scripts are up to date; a saved script updates the Inspector in about a second.'
                : 'Generated behaviour files match the source. Click to check again.',
          ),
          CodegenStatus.stale => (
            FIcons.syncProblem,
            t.warning,
            'Run codegen',
            'Generated behaviour files are stale.\nRun: ${check.command}\nClick to check again.',
          ),
          CodegenStatus.failed when check.watching => (
            FIcons.errorOutline,
            t.error,
            'Script error',
            '${check.error ?? 'A script does not compile.'}\nPlay waits until it is fixed.',
          ),
          CodegenStatus.failed => (
            FIcons.errorOutline,
            t.error,
            'Codegen check failed',
            '${check.error ?? 'The check could not run.'}\nClick to try again.',
          ),
          CodegenStatus.unknown || CodegenStatus.notADartPackage => (FIcons.circle, t.textDisabled, null, ''),
        };
        if (tip.isEmpty) return const SizedBox.shrink();
        final running = check.status == CodegenStatus.running;
        return Padding(
          padding: const EdgeInsets.only(right: 6),
          child: Tooltip(
            message: tip,
            waitDuration: const Duration(milliseconds: 400),
            child: InkWell(
              key: const Key('window.codegen'),
              borderRadius: BorderRadius.circular(6),
              hoverColor: t.hover,
              onTap: running ? null : onCheckCodegen,
              child: Container(
                height: 26,
                padding: EdgeInsets.symmetric(horizontal: label == null ? 5 : 8),
                decoration: BoxDecoration(
                  color: label != null && !running ? t.surfaceOf(color, .14) : null,
                  borderRadius: BorderRadius.circular(6),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    if (running)
                      SizedBox(
                        width: 11,
                        height: 11,
                        child: CircularProgressIndicator(strokeWidth: 1.5, color: t.textSecondary),
                      )
                    else
                      Icon(icon, size: 14, color: color),
                    if (label != null) ...[
                      const SizedBox(width: 6),
                      Text(label, style: theme.textTheme.bodySmall?.copyWith(color: color)),
                    ],
                  ],
                ),
              ),
            ),
          ),
        );
      },
    );
  }

  /// "Search or run a command" — the palette, which finds commands,
  /// documents and assets alike.
  Widget _commandField(BuildContext context, ThemeData theme) {
    final t = context.tokens;
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 260),
      child: Material(
        color: t.field,
        borderRadius: BorderRadius.circular(6),
        child: InkWell(
          key: const Key('window.commands'),
          borderRadius: BorderRadius.circular(6),
          hoverColor: t.hover,
          onTap: onCommandPalette,
          child: Container(
            height: 26,
            padding: const EdgeInsets.only(left: 8, right: 6),
            decoration: BoxDecoration(
              borderRadius: BorderRadius.circular(6),
              border: Border.all(color: t.line),
            ),
            child: Row(
              children: [
                Icon(FIcons.search, size: 13, color: t.textDisabled),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    'Search or run a command',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
                  ),
                ),
                const SizedBox(width: 6),
                Text('⌘K', style: theme.textTheme.labelSmall?.copyWith(color: t.textDisabled, letterSpacing: .5)),
              ],
            ),
          ),
        ),
      ),
    );
  }

  /// ▶ / ■ and the run's state — starting, playing (with the scene), or
  /// the exit code the last run ended with — as one pill in the middle.
  Widget _playControls(BuildContext context, ThemeData theme) {
    final t = context.tokens;
    final session = play!;
    final run = gameRun;
    return ListenableBuilder(
      listenable: Listenable.merge([session, ?run]),
      builder: (context, _) {
        final active = session.isActive;
        final paused = run?.paused ?? false;
        final inEditor = run?.running ?? false;
        final busy = session.state == PlayState.starting || session.state == PlayState.stopping;
        return Container(
          height: 28,
          decoration: BoxDecoration(
            color: active ? t.surfaceOf(t.accent, .14) : t.field,
            borderRadius: BorderRadius.circular(7),
            border: Border.all(color: active ? t.surfaceOf(t.accent, .5) : t.line),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              IconButton(
                key: const Key('window.play'),
                tooltip: active ? 'Stop the game (⌘P)' : 'Play the project (⌘P)',
                padding: EdgeInsets.zero,
                constraints: const BoxConstraints.tightFor(width: 34, height: 26),
                iconSize: 15,
                icon: Icon(active ? FIcons.stop : FIcons.playArrow, color: active ? t.error : t.accent),
                onPressed: session.state == PlayState.stopping ? null : (active ? onStop : onPlay),
              ),
              if (run != null) ...[
                IconButton(
                  key: const Key('window.pause'),
                  tooltip: paused ? 'Resume (⇧⌘P)' : 'Pause (⇧⌘P)',
                  padding: EdgeInsets.zero,
                  constraints: const BoxConstraints.tightFor(width: 30, height: 26),
                  iconSize: 15,
                  isSelected: paused,
                  icon: Icon(FIcons.pause, color: paused ? t.accent : t.text),
                  onPressed: run.togglePause,
                ),
                IconButton(
                  key: const Key('window.step'),
                  tooltip: 'Step one frame (⌥⌘P)',
                  padding: EdgeInsets.zero,
                  constraints: const BoxConstraints.tightFor(width: 30, height: 26),
                  iconSize: 15,
                  icon: Icon(FIcons.skipForward, color: inEditor ? t.text : t.textDisabled),
                  onPressed: inEditor ? run.step : null,
                ),
              ],
              if (session.state != PlayState.idle) ...[
                Container(width: 1, height: 14, color: t.line),
                InkWell(
                  key: const Key('window.playState'),
                  onTap: onShowConsole,
                  borderRadius: const BorderRadius.horizontal(right: Radius.circular(7)),
                  child: Padding(
                    padding: const EdgeInsets.only(left: 8, right: 10),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        if (busy)
                          Padding(
                            padding: const EdgeInsets.only(right: 6),
                            child: SizedBox(
                              width: 10,
                              height: 10,
                              child: CircularProgressIndicator(strokeWidth: 1.5, color: t.textSecondary),
                            ),
                          )
                        else if (active)
                          Padding(
                            padding: const EdgeInsets.only(right: 6),
                            child: Icon(FIcons.circle, size: 7, color: paused ? t.textSecondary : t.ok),
                          ),
                        ConstrainedBox(
                          constraints: const BoxConstraints(maxWidth: 220),
                          child: Text(
                            paused ? 'Paused' : session.statusText,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: theme.textTheme.bodySmall?.copyWith(
                              color: session.crashed ? t.error : (active ? t.text : t.textSecondary),
                            ),
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ],
            ],
          ),
        );
      },
    );
  }

  Widget _statusBar(BuildContext context, ThemeData theme) {
    final t = context.tokens;
    final notices = Notices.maybeOf(context);
    return ListenableBuilder(
      listenable: project,
      builder: (context, _) {
        final errors = project.errorCount;
        final warnings = project.warningCount;
        return InkWell(
          key: const Key('window.status'),
          onTap: onShowConsole,
          child: Container(
            height: t.row + 2,
            padding: EdgeInsets.symmetric(horizontal: t.pad),
            color: t.window,
            child: Row(
              children: [
                Icon(FIcons.error, size: 13, color: errors > 0 ? t.error : t.textDisabled),
                const SizedBox(width: 4),
                Text('$errors', style: theme.textTheme.bodySmall),
                const SizedBox(width: 10),
                Icon(FIcons.warningAmber, size: 13, color: warnings > 0 ? t.warning : t.textDisabled),
                const SizedBox(width: 4),
                Text('$warnings', style: theme.textTheme.bodySmall),
                const SizedBox(width: 16),
                Expanded(
                  child: notices?.current != null
                      ? NoticeLine(notices: notices!)
                      : Text(
                          project.validating
                              ? 'Validating…'
                              : project.validationError ??
                                    '${project.documents} documents · ${project.assetCount} assets · ${project.references} references',
                          style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
                          overflow: TextOverflow.ellipsis,
                        ),
                ),
              ],
            ),
          ),
        );
      },
    );
  }
}
