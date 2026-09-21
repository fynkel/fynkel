// The command palette (⇧⌘P): every registered command, filtered as you type,
// arrows to move, Enter to run, Escape to close.
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'commands.dart';

Future<void> showCommandPalette(BuildContext context, CommandRegistry registry) {
  return showDialog<void>(
    context: context,
    barrierColor: Colors.black38,
    builder: (_) => CommandPalette(registry: registry),
  );
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

class CommandPalette extends StatefulWidget {
  const CommandPalette({super.key, required this.registry});

  final CommandRegistry registry;

  @override
  State<CommandPalette> createState() => _CommandPaletteState();
}

class _CommandPaletteState extends State<CommandPalette> {
  final _controller = TextEditingController();
  final _scroll = ScrollController();
  var _selected = 0;

  List<EditorCommand> get _matches => paletteMatches(widget.registry, _controller.text);

  @override
  void dispose() {
    _controller.dispose();
    _scroll.dispose();
    super.dispose();
  }

  void _move(int delta) {
    final n = _matches.length;
    if (n == 0) return;
    setState(() => _selected = (_selected + delta).clamp(0, n - 1));
    final offset = (_selected * _rowHeight) - 2 * _rowHeight;
    if (_scroll.hasClients) {
      _scroll.animateTo(
        offset.clamp(0, _scroll.position.maxScrollExtent),
        duration: const Duration(milliseconds: 80),
        curve: Curves.easeOut,
      );
    }
  }

  void _runSelected() {
    final m = _matches;
    if (_selected >= m.length) return;
    _run(m[_selected]);
  }

  void _run(EditorCommand c) {
    if (!c.isEnabled) return;
    Navigator.of(context).pop();
    // After the dialog is gone, so a command that opens another dialog can.
    WidgetsBinding.instance.addPostFrameCallback((_) => widget.registry.execute(c.id));
  }

  static const _rowHeight = 40.0;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final matches = _matches;
    if (_selected >= matches.length) _selected = matches.isEmpty ? 0 : matches.length - 1;
    return Dialog(
      alignment: const Alignment(0, -0.6),
      insetPadding: const EdgeInsets.all(24),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 640, maxHeight: 480),
        child: CallbackShortcuts(
          bindings: {
            const SingleActivator(LogicalKeyboardKey.arrowDown): () => _move(1),
            const SingleActivator(LogicalKeyboardKey.arrowUp): () => _move(-1),
            const SingleActivator(LogicalKeyboardKey.pageDown): () => _move(8),
            const SingleActivator(LogicalKeyboardKey.pageUp): () => _move(-8),
            const SingleActivator(LogicalKeyboardKey.enter): _runSelected,
            const SingleActivator(LogicalKeyboardKey.numpadEnter): _runSelected,
          },
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(12, 12, 12, 4),
                child: TextField(
                  key: const Key('palette.search'),
                  controller: _controller,
                  autofocus: true,
                  decoration: const InputDecoration(
                    prefixIcon: Icon(Icons.search),
                    hintText: 'Type a command…',
                    isDense: true,
                    border: OutlineInputBorder(),
                  ),
                  onChanged: (_) => setState(() => _selected = 0),
                ),
              ),
              Flexible(
                child: matches.isEmpty
                    ? const Padding(padding: EdgeInsets.all(24), child: Text('No matching command'))
                    : ListView.builder(
                        controller: _scroll,
                        shrinkWrap: true,
                        itemExtent: _rowHeight,
                        itemCount: matches.length,
                        itemBuilder: (context, i) {
                          final c = matches[i];
                          final selected = i == _selected;
                          final shortcut = c.shortcut;
                          return InkWell(
                            key: Key('palette.item.${c.id}'),
                            onTap: c.isEnabled ? () => _run(c) : null,
                            onHover: (h) {
                              if (h) setState(() => _selected = i);
                            },
                            child: Container(
                              color: selected ? theme.colorScheme.primaryContainer : null,
                              padding: const EdgeInsets.symmetric(horizontal: 16),
                              child: Row(
                                children: [
                                  Text(
                                    '${c.menu.title} › ',
                                    style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
                                  ),
                                  Expanded(
                                    child: Text(
                                      c.label,
                                      overflow: TextOverflow.ellipsis,
                                      style: c.isEnabled ? null : TextStyle(color: theme.disabledColor),
                                    ),
                                  ),
                                  if (shortcut != null)
                                    Container(
                                      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                                      decoration: BoxDecoration(
                                        border: Border.all(color: theme.dividerColor),
                                        borderRadius: BorderRadius.circular(4),
                                      ),
                                      child: Text(
                                        CommandRegistry.shortcutLabel(shortcut),
                                        style: theme.textTheme.bodySmall,
                                      ),
                                    ),
                                ],
                              ),
                            ),
                          );
                        },
                      ),
              ),
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 4, 16, 10),
                child: Row(
                  children: [
                    Text(
                      '${matches.length} of ${widget.registry.all.length} commands',
                      style: theme.textTheme.bodySmall,
                    ),
                    const Spacer(),
                    Text('↑↓ move · ↩ run · ⎋ close', style: theme.textTheme.bodySmall),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
