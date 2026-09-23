// Tags and Layers (ADR-0020): the project's `layers` and `tags` edited as
// two lists — add, rename, reorder, remove — and applied as one asset
// operation (`setLayersAndTags`, undone with the Project browser's ⌥⌘Z).
// A rename or an addition changes the project file only; a reorder or a
// removal rewrites every scene and prefab that uses the entries, so the
// dialog says so before Apply. Layer 0 is always `default`.
import 'package:flutter/material.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'editor_project.dart';

/// One entry of a list being edited: its name and the old index it comes
/// from (null: added in this edit).
final class DraftEntry {
  DraftEntry(this.name, this.from);

  String name;
  final int? from;
}

/// A project list (layers or tags) being edited; what `setLayersAndTags`
/// takes is [names] and [from].
final class ListDraft {
  ListDraft(this.old, {required this.noun, this.fixedFirst = false, this.max = 32})
    : entries = [for (var i = 0; i < old.length; i++) DraftEntry(old[i], i)];

  final List<String> old;

  /// `layer` or `tag`, for messages.
  final String noun;

  /// The first entry (`default`) cannot be renamed, moved or removed.
  final bool fixedFirst;
  final int max;
  final List<DraftEntry> entries;

  List<String> get names => [for (final e in entries) e.name.trim()];
  List<int?> get from => [for (final e in entries) e.from];

  bool isFixed(int i) => fixedFirst && i == 0;
  bool get canAdd => entries.length < max;
  bool canMove(int i, int delta) {
    final j = i + delta;
    return !isFixed(i) && j >= (fixedFirst ? 1 : 0) && j < entries.length;
  }

  /// A new entry at the end, named `<noun> N` (unique).
  DraftEntry add() {
    var n = entries.length;
    String name() => '${noun[0].toUpperCase()}${noun.substring(1)} $n';
    while (names.contains(name())) {
      n++;
    }
    final e = DraftEntry(name(), null);
    entries.add(e);
    return e;
  }

  void rename(int i, String name) => entries[i].name = name;

  void move(int i, int delta) {
    if (!canMove(i, delta)) return;
    final e = entries.removeAt(i);
    entries.insert(i + delta, e);
  }

  void remove(int i) {
    if (!isFixed(i)) entries.removeAt(i);
  }

  /// Why the list cannot be applied, or null.
  String? get error {
    final seen = <String>{};
    for (final (i, n) in names.indexed) {
      if (n.isEmpty) return '${noun[0].toUpperCase()}${noun.substring(1)} $i has no name';
      if (!seen.add(n)) return '"$n" is there twice';
    }
    if (entries.length > max) return 'At most $max ${noun}s';
    return null;
  }

  /// Something differs from the project's list.
  bool get changed =>
      entries.length != old.length || entries.indexed.any((x) => x.$2.from != x.$1 || x.$2.name.trim() != old[x.$1]);

  /// An old entry moved or was removed: documents are rewritten.
  bool get rewrites => removed.isNotEmpty || entries.indexed.any((x) => x.$2.from != null && x.$2.from != x.$1);

  /// The old entries this edit removes.
  List<String> get removed {
    final kept = {for (final e in entries) ?e.from};
    return [
      for (var i = 0; i < old.length; i++)
        if (!kept.contains(i)) old[i],
    ];
  }
}

/// Which list the dialog opens on (both are shown; this one gets focus).
enum TagsAndLayersFocus { layers, tags }

Future<void> showTagsAndLayersDialog(
  BuildContext context,
  EditorProject project, {
  TagsAndLayersFocus focus = TagsAndLayersFocus.layers,
}) {
  return showDialog<void>(
    animationStyle: AnimationStyle.noAnimation,
    context: context,
    builder: (_) => TagsAndLayersDialog(project: project, focus: focus),
  );
}

class TagsAndLayersDialog extends StatefulWidget {
  const TagsAndLayersDialog({super.key, required this.project, this.focus = TagsAndLayersFocus.layers});

