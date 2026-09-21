// The main window: toolbar (project, read-only badge), the docking
// workspace, the status bar (diagnostic counts → Console).
import 'package:docking/docking.dart';
import 'package:flutter/material.dart';

import '../project/editor_project.dart';
import 'workspace.dart';

class MainWindow extends StatelessWidget {
  const MainWindow({
    super.key,
    required this.project,
    required this.workspace,
    required this.onShowConsole,
    required this.onClose,
    required this.onRevalidate,
    required this.onMigrationPreview,
    this.layoutNotice,
    this.onDismissNotice,
  });

  final EditorProject project;
  final Workspace workspace;
  final VoidCallback onShowConsole;
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
