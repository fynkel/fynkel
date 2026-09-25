// The document tabs in the window's title bar (one tab per open document:
// kind icon, name, dirty dot or error badge, conflict badge, close × on the
// focused tab and under the pointer; middle click closes; right click for
// Close / Close Others / Save / Revert / Save As) and the dialogs the tabs need: the save prompt on close, the Open
// Document picker, Save As, and the conflict dialog with the revisions and
// the diff. Every document-changing call goes through the tab's
// `OpenDocument` — the dialogs never touch a file.
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path/path.dart' as p;

import '../picker/picker.dart';
import '../project/editor_project.dart';
import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'document_tabs.dart';
import 'line_diff.dart';
import 'open_document.dart';

IconData documentKindIcon(String kind) => switch (kind) {
  'scene' => FIcons.landscapeOutlined,
  'prefab' => FIcons.widgetsOutlined,
  'material' => FIcons.paletteOutlined,
  'rig' => FIcons.accessibilityNew,
  'blend' => FIcons.callSplit,
  'volume' => FIcons.tune,
  'physics_material' => FIcons.physicsMaterial,
  'data' => FIcons.dataAsset,
  _ => FIcons.descriptionOutlined,
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

  /// "Open Document…" (the + button); the picker opens under [anchor].
  final void Function(Rect anchor) onOpen;

  /// Close with the save prompt.
  final void Function(OpenDocument tab) onClose;
  final void Function(OpenDocument tab) onSaveAs;
  final void Function(OpenDocument tab) onShowConflict;
  final void Function(OpenDocument tab) onRevert;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    // Sits in the window's title bar (item 9b): the tabs take the width they
    // need, so the bar's empty space to their right still moves the window.
    return ListenableBuilder(
      listenable: tabs,
      builder: (context, _) => Row(
        key: const Key('tabs.bar'),
        children: [
          if (tabs.open.isEmpty)
            Flexible(
              child: Padding(
                padding: EdgeInsets.symmetric(horizontal: t.gap),
                child: Text(
                  'No document open — ⇧⌘O',
                  key: const Key('tabs.empty'),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
                ),
              ),
            )
          else
            Flexible(
              child: ScrollConfiguration(
                behavior: ScrollConfiguration.of(context).copyWith(scrollbars: false),
                child: ListView(
                  shrinkWrap: true,
                  scrollDirection: Axis.horizontal,
                  children: [
                    for (final o in tabs.open)
                      _Tab(
                        key: Key('tab.${o.document.id}'),
                        tab: o,
                        focused: tabs.focused == o,
                        onFocus: () => tabs.focus(o),
                        onClose: () => onClose(o),
                        onMenu: (at) => _menu(context, o, at),
                        onShowConflict: () => onShowConflict(o),
                      ),
                  ],
                ),
              ),
            ),
          Builder(
            builder: (button) => IconButton(
              key: const Key('tabs.open'),
              tooltip: 'Open Document… (⇧⌘O)',
              iconSize: 15,
              padding: EdgeInsets.zero,
              constraints: const BoxConstraints.tightFor(width: 26, height: 26),
              color: t.textSecondary,
              icon: const Icon(FIcons.add),
              onPressed: () => onOpen(_globalRect(button)),
            ),
          ),
        ],
      ),
    );
  }

  static Rect _globalRect(BuildContext context) {
    final box = context.findRenderObject() as RenderBox;
    return box.localToGlobal(Offset.zero) & box.size;
  }

  Future<void> _menu(BuildContext context, OpenDocument t, Offset at) async {
    final overlay = Overlay.of(context).context.findRenderObject() as RenderBox;
    final choice = await showMenu<String>(
      popUpAnimationStyle: AnimationStyle.noAnimation,
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

/// One tab: kind icon, name, the conflict or error badge, and at the end
/// the dirty dot or the close × (on the focused tab, and under the pointer).
class _Tab extends StatefulWidget {
  const _Tab({
    super.key,
    required this.tab,
    required this.focused,
    required this.onFocus,
    required this.onClose,
    required this.onMenu,
    required this.onShowConflict,
  });

  final OpenDocument tab;
  final bool focused;
  final VoidCallback onFocus;
  final VoidCallback onClose;
  final void Function(Offset at) onMenu;
  final VoidCallback onShowConflict;

  @override
  State<_Tab> createState() => _TabState();
}

class _TabState extends State<_Tab> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final k = context.tokens;
    final t = widget.tab;
    final focused = widget.focused;
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
        final id = t.document.id;
        final Widget end;
        if (t.dirty) {
          end = _DirtyOrClose(key: Key('tab.dirty.$id'), onClose: widget.onClose);
        } else if (focused || _hover) {
          end = IconButton(
            key: Key('tab.close.$id'),
            tooltip: 'Close (⌘W)',
            padding: EdgeInsets.zero,
            iconSize: 13,
            color: k.textSecondary,
            icon: const Icon(FIcons.close),
            onPressed: widget.onClose,
          );
        } else {
          end = const SizedBox.shrink();
        }
        return Listener(
          onPointerDown: (e) {
            if (e.buttons == kMiddleMouseButton) widget.onClose();
          },
          child: MouseRegion(
            onEnter: (_) => setState(() => _hover = true),
            onExit: (_) => setState(() => _hover = false),
            child: GestureDetector(
              excludeFromSemantics: true,
              onSecondaryTapUp: (d) => widget.onMenu(d.globalPosition),
              child: Tooltip(
                message: tooltip,
                waitDuration: const Duration(milliseconds: 600),
                child: Center(
                  child: Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 1),
                    child: Material(
                      type: MaterialType.transparency,
                      child: InkWell(
                        onTap: widget.onFocus,
                        borderRadius: BorderRadius.circular(6),
                        hoverColor: k.hover,
                        child: Container(
                          height: 28,
                          padding: const EdgeInsets.only(left: 10, right: 4),
                          decoration: BoxDecoration(
                            color: focused ? k.panel : null,
                            borderRadius: BorderRadius.circular(6),
                            border: Border.all(color: focused ? k.lineStrong : Colors.transparent),
                          ),
                          child: Row(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              Icon(documentKindIcon(t.kind), size: 13, color: focused ? k.accent : k.textDisabled),
                              const SizedBox(width: 6),
                              Text(
                                t.tabLabel,
                                style: theme.textTheme.bodySmall?.copyWith(
                                  color: focused ? k.text : k.textSecondary,
                                  fontWeight: focused ? FontWeight.w500 : null,
                                  fontStyle: t.document.readOnly != null ? FontStyle.italic : null,
                                ),
                              ),
                              if (t.hasConflict) ...[
                                const SizedBox(width: 4),
                                InkWell(
                                  key: Key('tab.conflict.$id'),
                                  onTap: widget.onShowConflict,
                                  child: Tooltip(
                                    message: 'This file changed on disk while you had unsaved edits. Click to resolve.',
                                    child: Icon(FIcons.warningAmberRounded, size: 14, color: k.warning),
                                  ),
                                ),
                              ] else if (errors > 0) ...[
                                const SizedBox(width: 5),
                                Container(
                                  key: Key('tab.errors.$id'),
                                  padding: const EdgeInsets.symmetric(horizontal: 5),
                                  decoration: BoxDecoration(
                                    color: k.surfaceOf(k.error, .3),
                                    borderRadius: BorderRadius.circular(7),
                                  ),
                                  child: Text('$errors', style: theme.textTheme.labelSmall?.copyWith(color: k.error)),
                                ),
                              ],
                              const SizedBox(width: 3),
                              SizedBox(width: 18, height: 18, child: end),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
        );
      },
    );
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
    final k = context.tokens;
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: _hover
          ? IconButton(
              tooltip: 'Close (⌘W) — asks about the unsaved edits',
              padding: EdgeInsets.zero,
              iconSize: 13,
              color: k.textSecondary,
              icon: const Icon(FIcons.close),
              onPressed: widget.onClose,
            )
          : Center(child: Icon(FIcons.circle, size: 8, color: k.text)),
    );
  }
}

// ── Dialogs ───────────────────────────────────────────────────────────────

enum ClosePromptChoice { save, discard, cancel }

/// "Save changes to X?" — Save / Don't Save / Cancel.
Future<ClosePromptChoice> showClosePrompt(BuildContext context, OpenDocument tab) async {
  final r = await showDialog<ClosePromptChoice>(
    animationStyle: AnimationStyle.noAnimation,
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

enum CreatePrefabChoice { original, variant }

/// Unity's question when a prefab instance is dragged to the Project: a new
/// original prefab (the instance unpacked one level into it) or a variant
/// of its prefab; null when cancelled.
Future<CreatePrefabChoice?> showCreatePrefabPrompt(BuildContext context) => showDialog<CreatePrefabChoice>(
  animationStyle: AnimationStyle.noAnimation,
  context: context,
  builder: (context) => AlertDialog(
    key: const Key('dialog.createPrefab'),
    title: const Text('Create Prefab or Variant?'),
    content: const Text('Would you like to create a new original Prefab or a variant of this Prefab?'),
    actions: [
      TextButton(
        key: const Key('createPrefab.cancel'),
        onPressed: () => Navigator.pop(context),
        child: const Text('Cancel'),
      ),
      TextButton(
        key: const Key('createPrefab.original'),
        onPressed: () => Navigator.pop(context, CreatePrefabChoice.original),
        child: const Text('Original Prefab'),
      ),
      FilledButton(
        key: const Key('createPrefab.variant'),
        onPressed: () => Navigator.pop(context, CreatePrefabChoice.variant),
        child: const Text('Prefab Variant'),
      ),
    ],
  ),
);

enum RevertChoice { revert, cancel }

Future<bool> showRevertPrompt(BuildContext context, OpenDocument tab) async {
  final r = await showDialog<RevertChoice>(
    animationStyle: AnimationStyle.noAnimation,
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

/// Open Document through the shared picker (item 9d): the project's scenes
/// and prefabs, grouped by kind, each with its folder.
Future<DocumentAsset?> showOpenDocumentDialog(BuildContext context, EditorProject project, {Rect? anchor}) async {
  final all = project.documentAssets().where((a) => a.kind == 'scene' || a.kind == 'prefab').toList()
    ..sort((a, b) {
      if (a.kind != b.kind) return a.kind == 'scene' ? -1 : 1;
      return a.path.toLowerCase().compareTo(b.path.toLowerCase());
    });
  final choice = await showPicker<DocumentAsset>(
    context,
    title: 'Open Document',
    anchor: anchor,
    hint: 'Search scenes and prefabs by name or path',
    keyPrefix: 'openDocument',
    countNoun: 'documents',
    empty: 'Nothing matches.',
    items: [
      for (final a in all)
        PickerItem(
          value: a,
          id: a.id,
          title: a.name.isEmpty ? p.basename(a.path) : a.name,
          detail: p.dirname(a.path) == '.' ? null : p.dirname(a.path),
          group: a.kind == 'scene' ? 'Scenes' : 'Prefabs',
          icon: documentKindIcon(a.kind),
          keywords: '${a.path} ${a.kind}',
          facts: [('Kind', a.kind), ('File', a.path)],
        ),
    ],
  );
  return choice?.value;
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
    animationStyle: AnimationStyle.noAnimation,
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
    animationStyle: AnimationStyle.noAnimation,
    context: context,
    builder: (context) {
      final theme = Theme.of(context);
      final mono = theme.textTheme.bodySmall?.copyWith(fontFamily: 'Menlo', fontFamilyFallback: const ['monospace']);
      return AlertDialog(
        key: const Key('dialog.conflict'),
        title: Row(
          children: [
            Icon(FIcons.warningAmberRounded, color: theme.tokens.warning),
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
                                  ? mono?.copyWith(color: theme.tokens.ok)
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
