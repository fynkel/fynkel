// The Console: the project's diagnostics (ADR-0014 records) with severity,
// code, file, line:column or pointer, message; search, severity filter,
// collapsed repeats, copy, and a double-click (or Enter) that opens a scene
// or prefab diagnostic in the editor (its entity selected) and any other
// file's line in the configured IDE; both also have a button and a command.
import 'dart:async';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../play/game_console.dart';
import '../play/game_console_panel.dart';
import '../project/editor_project.dart';
import '../theme/icons.dart';
import '../theme/panel_focus.dart';
import '../theme/theme.dart';
import '../theme/tokens.dart';

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
  ConsoleModel({required this.project, required this.openInIde, this.openInEditor});

  final EditorProject project;

  /// Opens a file at a position; returns a problem or null.
  final Future<String?> Function(String file, {int? line, int? column}) openInIde;

  /// Opens a scene or prefab diagnostic into a document tab (item 3).
  final void Function(Diagnostic d)? openInEditor;

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

  /// Whether a diagnostic names a document the editor opens in a tab.
  static bool isEditable(Diagnostic d) {
    final doc = d.document;
    return doc != null && (doc.endsWith('.scene.json') || doc.endsWith('.prefab.json'));
  }

  bool get canOpenSelectedInEditor {
    final d = selected?.diagnostic;
    return openInEditor != null && d != null && isEditable(d);
  }

  void openSelectedInEditor() {
    final d = selected?.diagnostic;
    if (d != null && isEditable(d)) openInEditor?.call(d);
  }

  /// Double-click / ↩: the editor for a scene or prefab, the IDE otherwise.
  Future<void> activate(Diagnostic d) async {
    if (openInEditor != null && isEditable(d)) {
      openInEditor!(d);
    } else {
      await open(d);
    }
  }

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

/// The Console panel: the project's diagnostics, and — once a project is
/// open — the running game's records beside them (item 8). Two tabs, one
/// panel: the counts are on the tabs, so a Play that errors is visible
/// while the Project tab is in front.
class ConsolePanel extends StatefulWidget {
  const ConsolePanel({
    super.key,
    required this.model,
    this.searchFocus,
    this.tab,
    this.game,
    this.gameSearchFocus,
    this.onPlay,
    this.onStop,
  });

  /// The tabs, by index.
  static const projectTab = 0;
  static const gameTab = 1;

  final ConsoleModel model;

  /// The `console.focusSearch` command puts focus here.
  final FocusNode? searchFocus;

  /// Which tab is in front; the shell writes it (Play shows the Game tab)
  /// and a tap on a tab writes back.
  final ValueNotifier<int>? tab;

  /// The Game tab; null in a window without a project (and in the tests
  /// that only drive the project's diagnostics).
  final GameConsoleModel? game;
  final FocusNode? gameSearchFocus;
  final VoidCallback? onPlay;
  final VoidCallback? onStop;

  @override
  State<ConsolePanel> createState() => _ConsolePanelState();
}

class _ConsolePanelState extends State<ConsolePanel> with SingleTickerProviderStateMixin {
  final _search = TextEditingController();
  late final FocusNode _searchFocus = widget.searchFocus ?? FocusNode(debugLabel: 'console.search');
  final _listFocus = FocusNode(debugLabel: 'console.list');
  DateTime? _lastTap;
  String? _lastTapKey;

  ConsoleModel get model => widget.model;

  TabController? _tabs;

  @override
  void initState() {
    super.initState();
    _search.text = model.query;
    if (widget.game != null) _install();
    widget.tab?.addListener(_showRequestedTab);
  }

  void _install() {
    _tabs =
        TabController(length: 2, vsync: this, initialIndex: widget.tab?.value ?? 0, animationDuration: Duration.zero)
          ..addListener(() {
            if (!_tabs!.indexIsChanging) widget.tab?.value = _tabs!.index;
          });
  }

  void _showRequestedTab() {
    final wanted = widget.tab?.value;
    final tabs = _tabs;
    if (wanted == null || tabs == null || tabs.index == wanted) return;
    tabs.animateTo(wanted);
  }