  final EditorProject project;
  final TagsAndLayersFocus focus;

  @override
  State<TagsAndLayersDialog> createState() => _TagsAndLayersDialogState();
}

class _TagsAndLayersDialogState extends State<TagsAndLayersDialog> {
  late final _layers = ListDraft(widget.project.layerNames(), noun: 'layer', fixedFirst: true);
  late final _tags = ListDraft(widget.project.tagNames(), noun: 'tag');
  String? _refusal;

  bool get _changed => _layers.changed || _tags.changed;
  String? get _error => _layers.error ?? _tags.error;

  void _apply() {
    try {
      widget.project.setLayersAndTags(
        layers: _layers.names,
        tags: _tags.names,
        layersFrom: _layers.from,
        tagsFrom: _tags.from,
      );
      Navigator.of(context).pop();
    } on AuthoringException catch (e) {
      setState(() => _refusal = e.diagnostics.isEmpty ? '$e' : e.diagnostics.first.message);
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    final error = _error;
    final removed = [..._layers.removed, ..._tags.removed];
    final note = error ?? _refusal;
    return AlertDialog(
      title: const Text('Tags and Layers'),
      content: SizedBox(
        width: 640,
        height: 420,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              'A layer is what an object is for physics, cameras and lights (one per object); '
              'tags are what it is for gameplay (any number).',
              style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary),
            ),
            const SizedBox(height: 8),
            Expanded(
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Expanded(
                    child: _ListEditor(
                      key: const Key('layers.list'),
                      title: 'Layers',
                      draft: _layers,
                      autofocus: widget.focus == TagsAndLayersFocus.layers,
                      onChanged: () => setState(() => _refusal = null),
                    ),
                  ),
                  const VerticalDivider(width: 17),
                  Expanded(
                    child: _ListEditor(
                      key: const Key('tags.list'),
                      title: 'Tags',
                      draft: _tags,
                      autofocus: widget.focus == TagsAndLayersFocus.tags,
                      onChanged: () => setState(() => _refusal = null),
                    ),
                  ),
                ],
              ),
            ),
            if (_layers.rewrites || _tags.rewrites)
              Padding(
                padding: const EdgeInsets.only(top: 6),
                child: Text(
                  removed.isEmpty
                      ? 'Moving entries rewrites the scenes and prefabs that use them, so every object keeps its '
                            'layer and tags. ⌥⌘Z (the asset undo) puts them back.'
                      : 'Removed: ${removed.join(', ')}. Objects on a removed layer go to default, a removed tag '
                            'is taken off, and masks lose it — every scene and prefab that uses them is rewritten.',
                  key: const Key('tagsLayers.rewrites'),
                  style: theme.textTheme.bodySmall?.copyWith(color: t.warning),
                ),
              ),
            if (note != null)
              Padding(
                padding: const EdgeInsets.only(top: 6),
                child: Text(
                  note,
                  key: const Key('tagsLayers.error'),
                  style: theme.textTheme.bodySmall?.copyWith(color: t.error),
                ),
              ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: const Text('Cancel')),
        FilledButton(
          key: const Key('tagsLayers.apply'),
          onPressed: _changed && error == null ? _apply : null,
          child: const Text('Apply'),
        ),
      ],
    );
  }
}

class _ListEditor extends StatefulWidget {
  const _ListEditor({
    super.key,
    required this.title,
    required this.draft,
    required this.onChanged,
    this.autofocus = false,
  });

  final String title;
  final ListDraft draft;
  final VoidCallback onChanged;
  final bool autofocus;

  @override
  State<_ListEditor> createState() => _ListEditorState();
}

class _ListEditorState extends State<_ListEditor> {
  final _controllers = <DraftEntry, TextEditingController>{};
  final _focus = <DraftEntry, FocusNode>{};

  TextEditingController _controller(DraftEntry e) =>
      _controllers.putIfAbsent(e, () => TextEditingController(text: e.name));
  FocusNode _node(DraftEntry e) => _focus.putIfAbsent(e, FocusNode.new);

