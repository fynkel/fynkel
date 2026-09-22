// The main window: toolbar (project, read-only badge, Play / Stop and the
// run's state), the docking workspace, the status bar (diagnostic counts →
// Console).
import 'package:docking/docking.dart';
import 'package:flutter/material.dart';

import '../play/play_session.dart';
import '../project/editor_project.dart';
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
    required this.onMigrationPreview,
    this.play,
    this.onPlay,
    this.onReplay,
    this.onStop,
    this.layoutNotice,
    this.onDismissNotice,
  });

  final EditorProject project;
  final Workspace workspace;

  /// The document tabs, above the docking area.
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
  final VoidCallback onClose;
  final VoidCallback onRevalidate;
  final VoidCallback onMigrationPreview;

  /// "The saved layout could not be loaded; the default replaced it."
  final String? layoutNotice;
  final VoidCallback? onDismissNotice;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final dark = theme.brightness == Brightness.dark;
    return Scaffold(
      body: Column(
        children: [
          _toolbar(context, theme),
          if (play != null)
            ListenableBuilder(
              listenable: play!,
              builder: (context, _) => !play!.runningOlderFiles
                  ? const SizedBox.shrink()
                  : MaterialBanner(
                      key: const Key('window.playEdited'),
                      leading: const Icon(Icons.edit_note),
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
              leading: const Icon(Icons.lock_outline),
              content: Text('Read-only: ${project.readOnly}. Edits are refused until this editor is updated.'),
              actions: const [SizedBox.shrink()],
            ),
          ListenableBuilder(
            listenable: project,
            builder: (context, _) => project.outdatedDocuments == 0
                ? const SizedBox.shrink()
                : MaterialBanner(
                    key: const Key('window.migrations'),
                    leading: const Icon(Icons.upgrade),
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
              leading: const Icon(Icons.dashboard_customize_outlined),
              content: Text(layoutNotice!),
              actions: [TextButton(onPressed: onDismissNotice, child: const Text('OK'))],
            ),
          ?tabBar,
          Expanded(
            child: TabbedViewTheme(
              data: dark ? TabbedViewThemeData.dark() : TabbedViewThemeData.classic(),
              child: MultiSplitViewTheme(
                data: MultiSplitViewThemeData(
                  dividerThickness: 6,
                  dividerPainter: DividerPainters.background(color: theme.dividerColor),
                ),
                child: ListenableBuilder(
                  listenable: workspace,
                  builder: (context, _) => Docking(
                    key: ValueKey(workspace.layout),
                    layout: workspace.layout,
                    onItemClose: (_) => workspace.save(),
                    maximizableItem: true,
                    maximizableTab: true,
                    maximizableTabsArea: true,
                  ),
                ),
              ),
            ),
          ),
          const Divider(height: 1),
          _statusBar(theme),
        ],
      ),
    );
  }

  Widget _toolbar(BuildContext context, ThemeData theme) {
    return Container(
      height: 40,
      padding: const EdgeInsets.symmetric(horizontal: 12),
      color: theme.colorScheme.surfaceContainer,
      child: ListenableBuilder(
        listenable: project,
        builder: (context, _) => Row(
          children: [
            const Icon(Icons.videogame_asset, size: 18),
            const SizedBox(width: 8),
            Text(project.name, key: const Key('window.projectName'), style: theme.textTheme.titleSmall),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                project.dir,
                style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
                overflow: TextOverflow.ellipsis,
              ),
            ),
            if (project.isReadOnly)
              Tooltip(
                message: project.readOnly!,
                child: Chip(
                  key: const Key('window.readOnlyChip'),
                  avatar: const Icon(Icons.lock_outline, size: 14),
                  label: const Text('read-only'),
                  visualDensity: VisualDensity.compact,
                ),
              ),
            const SizedBox(width: 8),
            if (play != null) _playControls(theme),
            IconButton(
              tooltip: 'Validate the project again',
              icon: project.validating
                  ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
                  : const Icon(Icons.refresh, size: 18),
              onPressed: project.validating ? null : onRevalidate,
            ),
            IconButton(
              key: const Key('window.close'),
              tooltip: 'Close the project',
              icon: const Icon(Icons.logout, size: 18),
              onPressed: onClose,
            ),
          ],
        ),
      ),
    );
  }

  /// ▶ / ■ and the run's state — starting, playing (with the scene), or
  /// the exit code the last run ended with.
  Widget _playControls(ThemeData theme) {
    final session = play!;
    return ListenableBuilder(
      listenable: session,
      builder: (context, _) {
        final active = session.isActive;
        return Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            IconButton(
              key: const Key('window.play'),
              tooltip: active ? 'Stop the game (⌘.)' : 'Play the project (⌘P)',
              icon: Icon(
                active ? Icons.stop : Icons.play_arrow,
                size: 20,
                color: active ? theme.colorScheme.error : theme.colorScheme.primary,
              ),
              onPressed: session.state == PlayState.stopping ? null : (active ? onStop : onPlay),
            ),
            if (session.state != PlayState.idle)
              InkWell(
                key: const Key('window.playState'),
                onTap: onShowConsole,
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 4),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      if (session.state == PlayState.starting || session.state == PlayState.stopping)
                        const Padding(
                          padding: EdgeInsets.only(right: 6),
                          child: SizedBox(width: 12, height: 12, child: CircularProgressIndicator(strokeWidth: 2)),
                        ),
                      Text(
                        session.statusText,
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: session.crashed ? theme.colorScheme.error : theme.disabledColor,
                        ),
                      ),
                    ],
                  ),
                ),
              ),
          ],
        );
      },
    );
  }

  Widget _statusBar(ThemeData theme) {
    return ListenableBuilder(
      listenable: project,
      builder: (context, _) {
        final errors = project.errorCount;
        final warnings = project.warningCount;
        return InkWell(
          key: const Key('window.status'),
          onTap: onShowConsole,
          child: Container(
            height: 24,
            padding: const EdgeInsets.symmetric(horizontal: 10),
            child: Row(
              children: [
                Icon(Icons.error, size: 14, color: errors > 0 ? theme.colorScheme.error : theme.disabledColor),
                const SizedBox(width: 4),
                Text('$errors', style: theme.textTheme.bodySmall),
                const SizedBox(width: 10),
                Icon(Icons.warning_amber, size: 14, color: warnings > 0 ? Colors.orange : theme.disabledColor),
                const SizedBox(width: 4),
                Text('$warnings', style: theme.textTheme.bodySmall),
                const SizedBox(width: 16),
                Expanded(
                  child: Text(
                    project.validating
                        ? 'Validating…'
                        : project.validationError ??
                              '${project.documents} documents · ${project.assetCount} assets · ${project.references} references',
                    style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                Text('⇧⌘P commands', style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor)),
              ],
            ),
          ),
        );
      },
    );
  }
}
