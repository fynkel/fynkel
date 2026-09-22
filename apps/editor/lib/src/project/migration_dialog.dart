// The migration preview: what `fynkel migrate` would do to the documents
// behind their kind's formatVersion — steps and diff per document, read
// from the session. The editor writes nothing; the command to run is shown.
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../theme/icons.dart';
import 'editor_project.dart';

Future<void> showMigrationPreview(BuildContext context, EditorProject project) {
  return showDialog<void>(
    animationStyle: AnimationStyle.noAnimation,
    context: context,
    builder: (_) => MigrationPreviewDialog(project: project),
  );
}

class MigrationPreviewDialog extends StatefulWidget {
  const MigrationPreviewDialog({super.key, required this.project});

  final EditorProject project;

  @override
  State<MigrationPreviewDialog> createState() => _MigrationPreviewDialogState();
}

class _MigrationPreviewDialogState extends State<MigrationPreviewDialog> {
  int _selected = 0;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final mono = theme.textTheme.bodySmall?.copyWith(fontFamily: 'Menlo', fontFamilyFallback: const ['monospace']);
    return ListenableBuilder(
      listenable: widget.project,
      builder: (context, _) {
        final preview = widget.project.migrations;
        final docs = preview?.documents ?? const <DocumentMigration>[];
        if (_selected >= docs.length) _selected = 0;
        final doc = docs.isEmpty ? null : docs[_selected];
        final command = 'fynkel migrate ${widget.project.dir}';
        return AlertDialog(
          title: const Text('Migration preview'),
          content: SizedBox(
            width: 860,
            height: 520,
            child: preview == null
                ? const Center(child: Text('Validating…'))
                : docs.isEmpty
                ? Center(
                    child: Text(
                      'Every document is at its current formatVersion.\n'
                      'This build knows ${preview.steps.length} migration step${preview.steps.length == 1 ? '' : 's'}.',
                      key: const Key('migrations.none'),
                      textAlign: TextAlign.center,
                    ),
                  )
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '${docs.length} document${docs.length == 1 ? '' : 's'} use an older format. Nothing is changed '
                        'until you run the command below; originals are kept as <file>.bak.',
                        key: const Key('migrations.summary'),
                      ),
                      const SizedBox(height: 8),
                      Expanded(
                        child: Row(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            SizedBox(
                              width: 300,
                              child: ListView.builder(
                                itemCount: docs.length,
                                itemBuilder: (context, i) {
                                  final d = docs[i];
                                  return ListTile(
                                    key: Key('migrations.doc.${d.path}'),
                                    dense: true,
                                    selected: i == _selected,
                                    leading: Icon(
                                      d.ok ? FIcons.upgrade : FIcons.errorOutline,
                                      color: d.ok ? null : theme.colorScheme.error,
                                    ),
                                    title: Text(widget.project.relative(d.path), overflow: TextOverflow.ellipsis),
                                    subtitle: Text('${d.kind} v${d.from} → v${d.to}'),
                                    onTap: () => setState(() => _selected = i),
                                  );
                                },
                              ),
                            ),
                            const VerticalDivider(width: 1),
                            Expanded(
                              child: doc == null
                                  ? const SizedBox.shrink()
                                  : Padding(
                                      padding: const EdgeInsets.only(left: 12),
                                      child: Column(
                                        crossAxisAlignment: CrossAxisAlignment.start,
                                        children: [
                                          Text('Steps', style: theme.textTheme.titleSmall),
                                          for (final s in doc.applied)
                                            Text(
                                              '• v${s.from} → v${s.to}  ${s.id}${s.description.isEmpty ? '' : ': ${s.description}'}',
                                            ),
                                          for (final d in doc.diagnostics)
                                            Text(
                                              '${d.severity.name} ${d.code}: ${d.message}',
                                              style: TextStyle(color: theme.colorScheme.error),
                                            ),
                                          const SizedBox(height: 8),
                                          Text('Diff', style: theme.textTheme.titleSmall),
                                          Expanded(
                                            child: Container(
                                              width: double.infinity,
                                              padding: const EdgeInsets.all(8),
                                              decoration: BoxDecoration(
                                                color: theme.colorScheme.surfaceContainerHighest,
                                                borderRadius: BorderRadius.circular(4),
                                              ),
                                              child: SingleChildScrollView(
                                                child: SelectableText(
                                                  doc.diff.isEmpty ? '(no textual change)' : doc.diff,
                                                  key: const Key('migrations.diff'),
                                                  style: mono,
                                                ),
                                              ),
                                            ),
                                          ),
                                        ],
                                      ),
                                    ),
                            ),
                          ],
                        ),
                      ),
                      const SizedBox(height: 8),
                      Row(
                        children: [
                          Expanded(
                            child: SelectableText(command, key: const Key('migrations.command'), style: mono),
                          ),
                          TextButton.icon(
                            key: const Key('migrations.copy'),
                            onPressed: () => Clipboard.setData(ClipboardData(text: command)),
                            icon: const Icon(FIcons.copy, size: 16),
                            label: const Text('Copy command'),
                          ),
                        ],
                      ),
                    ],
                  ),
          ),
          actions: [
            TextButton(
              onPressed: widget.project.validating ? null : widget.project.revalidate,
              child: const Text('Re-check'),
            ),
            FilledButton(onPressed: () => Navigator.of(context).pop(), child: const Text('Close')),
          ],
        );
      },
    );
  }
}