  @override
  void dispose() {
    for (final c in _controllers.values) {
      c.dispose();
    }
    for (final f in _focus.values) {
      f.dispose();
    }
    super.dispose();
  }

  void _edit(void Function() change) {
    change();
    widget.onChanged();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    final draft = widget.draft;
    final noun = draft.noun;
    final keys = '${noun}s';
    final firstEditable = draft.fixedFirst ? 1 : 0;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(child: Text(widget.title, style: theme.textTheme.titleSmall)),
            Text(
              '${draft.entries.length} / ${draft.max}',
              style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
            ),
          ],
        ),
        const SizedBox(height: 4),
        Expanded(
          child: draft.entries.isEmpty
              ? Center(
                  child: Text('No ${noun}s yet', style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled)),
                )
              : ListView.builder(
                  itemCount: draft.entries.length,
                  itemBuilder: (context, i) {
                    final e = draft.entries[i];
                    final fixed = draft.isFixed(i);
                    return Padding(
                      key: ObjectKey(e),
                      padding: const EdgeInsets.symmetric(vertical: 1),
                      child: Row(
                        children: [
                          SizedBox(
                            width: 24,
                            child: Text(
                              '$i',
                              textAlign: TextAlign.right,
                              style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
                            ),
                          ),
                          const SizedBox(width: 6),
                          Expanded(
                            child: TextField(
                              key: Key('$keys.name.$i'),
                              controller: _controller(e),
                              focusNode: _node(e),
                              autofocus: widget.autofocus && i == firstEditable,
                              enabled: !fixed,
                              style: theme.textTheme.bodySmall?.copyWith(color: fixed ? t.textDisabled : t.text),
                              decoration: InputDecoration(
                                isDense: true,
                                contentPadding: const EdgeInsets.symmetric(horizontal: 6, vertical: 6),
                                suffixText: e.from == null ? 'new' : null,
                              ),
                              onChanged: (v) => _edit(() => draft.rename(i, v)),
                            ),
                          ),
                          _RowButton(
                            key: Key('$keys.up.$i'),
                            tooltip: 'Move up',
                            icon: FIcons.arrowUpward,
                            onPressed: draft.canMove(i, -1) ? () => _edit(() => draft.move(i, -1)) : null,
                          ),
                          _RowButton(
                            key: Key('$keys.down.$i'),
                            tooltip: 'Move down',
                            icon: FIcons.arrowDownward,
                            onPressed: draft.canMove(i, 1) ? () => _edit(() => draft.move(i, 1)) : null,
                          ),
                          _RowButton(
                            key: Key('$keys.remove.$i'),
                            tooltip: fixed ? 'default is always layer 0' : 'Remove',
                            icon: FIcons.deleteOutline,
                            onPressed: fixed ? null : () => _edit(() => draft.remove(i)),
                          ),
                        ],
                      ),
                    );
                  },
                ),
        ),
        Align(
          alignment: Alignment.centerLeft,
          child: TextButton.icon(
            key: Key('$keys.add'),
            onPressed: draft.canAdd
                ? () {
                    late DraftEntry added;
                    _edit(() => added = draft.add());
                    WidgetsBinding.instance.addPostFrameCallback((_) {
                      final c = _controller(added);
                      c.selection = TextSelection(baseOffset: 0, extentOffset: c.text.length);
                      _node(added).requestFocus();
                    });
                  }
                : null,
            icon: const Icon(FIcons.add, size: 14),
            label: Text('Add $noun'),
          ),
        ),
      ],
    );
  }
}

class _RowButton extends StatelessWidget {
  const _RowButton({super.key, required this.tooltip, required this.icon, this.onPressed});

  final String tooltip;
  final IconData icon;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    return IconButton(
      tooltip: tooltip,
      icon: Icon(icon, size: 13),
      padding: EdgeInsets.zero,
      constraints: const BoxConstraints(minWidth: 24, minHeight: 24),
      onPressed: onPressed,
    );
  }
}
