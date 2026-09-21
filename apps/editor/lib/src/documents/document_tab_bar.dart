// The tab strip over the docking area (one tab per open document: kind
// icon, name, dirty dot or error badge, conflict badge, close ×; middle
// click closes; right click for Close / Close Others / Save / Revert / Save
// As) and the dialogs the tabs need: the save prompt on close, the Open
// Document picker, Save As, and the conflict dialog with the revisions and
// the diff. Every document-changing call goes through the tab's
// `OpenDocument` — the dialogs never touch a file.
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path/path.dart' as p;

import '../project/editor_project.dart';
import 'document_tabs.dart';
import 'line_diff.dart';
import 'open_document.dart';

IconData documentKindIcon(String kind) => switch (kind) {
  'scene' => Icons.landscape_outlined,
  'prefab' => Icons.widgets_outlined,
  'material' => Icons.palette_outlined,
  'rig' => Icons.accessibility_new,
  'blend' => Icons.call_split,
  _ => Icons.description_outlined,
};

class DocumentTabBar extends StatelessWidget {
  const DocumentTabBar({
    super.key,
    required this.tabs,
    required this.onOpen,
    required this.onClose,
    required this.onSaveAs,
    required this.onShowConflict,
    required this.onRevert,
  });

  final DocumentTabs tabs;

  /// "Open Document…" (the + button).
  final VoidCallback onOpen;

