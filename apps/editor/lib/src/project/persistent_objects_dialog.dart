// Project › Persistent Objects… (ADR-0039; Godot's autoloads): prefabs the
// game makes once when the project loads, before the startup scene, and
// keeps across every scene load — a manager or a music player that is
// there whichever scene Play starts in. Each row is a name (what
// `world.persistentObject(name)` finds) and a prefab; they are made in
// list order. Applied as one asset operation (`setPersistentObjects`,
// undone with the Project browser's ⌥⌘Z).
import 'package:flutter/material.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../inspector/field_widgets.dart';
import '../inspector/material_inspector.dart' show describeAssetRef;
import '../theme/tokens.dart';
import 'editor_project.dart';

Future<void> showPersistentObjectsDialog(BuildContext context, EditorProject project) {
  return showDialog<void>(
    animationStyle: AnimationStyle.noAnimation,
    context: context,
    builder: (_) => PersistentObjectsDialog(project: project),
  );
}

class PersistentObjectsDialog extends StatefulWidget {
  const PersistentObjectsDialog({super.key, required this.project});

  final EditorProject project;

  @override
  State<PersistentObjectsDialog> createState() => _PersistentObjectsDialogState();
}

class _Row {
  _Row(String name, this.prefab) : name = TextEditingController(text: name);

  final TextEditingController name;
  Map<String, Object?>? prefab;

  /// Still the name the dialog gave it: picking a prefab renames it.
  bool named = false;
}

class _PersistentObjectsDialogState extends State<PersistentObjectsDialog> {
  late final List<PersistentObjectEntry> _old = widget.project.persistentObjects();
  late final List<_Row> _rows = [for (final o in _old) _Row(o.name, o.prefab)..named = true];
  String? _refusal;

  @override
  void dispose() {
    for (final r in _rows) {
      r.name.dispose();
    }
    super.dispose();
  }

  List<PersistentObjectEntry> get _draft => [
    for (final r in _rows) PersistentObjectEntry(name: r.name.text, prefab: r.prefab),
  ];

  /// What is wrong with row [i], or null.
  String? _problem(int i) {
    final name = _rows[i].name.text;
    if (!PersistentObjectEntry.identifier.hasMatch(name)) {
      return 'The name must start with a letter, then letters, digits or _.';
    }
    if (_rows.take(i).any((r) => r.name.text == name)) return '"$name" is used twice.';
    if (_rows[i].prefab == null) return 'Choose a prefab for "$name".';
    return null;
  }

  /// [base], or `base2`, `base3`… when another row has it.
  String _unique(String base, _Row except) {
    bool used(String n) => _rows.any((r) => r != except && r.name.text == n);
    if (!used(base)) return base;
    var n = 2;
    while (used('$base$n')) {
      n++;
    }
    return '$base$n';
  }

  void _changed() => setState(() => _refusal = null);

  void _add() {
    final row = _Row('', null);
    row.name.text = _unique('Object', row);
    _rows.add(row);
    _changed();
  }

  void _pick(_Row row, Map<String, Object?>? prefab) {
    row.prefab = prefab;
    if (!row.named && prefab != null) {
      final name = describeAssetRef(widget.project, {'asset': prefab['asset']});
      row.name.text = _unique(PersistentObjectEntry.nameFrom(name), row);
    }
    _changed();
  }

  void _move(int from, int to) {
    _rows.insert(to, _rows.removeAt(from));
    _changed();
  }

  void _remove(int i) {
    _rows.removeAt(i).name.dispose();
    _changed();
  }

  void _apply() {
    try {
      widget.project.setPersistentObjects(_draft);
      Navigator.of(context).pop();
    } on AuthoringException catch (e) {
      setState(() => _refusal = e.diagnostics.isEmpty ? '$e' : e.diagnostics.first.message);
    }
  }

  Widget _row(int i) {
    final row = _rows[i];
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        children: [
          SizedBox(
            width: 170,
            child: TextField(
              key: Key('persistentObjects.name.$i'),
              controller: row.name,
              decoration: const InputDecoration(isDense: true, hintText: 'Name'),
              onChanged: (_) {
                row.named = true;
                _changed();
              },
            ),
          ),
          const SizedBox(width: 8),
          Expanded(
            child: AssetField(
              key: Key('persistentObjects.prefab.$i'),
              value: row.prefab,
              kind: 'prefab',
              subject: 'Prefab',
              describe: (ref) => describeAssetRef(widget.project, ref),
              candidates: () => [
                for (final a in widget.project.assets())
                  if (a.offeredKinds.contains('prefab')) a,
              ],
              onCommit: (ref) => _pick(row, ref),
            ),
          ),
          IconButton(
            key: Key('persistentObjects.up.$i'),
            tooltip: 'Move up',
            visualDensity: VisualDensity.compact,
            icon: const Icon(Icons.arrow_upward, size: 16),
            onPressed: i > 0 ? () => _move(i, i - 1) : null,
          ),
          IconButton(
            key: Key('persistentObjects.down.$i'),
            tooltip: 'Move down',
            visualDensity: VisualDensity.compact,
            icon: const Icon(Icons.arrow_downward, size: 16),
            onPressed: i < _rows.length - 1 ? () => _move(i, i + 1) : null,
          ),
          IconButton(
            key: Key('persistentObjects.remove.$i'),
            tooltip: 'Remove',
            visualDensity: VisualDensity.compact,
            icon: const Icon(Icons.remove_circle_outline, size: 16),
            onPressed: () => _remove(i),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    final problem = [for (var i = 0; i < _rows.length; i++) _problem(i)].nonNulls.firstOrNull;
    final note = problem ?? _refusal;
    final changed = !_listEquals(_draft, _old);
    return AlertDialog(
      title: const Text('Persistent Objects'),
      content: SizedBox(
        width: 620,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                'Prefabs the game makes once when it starts, before the first scene, and keeps across every '
                'scene load — whichever scene Play starts in. Scripts find one with '
                'world.persistentObject(name). They are made in this order.',
                style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary),
              ),
              const SizedBox(height: 8),
              if (_rows.isEmpty)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 8),
                  child: Text(
                    'No persistent objects.',
                    key: const Key('persistentObjects.empty'),
                    style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary),
                  ),
                ),
              for (var i = 0; i < _rows.length; i++) _row(i),
              Align(
                alignment: Alignment.centerLeft,
                child: TextButton.icon(
                  key: const Key('persistentObjects.add'),
                  onPressed: _add,
                  icon: const Icon(Icons.add, size: 16),
                  label: const Text('Add'),
                ),
              ),
              if (note != null)
                Padding(
                  padding: const EdgeInsets.only(top: 6),
                  child: Text(
                    note,
                    key: const Key('persistentObjects.error'),
                    style: theme.textTheme.bodySmall?.copyWith(color: t.error),
                  ),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: const Text('Cancel')),
        FilledButton(
          key: const Key('persistentObjects.apply'),
          onPressed: changed && problem == null ? _apply : null,
          child: const Text('Apply'),
        ),
      ],
    );
  }
}

bool _listEquals(List<PersistentObjectEntry> a, List<PersistentObjectEntry> b) {
  if (a.length != b.length) return false;
  for (var i = 0; i < a.length; i++) {
    if (a[i] != b[i]) return false;
  }
  return true;
}
