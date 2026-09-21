// The Console: the project's diagnostics (ADR-0014 records) with severity,
// code, file, line:column or pointer, message; search, severity filter,
// collapsed repeats, copy, and a double-click (or Enter, or the button) that
// opens the file's line in the configured IDE.
import 'dart:async';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../project/editor_project.dart';

/// One row: a diagnostic and how many identical ones it stands for.
final class ConsoleEntry {
  ConsoleEntry(this.diagnostic, this.count);

  final Diagnostic diagnostic;
  final int count;

  /// The identity used to collapse repeats.
  static String keyOf(Diagnostic d) =>
      '${d.severity.name}|${d.code}|${d.document}|${d.pointer}|${d.line}|${d.column}|${d.message}';
}

/// Filters and selection; shared between the panel and the `console.*` commands.
final class ConsoleModel extends ChangeNotifier {
  ConsoleModel({required this.project, required this.openInIde});

  final EditorProject project;

  /// Opens a file at a position; returns a problem or null.
  final Future<String?> Function(String file, {int? line, int? column}) openInIde;

  String query = '';
  final severities = {Severity.error, Severity.warning, Severity.info};
  bool collapse = true;
  String? selectedKey;

  /// The last IDE launch's problem (cleared on success).
  String? lastIdeError;

  void setQuery(String q) {
    query = q;
    notifyListeners();
  }

  void toggleSeverity(Severity s) {
    if (!severities.remove(s)) severities.add(s);
    notifyListeners();
  }

  void setCollapse(bool v) {
    collapse = v;
    notifyListeners();
  }

  void select(String? key) {
    selectedKey = key;
    notifyListeners();
  }

  List<ConsoleEntry> get entries {
    final q = query.trim().toLowerCase();
    final counts = <String, int>{};
    final order = <String, Diagnostic>{};
    for (final d in project.diagnostics) {
      if (!severities.contains(d.severity)) continue;
      if (q.isNotEmpty && !_matches(d, q)) continue;
      final key = collapse ? ConsoleEntry.keyOf(d) : '${ConsoleEntry.keyOf(d)}#${counts.length}';
      counts[key] = (counts[key] ?? 0) + 1;
      order.putIfAbsent(key, () => d);
    }
    return [for (final e in order.entries) ConsoleEntry(e.value, counts[e.key]!)];
  }

  bool _matches(Diagnostic d, String q) {
    final hay = [
      d.code,
      d.message,
      d.document == null ? '' : project.relative(d.document!),
      d.pointer ?? '',
      d.id ?? '',
      d.suggestion ?? '',
    ].join(' ').toLowerCase();
    return q.split(RegExp(r'\s+')).every(hay.contains);
  }

  ConsoleEntry? get selected {
    final key = selectedKey;
    if (key == null) return null;
    for (final e in entries) {
      if (ConsoleEntry.keyOf(e.diagnostic) == key) return e;
    }
    return null;
  }

  int countOf(Severity s) => project.diagnostics.where((d) => d.severity == s).length;

  /// The position shown for a diagnostic: `scenes/main.scene.json:75:13`,
  /// or the pointer when the record has no line.
  String locationOf(Diagnostic d) {
    final doc = d.document == null ? '' : project.relative(d.document!);
    if (d.line != null) return '$doc:${d.line}${d.column != null ? ':${d.column}' : ''}';
    if (d.pointer != null && d.pointer!.isNotEmpty) return doc.isEmpty ? d.pointer! : '$doc  ${d.pointer}';
    return doc;
  }

  /// One text line per entry, as `fynkel validate` prints them.
  String lineOf(ConsoleEntry e) {
    final d = e.diagnostic;
    final where = locationOf(d);
    final repeat = e.count > 1 ? ' (×${e.count})' : '';
    final suggestion = d.suggestion == null ? '' : ' — ${d.suggestion}';
    return '${d.severity.name} ${d.code}: ${d.message}${where.isEmpty ? '' : ' ($where)'}$suggestion$repeat';
  }

  String textOf(Iterable<ConsoleEntry> es) => es.map(lineOf).join('\n');

  /// Copies the selection, or every visible entry, to the clipboard.
  Future<int> copy() async {
    final sel = selected;
    final es = sel != null ? [sel] : entries;
    if (es.isEmpty) return 0;
    await Clipboard.setData(ClipboardData(text: textOf(es)));
    return es.length;
  }

  bool get canOpenSelected => selected?.diagnostic.document != null;

  Future<void> openSelected() async {
    final d = selected?.diagnostic;
    if (d?.document == null) return;
    await open(d!);
  }

  Future<void> open(Diagnostic d) async {
    if (d.document == null) return;
    lastIdeError = await openInIde(d.document!, line: d.line, column: d.column);
    notifyListeners();
  }
}

class ConsolePanel extends StatefulWidget {
  const ConsolePanel({super.key, required this.model, this.searchFocus});

  final ConsoleModel model;