  /// Close with the save prompt.
  final void Function(OpenDocument tab) onClose;
  final void Function(OpenDocument tab) onSaveAs;
  final void Function(OpenDocument tab) onShowConflict;
  final void Function(OpenDocument tab) onRevert;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return ListenableBuilder(
      listenable: tabs,
      builder: (context, _) => Container(
        key: const Key('tabs.bar'),
        height: 32,
        color: theme.colorScheme.surfaceContainerHigh,
        child: Row(
          children: [
            Expanded(
              child: tabs.open.isEmpty
                  ? Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 12),
                      child: Align(
                        alignment: Alignment.centerLeft,
                        child: Text(
                          'No document open — ⇧⌘O opens a scene or prefab',
                          key: const Key('tabs.empty'),
                          style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
                        ),
                      ),
                    )
                  : ScrollConfiguration(
                      behavior: ScrollConfiguration.of(context).copyWith(scrollbars: false),
                      child: ListView(
                        scrollDirection: Axis.horizontal,
                        children: [for (final t in tabs.open) _tab(context, theme, t)],
                      ),
                    ),
            ),
            IconButton(
              key: const Key('tabs.open'),
              tooltip: 'Open Document… (⇧⌘O)',
              icon: const Icon(Icons.add, size: 18),
              onPressed: onOpen,
            ),
          ],
        ),
      ),
    );
  }

  Widget _tab(BuildContext context, ThemeData theme, OpenDocument t) {
    final focused = tabs.focused == t;
    return ListenableBuilder(
      listenable: t,
      builder: (context, _) {
        final errors = t.errorCount;
        final tooltip = [
          t.location,
          if (t.dirty) 'unsaved edits (${t.document.history} in history)',
          if (t.hasConflict) 'changed on disk — click the badge',
          if (errors > 0) '$errors error${errors == 1 ? '' : 's'}',
        ].join('\n');
        return Listener(
          onPointerDown: (e) {
            if (e.buttons == kMiddleMouseButton) onClose(t);
          },
          child: GestureDetector(
            onSecondaryTapUp: (d) => _menu(context, t, d.globalPosition),
            child: Tooltip(
              message: tooltip,
              waitDuration: const Duration(milliseconds: 600),
              child: InkWell(
                key: Key('tab.${t.document.id}'),
                onTap: () => tabs.focus(t),
                child: Container(
                  padding: const EdgeInsets.only(left: 10, right: 4),
                  decoration: BoxDecoration(
                    color: focused ? theme.colorScheme.surface : null,
                    border: Border(
                      bottom: BorderSide(color: focused ? theme.colorScheme.primary : Colors.transparent, width: 2),
                      right: BorderSide(color: theme.dividerColor),
                    ),
                  ),
                  child: Row(
                    children: [
                      Icon(documentKindIcon(t.kind), size: 14, color: focused ? null : theme.disabledColor),
                      const SizedBox(width: 6),
                      Text(
                        t.tabLabel,
                        style: theme.textTheme.bodySmall?.copyWith(
                          fontWeight: focused ? FontWeight.w600 : null,
                          fontStyle: t.document.readOnly != null ? FontStyle.italic : null,
                        ),
                      ),
                      if (t.hasConflict) ...[
                        const SizedBox(width: 4),
                        InkWell(
                          key: Key('tab.conflict.${t.document.id}'),
                          onTap: () => onShowConflict(t),
                          child: Tooltip(
                            message: 'This file changed on disk while you had unsaved edits. Click to resolve.',
                            child: Icon(Icons.warning_amber_rounded, size: 14, color: Colors.orange.shade700),
                          ),
                        ),
                      ] else if (errors > 0) ...[
                        const SizedBox(width: 4),
                        Container(
                          key: Key('tab.errors.${t.document.id}'),
                          padding: const EdgeInsets.symmetric(horizontal: 4),
                          decoration: BoxDecoration(
                            color: theme.colorScheme.errorContainer,
                            borderRadius: BorderRadius.circular(7),
                          ),
                          child: Text(
                            '$errors',
                            style: theme.textTheme.labelSmall?.copyWith(color: theme.colorScheme.onErrorContainer),
                          ),
                        ),
                      ],
                      const SizedBox(width: 2),
                      SizedBox(
                        width: 20,
                        height: 20,
                        child: t.dirty
                            ? _DirtyOrClose(key: Key('tab.dirty.${t.document.id}'), onClose: () => onClose(t))
                            : IconButton(
                                key: Key('tab.close.${t.document.id}'),
                                tooltip: 'Close (⌘W)',
                                padding: EdgeInsets.zero,
                                iconSize: 14,
                                icon: const Icon(Icons.close),
                                onPressed: () => onClose(t),
                              ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ),
        );
      },
    );
  }

  Future<void> _menu(BuildContext context, OpenDocument t, Offset at) async {
    final overlay = Overlay.of(context).context.findRenderObject() as RenderBox;
    final choice = await showMenu<String>(
      context: context,
      position: RelativeRect.fromRect(Rect.fromPoints(at, at), Offset.zero & overlay.size),
      items: [
        const PopupMenuItem(value: 'close', child: Text('Close')),
        PopupMenuItem(value: 'closeOthers', enabled: tabs.open.length > 1, child: const Text('Close Others')),
        const PopupMenuDivider(),
        PopupMenuItem(value: 'save', enabled: t.dirty, child: const Text('Save')),
        const PopupMenuItem(value: 'saveAs', child: Text('Save As…')),
        PopupMenuItem(value: 'revert', enabled: t.dirty, child: const Text('Revert to Saved')),
        const PopupMenuDivider(),
        PopupMenuItem(value: 'copyPath', child: Text('Copy Path (${t.location})')),
      ],
    );
    switch (choice) {
      case 'close':
        onClose(t);
      case 'closeOthers':
        for (final o in List.of(tabs.open)) {
          if (o != t) onClose(o);
        }
      case 'save':
        if (!t.save()) onShowConflict(t);
      case 'saveAs':
        onSaveAs(t);
      case 'revert':
        onRevert(t);
      case 'copyPath':
        await Clipboard.setData(ClipboardData(text: t.path));
      case null:
        break;
    }
  }
}

/// The dirty dot, which turns into the close × under the pointer.
class _DirtyOrClose extends StatefulWidget {
  const _DirtyOrClose({super.key, required this.onClose});

  final VoidCallback onClose;

  @override
  State<_DirtyOrClose> createState() => _DirtyOrCloseState();
}

class _DirtyOrCloseState extends State<_DirtyOrClose> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: _hover
          ? IconButton(
              tooltip: 'Close (⌘W) — asks about the unsaved edits',
              padding: EdgeInsets.zero,
              iconSize: 14,
              icon: const Icon(Icons.close),
              onPressed: widget.onClose,
            )
          : Center(child: Icon(Icons.circle, size: 9, color: theme.colorScheme.primary)),
    );
  }
}

// ── Dialogs ───────────────────────────────────────────────────────────────

enum ClosePromptChoice { save, discard, cancel }

/// "Save changes to X?" — Save / Don't Save / Cancel.
Future<ClosePromptChoice> showClosePrompt(BuildContext context, OpenDocument tab) async {
  final r = await showDialog<ClosePromptChoice>(
    context: context,
    builder: (context) => AlertDialog(
      key: const Key('dialog.closePrompt'),
      title: Text('Save changes to ${tab.title}?'),
      content: Text(
        '${tab.document.history} unsaved transaction${tab.document.history == 1 ? '' : 's'} in ${tab.location}.\n'
        'Your changes are lost if you don\'t save them.',
      ),
      actions: [
        TextButton(
          key: const Key('closePrompt.discard'),
          onPressed: () => Navigator.pop(context, ClosePromptChoice.discard),
          child: const Text("Don't Save"),
        ),
        TextButton(
          key: const Key('closePrompt.cancel'),
          onPressed: () => Navigator.pop(context, ClosePromptChoice.cancel),
          child: const Text('Cancel'),
        ),
        FilledButton(
          key: const Key('closePrompt.save'),
          onPressed: () => Navigator.pop(context, ClosePromptChoice.save),
          child: const Text('Save'),
        ),
      ],
    ),
  );
  return r ?? ClosePromptChoice.cancel;
}

enum RevertChoice { revert, cancel }

Future<bool> showRevertPrompt(BuildContext context, OpenDocument tab) async {
  final r = await showDialog<RevertChoice>(
    context: context,
    builder: (context) => AlertDialog(
      key: const Key('dialog.revert'),
      title: Text('Revert ${tab.title} to the saved file?'),
      content: Text(
        '${tab.document.history} transaction${tab.document.history == 1 ? '' : 's'} and the undo history are dropped; '
        'the tab shows what is on disk.',
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, RevertChoice.cancel), child: const Text('Cancel')),
        FilledButton(
          key: const Key('revert.confirm'),
          onPressed: () => Navigator.pop(context, RevertChoice.revert),
          child: const Text('Revert'),
        ),
      ],
    ),
  );
  return r == RevertChoice.revert;
}

/// Lists the project's scenes and prefabs; returns the chosen asset's path (absolute) or null.
Future<DocumentAsset?> showOpenDocumentDialog(BuildContext context, EditorProject project) {
  final all = project.documentAssets().where((a) => a.kind == 'scene' || a.kind == 'prefab').toList();
  return showDialog<DocumentAsset>(
    context: context,
    builder: (context) => _OpenDocumentDialog(assets: all, project: project),
  );
}

class _OpenDocumentDialog extends StatefulWidget {
  const _OpenDocumentDialog({required this.assets, required this.project});

  final List<DocumentAsset> assets;
  final EditorProject project;

  @override
  State<_OpenDocumentDialog> createState() => _OpenDocumentDialogState();
}

class _OpenDocumentDialogState extends State<_OpenDocumentDialog> {
  String _query = '';
  int _cursor = 0;

  List<DocumentAsset> get _visible {
    final q = _query.trim().toLowerCase();
    if (q.isEmpty) return widget.assets;
    return [
      for (final a in widget.assets)
        if (q.split(RegExp(r'\s+')).every('${a.name} ${a.path} ${a.kind}'.toLowerCase().contains)) a,
    ];
  }

  void _pick(DocumentAsset a) => Navigator.pop(context, a);

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final visible = _visible;
    if (_cursor >= visible.length) _cursor = visible.isEmpty ? 0 : visible.length - 1;
    return AlertDialog(
      key: const Key('dialog.openDocument'),
      title: const Text('Open Document'),
      content: SizedBox(
        width: 520,
        height: 400,
        child: Column(
          children: [
            CallbackShortcuts(
              bindings: {
                const SingleActivator(LogicalKeyboardKey.arrowDown): () =>
                    setState(() => _cursor = (_cursor + 1).clamp(0, visible.isEmpty ? 0 : visible.length - 1)),
                const SingleActivator(LogicalKeyboardKey.arrowUp): () =>
                    setState(() => _cursor = (_cursor - 1).clamp(0, 999999)),
                const SingleActivator(LogicalKeyboardKey.enter): () {
                  if (visible.isNotEmpty) _pick(visible[_cursor]);
                },
              },
              child: TextField(
                key: const Key('openDocument.search'),
                autofocus: true,
                decoration: const InputDecoration(
                  isDense: true,
                  prefixIcon: Icon(Icons.search, size: 18),
                  hintText: 'Search scenes and prefabs by name or path',
                  border: OutlineInputBorder(),
                ),
                onChanged: (v) => setState(() {
                  _query = v;
                  _cursor = 0;
                }),
              ),
            ),
            const SizedBox(height: 8),
            Expanded(
              child: visible.isEmpty
                  ? Center(
                      child: Text(
                        widget.assets.isEmpty ? 'This project has no scenes or prefabs.' : 'Nothing matches.',
                        style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
                      ),
                    )
                  : ListView.builder(
                      itemCount: visible.length,
                      itemExtent: 40,
                      itemBuilder: (context, i) {
                        final a = visible[i];
                        return ListTile(
                          key: Key('openDocument.${a.id}'),
                          dense: true,
                          selected: i == _cursor,
                          leading: Icon(documentKindIcon(a.kind), size: 18),
                          title: Text(a.name.isEmpty ? p.basename(a.path) : a.name),
                          subtitle: Text(a.path, style: theme.textTheme.bodySmall, overflow: TextOverflow.ellipsis),
                          trailing: Text(a.kind, style: theme.textTheme.labelSmall),
                          onTap: () => _pick(a),
                        );
                      },
                    ),
            ),
          ],
        ),
      ),
      actions: [TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel'))],
    );
  }
}

/// A relative path under the project for a copy; returns the absolute path or null.
Future<String?> showSaveAsDialog(BuildContext context, OpenDocument tab) {
  final ext = tab.kind == 'prefab' ? '.prefab.json' : '.scene.json';
  // Relative to the *project* directory — that is what the field says and what
  // the session resolves a save path against (`tab.location` is under `assets/`).
  final rel = p.isWithin(tab.project.dir, tab.path) ? p.relative(tab.path, from: tab.project.dir) : tab.location;
  final stem = rel.endsWith(ext) ? rel.substring(0, rel.length - ext.length) : p.withoutExtension(rel);
  final controller = TextEditingController(text: '$stem copy$ext');
  String? problem(String v) {
    final t = v.trim();
    if (t.isEmpty) return 'A path is needed.';
    if (p.isAbsolute(t) || t.contains('..')) return 'A path inside the project, please (relative, no "..").';
    if (!t.endsWith(ext)) return 'The name must end with $ext (this is a ${tab.kind}).';
    final abs = p.join(tab.project.dir, t);
    if (p.equals(abs, tab.path)) return 'That is the file itself — use Save.';
    if (tab.project.fileRevision(abs) != null) return 'A document already exists there; the editor does not overwrite.';
    return null;
  }

  return showDialog<String>(
    context: context,
    builder: (context) => StatefulBuilder(
      builder: (context, setState) {
        final err = problem(controller.text);
        return AlertDialog(
          key: const Key('dialog.saveAs'),
          title: Text('Save ${tab.title} As'),
          content: SizedBox(
            width: 480,
            child: TextField(
              key: const Key('saveAs.path'),
              controller: controller,
              autofocus: true,
              decoration: InputDecoration(
                labelText: 'Path in ${p.basename(tab.project.dir)}',
                errorText: err,
                border: const OutlineInputBorder(),
              ),
              onChanged: (_) => setState(() {}),
              onSubmitted: (v) {
                if (problem(v) == null) Navigator.pop(context, p.join(tab.project.dir, v.trim()));
              },
            ),
          ),
          actions: [
            TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
            FilledButton(
              key: const Key('saveAs.confirm'),
              onPressed: err == null
                  ? () => Navigator.pop(context, p.join(tab.project.dir, controller.text.trim()))
                  : null,
              child: const Text('Save Copy'),
            ),
          ],
        );
      },
    ),
  );
}

enum ConflictChoice { reload, saveAs, keep }

/// The file changed on disk under unsaved edits: the three revisions, the
/// history, and what changed on disk since the tab last agreed with it.
Future<ConflictChoice> showConflictDialog(BuildContext context, OpenDocument tab) async {
  final c = tab.conflict;
  if (c == null) return ConflictChoice.keep;
  final diff = c.baselineText != null && c.diskText != null
      ? lineDiff(c.baselineText!, c.diskText!)
      : const <DiffLine>[];
  final (added, removed) = diffCounts(diff);
  final text = unifiedDiff(diff);
  String short(String? r) => r == null ? '(unreadable)' : (r.length > 12 ? r.substring(0, 12) : r);
  final r = await showDialog<ConflictChoice>(
    context: context,
    builder: (context) {
      final theme = Theme.of(context);
      final mono = theme.textTheme.bodySmall?.copyWith(fontFamily: 'Menlo', fontFamilyFallback: const ['monospace']);
      return AlertDialog(
        key: const Key('dialog.conflict'),
        title: Row(
          children: [
            Icon(Icons.warning_amber_rounded, color: Colors.orange.shade700),
            const SizedBox(width: 8),
            Expanded(child: Text('${tab.title} changed on disk')),
          ],
        ),
        content: SizedBox(
          width: 640,
          height: 440,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                c.fileMissing
                    ? 'The file ${tab.location} is gone from disk. Your ${c.history} unsaved transaction'
                          '${c.history == 1 ? '' : 's'} are still in this tab; nothing was overwritten.'
                    : 'Something else wrote ${tab.location} while this tab had ${c.history} unsaved transaction'
                          '${c.history == 1 ? '' : 's'}. The editor never overwrites another writer\'s work (FYN-E0001): '
                          'save is refused until you choose.',
              ),
              const SizedBox(height: 10),
              Table(
                columnWidths: const {0: IntrinsicColumnWidth(), 1: FlexColumnWidth()},
                children: [
                  TableRow(
                    children: [
                      const Text('opened / last saved  '),
                      SelectableText(c.baseRevision, style: mono),
                    ],
                  ),
                  TableRow(
                    children: [
                      const Text('this tab now  '),
                      SelectableText(c.ourRevision, style: mono),
                    ],
                  ),
                  TableRow(
                    children: [
                      const Text('on disk now  '),
                      SelectableText(c.diskRevision ?? '(unreadable)', style: mono),
                    ],
                  ),
                ],
              ),
              const SizedBox(height: 10),
              Text(
                c.fileMissing
                    ? 'No diff: the file is missing.'
                    : 'Disk changes since ${short(c.baseRevision)}: +$added −$removed line${added + removed == 1 ? '' : 's'}',
                style: theme.textTheme.labelMedium,
              ),
              const SizedBox(height: 4),
              Expanded(
                child: Container(
                  width: double.infinity,
                  padding: const EdgeInsets.all(8),
                  decoration: BoxDecoration(
                    color: theme.colorScheme.surfaceContainerHighest,
                    borderRadius: BorderRadius.circular(4),
                  ),
                  child: SingleChildScrollView(
                    child: SelectableText.rich(
                      TextSpan(
                        children: [
                          for (final line in text.split('\n'))
                            TextSpan(
                              text: '$line\n',
                              style: line.startsWith('+')
                                  ? mono?.copyWith(color: Colors.green.shade700)
                                  : line.startsWith('-')
                                  ? mono?.copyWith(color: theme.colorScheme.error)
                                  : line.startsWith('@@')
                                  ? mono?.copyWith(color: theme.disabledColor)
                                  : mono,
                            ),
                        ],
                      ),
                    ),
                  ),
                ),
              ),
            ],
          ),
        ),
        actions: [
          TextButton(
            key: const Key('conflict.keep'),
            onPressed: () => Navigator.pop(context, ConflictChoice.keep),
            child: const Text('Keep Editing'),
          ),
          TextButton(
            key: const Key('conflict.saveAs'),
            onPressed: () => Navigator.pop(context, ConflictChoice.saveAs),
            child: const Text('Save As…'),
          ),
          FilledButton.tonal(
            key: const Key('conflict.reload'),
            onPressed: c.fileMissing ? null : () => Navigator.pop(context, ConflictChoice.reload),
            child: const Text('Reload from Disk (discard my edits)'),
          ),
        ],
      );
    },
  );
  return r ?? ConflictChoice.keep;
}
