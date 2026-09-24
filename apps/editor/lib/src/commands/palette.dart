// The command palette (⌘K), on the shared picker (item 9d): every
// registered command grouped by menu; typing ranks them into one list.
// Arrows move, Enter runs, Escape closes; a disabled command is listed dim
// and does not run.
import 'package:flutter/material.dart';

import '../picker/picker.dart';
import 'commands.dart';

Future<void> showCommandPalette(BuildContext context, CommandRegistry registry) async {
  final all = registry.all;
  // Ties rank alphabetically, the way [paletteMatches] orders them.
  final byLabel = [...all]..sort((a, b) => a.baseLabel.compareTo(b.baseLabel));
  final alphabetical = {for (final (i, c) in byLabel.indexed) c.id: i};
  final choice = await showPicker<EditorCommand>(
    context,
    title: 'Commands',
    hint: 'Type a command…',
    keyPrefix: 'palette',
    countNoun: 'commands',
    empty: 'No matching command',
    score: (item, query) {
      final s = paletteScore(item.value, query);
      return s == 0 ? 0 : s * 10000 + (9999 - (alphabetical[item.value.id] ?? 0));
    },
    items: [
      for (final c in all)
        PickerItem(
          value: c,
          id: 'item.${c.id}',
          title: c.label,
          group: c.menu.title,
          enabled: c.isEnabled,
          shortcut: switch (c.shortcut ?? c.key) {
            final s? => CommandRegistry.shortcutLabel(s),
            null => null,
          },
          keywords: '${c.baseLabel} ${c.id}',
          facts: [
            ('Menu', c.menu.title),
            if (c.shortcut != null) ('Shortcut', CommandRegistry.shortcutLabel(c.shortcut!)),
            if (c.key != null) ('Key', '${CommandRegistry.shortcutLabel(c.key!)} (any panel but a text field)'),
            if (!c.isEnabled) ('State', 'not available now'),
          ],
        ),
    ],
  );
  final c = choice?.value;
  if (c == null || !c.isEnabled) return;
  // After the picker is gone, so a command that opens another dialog can.
  WidgetsBinding.instance.addPostFrameCallback((_) => registry.execute(c.id));
}

/// Ranks [command] against [query]: 0 = no match; higher is better. Both
/// the live label ("Hide Console") and the fixed one ("Toggle Console") count.
int paletteScore(EditorCommand command, String query) {
  final q = query.trim().toLowerCase();
  if (q.isEmpty) return 1;
  final labels = {command.label.toLowerCase(), command.baseLabel.toLowerCase()};
  var best = 0;
  for (final label in labels) {
    final s = _scoreLabel(command, label, q);
    if (s > best) best = s;
  }
  return best;
}

int _scoreLabel(EditorCommand command, String label, String q) {
  final id = command.id.toLowerCase();
  final menu = command.menu.title.toLowerCase();
  if (label == q) return 100;
  if (label.startsWith(q)) return 90;
  if (label.split(RegExp(r'\s+')).any((w) => w.startsWith(q))) return 80;
  if (label.contains(q)) return 70;
  if (id.contains(q) || '$menu $label'.contains(q)) return 60;
  // Subsequence: every character of q appears in order in the label.
  var i = 0;
  for (final ch in label.split('')) {
    if (i < q.length && ch == q[i]) i++;
  }
  return i == q.length ? 40 : 0;
}

List<EditorCommand> paletteMatches(CommandRegistry registry, String query) {
  final scored = [
    for (final c in registry.all)
      if (paletteScore(c, query) > 0) (c, paletteScore(c, query)),
  ];
  scored.sort((a, b) => b.$2 != a.$2 ? b.$2.compareTo(a.$2) : a.$1.baseLabel.compareTo(b.$1.baseLabel));
  return [for (final s in scored) s.$1];
}