  /// The `console.focusSearch` command puts focus here.
  final FocusNode? searchFocus;

  @override
  State<ConsolePanel> createState() => _ConsolePanelState();
}

class _ConsolePanelState extends State<ConsolePanel> {
  final _search = TextEditingController();
  late final FocusNode _searchFocus = widget.searchFocus ?? FocusNode(debugLabel: 'console.search');
  final _listFocus = FocusNode(debugLabel: 'console.list');
  DateTime? _lastTap;
  String? _lastTapKey;

  ConsoleModel get model => widget.model;

  @override
  void initState() {
    super.initState();
    _search.text = model.query;
  }

  @override
  void dispose() {
    _search.dispose();
    if (widget.searchFocus == null) _searchFocus.dispose();
    _listFocus.dispose();
    super.dispose();
  }

  void _moveSelection(int delta) {
    final es = model.entries;
    if (es.isEmpty) return;
    final current = es.indexWhere((e) => ConsoleEntry.keyOf(e.diagnostic) == model.selectedKey);
    final next = (current < 0 ? (delta > 0 ? 0 : es.length - 1) : (current + delta).clamp(0, es.length - 1));
    model.select(ConsoleEntry.keyOf(es[next].diagnostic));
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: Listenable.merge([model, model.project]),
      builder: (context, _) {
        final theme = Theme.of(context);
        final entries = model.entries;
        final project = model.project;
        return Column(
          children: [
            _toolbar(context, theme),
            const Divider(height: 1),
            Expanded(
              child: entries.isEmpty
                  ? Center(
                      child: Text(
                        project.validating
                            ? 'Validating…'
                            : project.diagnostics.isEmpty
                            ? 'No diagnostics. The project validates clean.'
                            : 'Nothing matches the filter.',
                        style: theme.textTheme.bodyMedium?.copyWith(color: theme.disabledColor),
                      ),
                    )
                  : CallbackShortcuts(
                      bindings: {
                        const SingleActivator(LogicalKeyboardKey.arrowDown): () => _moveSelection(1),
                        const SingleActivator(LogicalKeyboardKey.arrowUp): () => _moveSelection(-1),
                        const SingleActivator(LogicalKeyboardKey.enter): model.openSelected,
                        const SingleActivator(LogicalKeyboardKey.keyC, meta: true): model.copy,
                      },
                      child: Focus(
                        focusNode: _listFocus,
                        child: ListView.builder(
                          itemCount: entries.length,
                          itemBuilder: (context, i) => _row(context, theme, entries[i]),
                        ),
                      ),
                    ),
            ),
            if (model.selected != null) ...[const Divider(height: 1), _detail(theme, model.selected!)],
            const Divider(height: 1),
            _status(theme, project),
          ],
        );
      },
    );
  }

  Widget _toolbar(BuildContext context, ThemeData theme) {
    final project = model.project;
    return Padding(
      padding: const EdgeInsets.fromLTRB(8, 6, 8, 6),
      child: Row(
        children: [
          Expanded(
            child: TextField(
              key: const Key('console.search'),
              controller: _search,
              focusNode: _searchFocus,
              decoration: InputDecoration(
                isDense: true,
                prefixIcon: const Icon(Icons.search, size: 18),
                hintText: 'Search code, message, file, pointer…',
                border: const OutlineInputBorder(),
                suffixIcon: model.query.isEmpty
                    ? null
                    : IconButton(
                        tooltip: 'Clear',
                        icon: const Icon(Icons.close, size: 16),
                        onPressed: () {
                          _search.clear();
                          model.setQuery('');
                        },
                      ),
              ),
              onChanged: model.setQuery,
            ),
          ),
          const SizedBox(width: 8),
          for (final s in Severity.values) ...[
            FilterChip(
              key: Key('console.filter.${s.name}'),
              avatar: Icon(_iconOf(s), size: 16, color: _colorOf(theme, s)),
              label: Text('${model.countOf(s)}'),
              tooltip: '${s.name}s',
              selected: model.severities.contains(s),
              onSelected: (_) => model.toggleSeverity(s),
              visualDensity: VisualDensity.compact,
            ),
            const SizedBox(width: 4),
          ],
          FilterChip(
            key: const Key('console.collapse'),
            label: const Text('Collapse'),
            tooltip: 'Collapse identical diagnostics into one row with a count',
            selected: model.collapse,
            onSelected: model.setCollapse,
            visualDensity: VisualDensity.compact,
          ),
          const SizedBox(width: 4),
          IconButton(
            key: const Key('console.copy'),
            tooltip: 'Copy the selected row, or every visible row',
            icon: const Icon(Icons.copy, size: 18),
            onPressed: model.entries.isEmpty ? null : model.copy,
          ),
          IconButton(
            key: const Key('console.open'),
            tooltip: 'Open in IDE (also: double-click or ↩)',
            icon: const Icon(Icons.open_in_new, size: 18),
            onPressed: model.canOpenSelected ? model.openSelected : null,
          ),
          IconButton(
            key: const Key('console.revalidate'),
            tooltip: 'Validate the project again',
            icon: project.validating
                ? const SizedBox(width: 18, height: 18, child: CircularProgressIndicator(strokeWidth: 2))
                : const Icon(Icons.refresh, size: 18),
            onPressed: project.validating ? null : project.revalidate,
          ),
        ],
      ),
    );
  }

  Widget _row(BuildContext context, ThemeData theme, ConsoleEntry e) {
    final d = e.diagnostic;
    final key = ConsoleEntry.keyOf(d);
    final selected = key == model.selectedKey;
    final mono = theme.textTheme.bodySmall?.copyWith(fontFamily: 'Menlo', fontFamilyFallback: const ['monospace']);
    return InkWell(
      key: Key('console.row.$key'),
      onTap: () {
        // A second click on the selected row within the double-click window
        // opens it; done by hand so the first click selects without delay.
        final now = DateTime.now();
        final isDouble = _lastTapKey == key && _lastTap != null && now.difference(_lastTap!) < kDoubleTapTimeout;
        _lastTap = now;
        _lastTapKey = key;
        model.select(key);
        _listFocus.requestFocus();
        if (isDouble && d.document != null) model.open(d);
      },
      child: Container(
        color: selected ? theme.colorScheme.primaryContainer : null,
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
        child: Row(
          children: [
            Icon(_iconOf(d.severity), size: 16, color: _colorOf(theme, d.severity)),
            const SizedBox(width: 6),
            SizedBox(width: 84, child: Text(d.code, style: mono)),
            Expanded(child: Text(d.message, overflow: TextOverflow.ellipsis)),
            if (e.count > 1)
              Container(
                margin: const EdgeInsets.only(left: 6),
                padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
                decoration: BoxDecoration(
                  color: theme.colorScheme.secondaryContainer,
                  borderRadius: BorderRadius.circular(8),
                ),
                child: Text('×${e.count}', style: theme.textTheme.labelSmall),
              ),
            const SizedBox(width: 12),
            ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 320),
              child: Text(
                model.locationOf(d),
                style: mono,
                overflow: TextOverflow.ellipsis,
                textAlign: TextAlign.right,
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _detail(ThemeData theme, ConsoleEntry e) {
    final d = e.diagnostic;
    final mono = theme.textTheme.bodySmall?.copyWith(fontFamily: 'Menlo', fontFamilyFallback: const ['monospace']);
    return Container(
      key: const Key('console.detail'),
      constraints: const BoxConstraints(maxHeight: 140),
      padding: const EdgeInsets.fromLTRB(12, 8, 12, 8),
      alignment: Alignment.topLeft,
      child: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SelectableText('${d.severity.name} ${d.code}: ${d.message}', style: theme.textTheme.bodyMedium),
            if (d.document != null) SelectableText(model.locationOf(d), style: mono),
            if (d.pointer != null && d.line != null) SelectableText(d.pointer!, style: mono),
            if (d.id != null) SelectableText('id ${d.id}', style: mono),
            if (d.suggestion != null) Text('Suggestion: ${d.suggestion}', style: theme.textTheme.bodySmall),
            for (final r in d.related) Text('related: ${r.code} ${r.message}', style: theme.textTheme.bodySmall),
            if (model.lastIdeError != null)
              Text(model.lastIdeError!, style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.error)),
          ],
        ),
      ),
    );
  }

  Widget _status(ThemeData theme, EditorProject project) {
    final when = project.validatedAt;
    final text = project.validating
        ? 'Validating…'
        : project.validationError != null
        ? 'Validation failed: ${project.validationError}'
        : '${project.errorCount} errors, ${project.warningCount} warnings · ${project.documents} documents, '
              '${project.sources} sources, ${project.references} references'
              '${when == null ? '' : ' · validated ${_time(when)}'}';
    return Padding(
      padding: const EdgeInsets.fromLTRB(10, 4, 10, 4),
      child: Row(
        children: [
          Expanded(
            child: Text(
              text,
              key: const Key('console.status'),
              style: theme.textTheme.bodySmall?.copyWith(
                color: project.validationError != null ? theme.colorScheme.error : theme.disabledColor,
              ),
              overflow: TextOverflow.ellipsis,
            ),
          ),
        ],
      ),
    );
  }

  static String _time(DateTime t) =>
      '${t.hour.toString().padLeft(2, '0')}:${t.minute.toString().padLeft(2, '0')}:${t.second.toString().padLeft(2, '0')}';

  static IconData _iconOf(Severity s) => switch (s) {
    Severity.error => Icons.error,
    Severity.warning => Icons.warning_amber,
    Severity.info => Icons.info_outline,
  };

  static Color _colorOf(ThemeData theme, Severity s) => switch (s) {
    Severity.error => theme.colorScheme.error,
    Severity.warning => Colors.orange,
    Severity.info => theme.colorScheme.primary,
  };
}