  @override
  void didUpdateWidget(ConsolePanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.tab != widget.tab) {
      oldWidget.tab?.removeListener(_showRequestedTab);
      widget.tab?.addListener(_showRequestedTab);
    }
    if ((oldWidget.game == null) != (widget.game == null)) {
      _tabs?.dispose();
      _tabs = null;
      if (widget.game != null) _install();
    }
  }

  @override
  void dispose() {
    widget.tab?.removeListener(_showRequestedTab);
    _tabs?.dispose();
    _search.dispose();
    if (widget.searchFocus == null) _searchFocus.dispose();
    _listFocus.dispose();
    super.dispose();
  }

  /// `console.showGame` puts the Game tab in front (and `console.*` the
  /// project's, so a command always lands where it can be seen).
  void showTab(int index) => _tabs?.animateTo(index);

  void _moveSelection(int delta) {
    final es = model.entries;
    if (es.isEmpty) return;
    final current = es.indexWhere((e) => ConsoleEntry.keyOf(e.diagnostic) == model.selectedKey);
    final next = (current < 0 ? (delta > 0 ? 0 : es.length - 1) : (current + delta).clamp(0, es.length - 1));
    model.select(ConsoleEntry.keyOf(es[next].diagnostic));
  }

  @override
  Widget build(BuildContext context) {
    final game = widget.game;
    final tabs = _tabs;
    if (game == null || tabs == null) return _project(context);
    return ListenableBuilder(
      listenable: Listenable.merge([model, model.project, game.session]),
      builder: (context, _) {
        final theme = Theme.of(context);
        final t = context.tokens;
        final errors = model.project.errorCount;
        final gameErrors = game.session.errorCount;
        return Column(
          children: [
            ColoredBox(
              color: t.header,
              child: TabBar(
                key: const Key('console.tabs'),
                controller: tabs,
                labelStyle: theme.textTheme.labelLarge,
                tabAlignment: TabAlignment.start,
                isScrollable: true,
                tabs: [
                  Tab(
                    key: const Key('console.tab.project'),
                    height: t.tab,
                    text: 'Project${errors > 0 ? ' ($errors)' : ''}',
                  ),
                  Tab(
                    key: const Key('console.tab.game'),
                    height: t.tab,
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        if (game.session.isActive)
                          Padding(
                            padding: const EdgeInsets.only(right: 6),
                            child: Icon(FIcons.playArrow, size: 13, color: t.accent),
                          ),
                        Text('Game${gameErrors > 0 ? ' ($gameErrors)' : ''}'),
                      ],
                    ),
                  ),
                ],
              ),
            ),
            Expanded(
              child: TabBarView(
                controller: tabs,
                children: [
                  _project(context),
                  GameConsoleView(
                    model: game,
                    searchFocus: widget.gameSearchFocus,
                    onPlay: widget.onPlay ?? () {},
                    onStop: widget.onStop ?? () {},
                  ),
                ],
              ),
            ),
          ],
        );
      },
    );
  }

  Widget _project(BuildContext context) {
    return ListenableBuilder(
      listenable: Listenable.merge([model, model.project]),
      builder: (context, _) {
        final theme = Theme.of(context);
        final entries = model.entries;
        final project = model.project;
        return LayoutBuilder(
          builder: (context, box) => Column(
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
                          const SingleActivator(LogicalKeyboardKey.enter): () {
                            final d = model.selected?.diagnostic;
                            if (d != null) model.activate(d);
                          },
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
              if (model.selected != null) ...[
                const Divider(height: 1),
                _detail(theme, model.selected!, box.maxHeight * 0.4),
              ],
              const Divider(height: 1),
              _status(theme, project),
            ],
          ),
        );
      },
    );
  }

  Widget _toolbar(BuildContext context, ThemeData theme) {
    final project = model.project;
    final t = context.tokens;
    return Container(
      decoration: BoxDecoration(
        color: t.header,
        border: Border(bottom: t.hairline),
      ),
      padding: const EdgeInsets.fromLTRB(4, 3, 4, 3),
      child: Row(
        children: [
          Expanded(
            child: TextField(
              key: const Key('console.search'),
              controller: _search,
              focusNode: _searchFocus,
              style: theme.textTheme.bodySmall?.copyWith(color: t.text),
              decoration: InputDecoration(
                prefixIcon: Icon(FIcons.search, size: 13, color: t.textDisabled),
                prefixIconConstraints: BoxConstraints(minWidth: t.row, minHeight: t.row),
                hintText: 'Search code, message, file, pointer…',
                suffixIcon: model.query.isEmpty
                    ? null
                    : IconButton(
                        tooltip: 'Clear',
                        iconSize: 12,
                        icon: const Icon(FIcons.close),
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
              avatar: Icon(_iconOf(s), size: 13, color: _colorOf(theme, s)),
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
            icon: const Icon(FIcons.copy),
            onPressed: model.entries.isEmpty ? null : model.copy,
          ),
          IconButton(
            key: const Key('console.openInEditor'),
            tooltip: 'Open in Editor — the scene or prefab into a tab, its entity selected (⌥⌘E)',
            icon: const Icon(FIcons.accountTreeOutlined),
            onPressed: model.canOpenSelectedInEditor ? model.openSelectedInEditor : null,
          ),
          IconButton(
            key: const Key('console.open'),
            tooltip: 'Open in IDE (⌥⌘O)',
            icon: const Icon(FIcons.openInNew),
            onPressed: model.canOpenSelected ? model.openSelected : null,
          ),
          IconButton(
            key: const Key('console.revalidate'),
            tooltip: 'Validate the project again',
            icon: project.validating
                ? SizedBox(width: t.icon, height: t.icon, child: const CircularProgressIndicator(strokeWidth: 2))
                : const Icon(FIcons.refresh),
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
    final t = context.tokens;
    final focused = PanelFocus.of(context);
    final mono = monoStyle(context, color: selected ? t.onSelectedRowSecondary(focused: focused) : null);
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
        if (isDouble && d.document != null) model.activate(d);
      },
      child: Container(
        height: context.tokens.row + 2,
        alignment: Alignment.centerLeft,
        color: selected ? t.selectedRow(focused: focused) : null,
        padding: EdgeInsets.symmetric(horizontal: context.tokens.pad),
        child: Row(
          children: [
            Icon(_iconOf(d.severity), size: 13, color: _colorOf(theme, d.severity)),
            const SizedBox(width: 6),
            SizedBox(width: 78, child: Text(d.code, style: mono)),
            Expanded(
              child: Text(
                d.message,
                style: theme.textTheme.bodySmall?.copyWith(
                  color: selected ? t.onSelectedRow(focused: focused) : t.text,
                ),
                overflow: TextOverflow.ellipsis,
              ),
            ),
            if (e.count > 1)
              Container(
                margin: const EdgeInsets.only(left: 6),
                padding: const EdgeInsets.symmetric(horizontal: 5),
                decoration: BoxDecoration(color: context.tokens.lineStrong, borderRadius: context.tokens.cornerControl),
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

  Widget _detail(ThemeData theme, ConsoleEntry e, double maxHeight) {
    final d = e.diagnostic;
    final mono = monoStyle(context);
    return Container(
      key: const Key('console.detail'),
      constraints: BoxConstraints(maxHeight: maxHeight.clamp(40.0, 140.0)),
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
    return Container(
      height: theme.tokens.row + 2,
      alignment: Alignment.centerLeft,
      padding: EdgeInsets.symmetric(horizontal: theme.tokens.pad),
      decoration: BoxDecoration(
        color: theme.tokens.header,
        border: Border(top: theme.tokens.hairline),
      ),
      child: Row(
        children: [
          Expanded(
            child: Text(
              text,
              key: const Key('console.status'),
              style: theme.textTheme.bodySmall?.copyWith(
                color: project.validationError != null ? theme.tokens.error : theme.tokens.textSecondary,
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
    Severity.error => FIcons.error,
    Severity.warning => FIcons.warningAmber,
    Severity.info => FIcons.infoOutline,
  };

  static Color _colorOf(ThemeData theme, Severity s) => switch (s) {
    Severity.error => theme.tokens.error,
    Severity.warning => theme.tokens.warning,
    Severity.info => theme.tokens.info,
  };
}
