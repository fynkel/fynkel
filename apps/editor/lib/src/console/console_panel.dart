// The Console: the project's diagnostics (ADR-0014 records) with severity,
// code, file, line:column or pointer, message; search, severity filter,
// collapsed repeats, copy, and a double-click (or Enter) that opens a scene
// or prefab diagnostic in the editor (its entity selected) and any other
// file's line in the configured IDE; both also have a button and a command.
// A project from an older build folds what follows from that — documents
// behind their format, what they report, the lock naming another build —
// under one strip with the way out, instead of a wall of errors.
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
  ConsoleModel({required this.project, required this.openInIde, this.openInEditor, this.extra});

  final EditorProject project;

  /// Rows that are not the project's validation: a failed hot reload's
  /// compile errors (listed first). [refresh] when they change.
  final List<Diagnostic> Function()? extra;

  /// What the Console lists: [extra], then the project's diagnostics.
  List<Diagnostic> get diagnostics => [...?extra?.call(), ...project.diagnostics];

  /// [extra] changed.
  void refresh() => notifyListeners();

  /// Opens a file at a position; returns a problem or null.
  final Future<String?> Function(String file, {int? line, int? column}) openInIde;

  /// Opens a scene or prefab diagnostic into a document tab (item 3).
  final void Function(Diagnostic d)? openInEditor;

  String query = '';
  final severities = {Severity.error, Severity.warning, Severity.info};
  bool collapse = true;

  /// Whether the diagnostics that follow from an older build are listed
  /// (off: folded under the upgrade strip).
  bool showUpgrade = false;

  static const outdatedCode = 'FYN-D0016';
  static const lockCode = 'FYN-P0011';
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

  void toggleUpgrade() {
    showUpgrade = !showUpgrade;
    notifyListeners();
  }

  /// The documents behind their kind's format (relative paths).
  Set<String> get outdatedDocuments => {
    for (final d in project.migrations?.documents ?? const <DocumentMigration>[]) project.relative(d.path),
    for (final d in project.diagnostics)
      if (d.code == outdatedCode && d.document != null) project.relative(d.document!),
  };

  /// Whether [d] follows from the project being from an older build: a
  /// document behind its format (and anything it reports — it is checked
  /// against the current schema), or the lock naming another build.
  bool isFromUpgrade(Diagnostic d, [Set<String>? outdated]) {
    if (d.code == outdatedCode || d.code == lockCode) return true;
    final doc = d.document;
    return doc != null && (outdated ?? outdatedDocuments).contains(project.relative(doc));
  }

  /// The diagnostics folded under the upgrade strip.
  int get upgradeCount {
    final outdated = outdatedDocuments;
    return project.diagnostics.where((d) => isFromUpgrade(d, outdated)).length;
  }

  /// The lock names another build (`fynkel project lock` rewrites it).
  bool get lockDiffers => project.diagnostics.any((d) => d.code == lockCode);

  void select(String? key) {
    selectedKey = key;
    notifyListeners();
  }

  List<ConsoleEntry> get entries {
    final q = query.trim().toLowerCase();
    final counts = <String, int>{};
    final order = <String, Diagnostic>{};
    final outdated = showUpgrade ? const <String>{} : outdatedDocuments;
    for (final d in diagnostics) {
      if (!severities.contains(d.severity)) continue;
      if (!showUpgrade && isFromUpgrade(d, outdated)) continue;
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

  int countOf(Severity s) => diagnostics.where((d) => d.severity == s).length;

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
    this.onMigrationPreview,
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

  /// The upgrade strip's "Preview migration…".
  final VoidCallback? onMigrationPreview;

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
    PanelFocus.primary(context, _listFocus);
    final game = widget.game;
    final tabs = _tabs;
    if (game == null || tabs == null) return _project(context);
    // One list at a time, as Unity's: the toolbar's Project ▾ / Game ▾
    // picks the source (docs/ui-refresh); Play puts the Game in front.
    return ListenableBuilder(
      listenable: Listenable.merge([tabs, game.session]),
      // Only the picked list is built, so its rows take the panel's focus.
      builder: (context, _) => tabs.index == ConsolePanel.projectTab
          ? _project(context)
          : GameConsoleView(
              model: game,
              searchFocus: widget.gameSearchFocus,
              onPlay: widget.onPlay ?? () {},
              onStop: widget.onStop ?? () {},
              source: _source(context, game: true),
            ),
    );
  }

  /// The source picker at the toolbar's left; null without a Game source.
  Widget? _source(BuildContext context, {required bool game}) {
    final tabs = _tabs;
    final g = widget.game;
    if (tabs == null || g == null) return null;
    return ConsoleSourceButton(
      game: game,
      playing: g.session.isActive,
      projectErrors: model.project.errorCount,
      gameErrors: g.session.errorCount,
      onPick: (i) => tabs.index = i,
    );
  }

  Widget _project(BuildContext context) {
    return ListenableBuilder(
      listenable: Listenable.merge([model, model.project]),
      builder: (context, _) {
        final theme = Theme.of(context);
        final entries = model.entries;
        final project = model.project;
        final upgrade = model.upgradeCount;
        return LayoutBuilder(
          builder: (context, box) => Column(
            children: [
              _toolbar(context, theme),
              if (upgrade > 0) _upgradeStrip(context, theme, upgrade),
              Expanded(
                child: entries.isEmpty
                    ? Center(
                        child: Text(
                          project.validating
                              ? 'Validating…'
                              : model.diagnostics.isEmpty
                              ? 'No diagnostics. The project validates clean.'
                              : upgrade == model.diagnostics.length && !model.showUpgrade
                              ? 'Nothing else to report.'
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
              // Unity has no footer: this one only says validation is
              // running or failed (docs/ui-refresh).
              if (project.validating || project.validationError != null) ...[
                const Divider(height: 1),
                _status(theme, project),
              ],
            ],
          ),
        );
      },
    );
  }

  /// One strip for everything an older build explains, with the way out.
  Widget _upgradeStrip(BuildContext context, ThemeData theme, int count) {
    final t = context.tokens;
    final docs = model.outdatedDocuments.length;
    final causes = [
      if (docs > 0) '$docs document${docs == 1 ? ' uses' : 's use'} an older format',
      if (model.lockDiffers) 'the lock was written by another build',
    ];
    final fix = docs > 0
        ? 'Preview the migration for the commands to run.'
        : 'Run `fynkel project lock` to rewrite it.';
    return Container(
      key: const Key('console.upgrade'),
      color: t.surfaceOf(t.warning, .12),
      padding: const EdgeInsets.fromLTRB(10, 4, 4, 4),
      child: Row(
        children: [
          Icon(FIcons.upgrade, size: t.icon, color: t.warning),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              'This project is from an older build: ${causes.join(' and ')}. '
              '$count diagnostic${count == 1 ? ' follows' : 's follow'} from that. $fix',
              style: theme.textTheme.bodySmall?.copyWith(color: t.text),
            ),
          ),
          if (docs > 0 && widget.onMigrationPreview != null)
            TextButton(
              key: const Key('console.upgrade.preview'),
              onPressed: widget.onMigrationPreview,
              child: const Text('Preview migration…'),
            ),
          TextButton(
            key: const Key('console.upgrade.toggle'),
            onPressed: model.toggleUpgrade,
            child: Text(model.showUpgrade ? 'Fold them' : 'Show them'),
          ),
        ],
      ),
    );
  }

  /// Unity's: the source ▾, Collapse, the search, the counts as flat
  /// toggles; copy, open and validate again in the ⋮ menu.
  Widget _toolbar(BuildContext context, ThemeData theme) {
    final project = model.project;
    final t = context.tokens;
    final source = _source(context, game: false);
    final small = theme.textTheme.bodySmall?.copyWith(color: t.text);
    PopupMenuItem<String> item(String key, String label, {bool enabled = true}) => PopupMenuItem(
      key: Key(key),
      value: key,
      height: 26,
      enabled: enabled,
      child: Text(label, style: enabled ? small : small?.copyWith(color: t.textDisabled)),
    );
    return Container(
      height: t.row + 6,
      decoration: BoxDecoration(
        color: t.header,
        border: Border(bottom: t.hairline),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 4),
      child: Row(
        children: [
          if (source != null) ...[source, const ConsoleToolbarDivider()],
          ConsoleToggle(
            key: const Key('console.collapse'),
            label: 'Collapse',
            tooltip: 'Collapse identical diagnostics into one row with a count',
            on: model.collapse,
            onTap: () => model.setCollapse(!model.collapse),
          ),
          const SizedBox(width: 6),
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
          const SizedBox(width: 6),
          for (final s in Severity.values)
            ConsoleToggle(
              key: Key('console.filter.${s.name}'),
              icon: _iconOf(s),
              iconColor: _colorOf(theme, s),
              label: '${model.countOf(s)}',
              tooltip: 'Show ${s.name}s',
              on: model.severities.contains(s),
              onTap: () => model.toggleSeverity(s),
            ),
          PopupMenuButton<String>(
            key: const Key('console.more'),
            popUpAnimationStyle: AnimationStyle.noAnimation,
            // The footer's facts, now the footer is gone (docs/ui-refresh).
            tooltip: [
              'More',
              '${project.documents} documents, ${project.sources} sources, ${project.references} references',
              if (project.validatedAt case final at?) 'validated ${_time(at)}',
            ].join('\n'),
            padding: EdgeInsets.zero,
            icon: Icon(FIcons.moreVert, size: 14, color: t.textSecondary),
            style: IconButton.styleFrom(minimumSize: const Size(24, 24), fixedSize: const Size(24, 24)),
            itemBuilder: (_) => [
              item('console.copy', 'Copy  ⌘C', enabled: model.entries.isNotEmpty),
              item('console.openInEditor', 'Open in Editor  ⌥⌘E', enabled: model.canOpenSelectedInEditor),
              item('console.open', 'Open in IDE  ⌥⌘O', enabled: model.canOpenSelected),
              const PopupMenuDivider(),
              item('console.revalidate', 'Validate Again', enabled: !project.validating),
            ],
            onSelected: (key) => switch (key) {
              'console.copy' => model.copy(),
              'console.openInEditor' => model.openSelectedInEditor(),
              'console.open' => model.openSelected(),
              'console.revalidate' => project.revalidate(),
              _ => null,
            },
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

/// Picks what the Console lists (Unity's Editor ▾): the project's
/// diagnostics or the running game's records. Their error counts show in
/// the menu, so a Play that errors is visible from the Project list.
class ConsoleSourceButton extends StatelessWidget {
  const ConsoleSourceButton({
    super.key,
    required this.game,
    required this.playing,
    required this.projectErrors,
    required this.gameErrors,
    required this.onPick,
  });

  final bool game;
  final bool playing;
  final int projectErrors;
  final int gameErrors;
  final ValueChanged<int> onPick;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final small = Theme.of(context).textTheme.bodySmall;
    Widget label(String text, int errors, {bool play = false}) => Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (play) ...[Icon(FIcons.playArrow, size: 12, color: t.accent), const SizedBox(width: 4)],
        Text(text, style: small?.copyWith(color: t.text)),
        if (errors > 0) ...[
          const SizedBox(width: 6),
          Icon(FIcons.error, size: 12, color: t.error),
          const SizedBox(width: 2),
          Text('$errors', style: small?.copyWith(color: t.textSecondary)),
        ],
      ],
    );
    return PopupMenuButton<int>(
      key: const Key('console.source'),
      popUpAnimationStyle: AnimationStyle.noAnimation,
      tooltip: 'What the Console lists: the project\'s diagnostics or the game\'s output',
      onSelected: onPick,
      itemBuilder: (_) => [
        PopupMenuItem(
          key: const Key('console.tab.project'),
          value: ConsolePanel.projectTab,
          height: 26,
          child: label('Project', projectErrors),
        ),
        PopupMenuItem(
          key: const Key('console.tab.game'),
          value: ConsolePanel.gameTab,
          height: 26,
          child: label('Game', gameErrors, play: playing),
        ),
      ],
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 3),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (game && playing) ...[Icon(FIcons.playArrow, size: 12, color: t.accent), const SizedBox(width: 4)],
            Text(game ? 'Game' : 'Project', style: small?.copyWith(color: t.text)),
            const SizedBox(width: 3),
            Icon(FIcons.chevronDown, size: 12, color: t.textDisabled),
          ],
        ),
      ),
    );
  }
}

/// A thin upright line between toolbar groups.
class ConsoleToolbarDivider extends StatelessWidget {
  const ConsoleToolbarDivider({super.key});

  @override
  Widget build(BuildContext context) =>
      Container(width: 1, height: 14, margin: const EdgeInsets.symmetric(horizontal: 6), color: context.tokens.line);
}

/// A flat toolbar toggle (Unity's Collapse, the counts): text, or an icon
/// and a count, on a quiet grey fill while on; the accent is not used.
class ConsoleToggle extends StatelessWidget {
  const ConsoleToggle({
    super.key,
    this.icon,
    this.iconColor,
    required this.label,
    required this.tooltip,
    required this.on,
    required this.onTap,
  });

  final IconData? icon;
  final Color? iconColor;
  final String label;
  final String tooltip;
  final bool on;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return Tooltip(
      message: tooltip,
      waitDuration: const Duration(milliseconds: 500),
      child: InkWell(
        onTap: onTap,
        borderRadius: t.cornerControl,
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
          decoration: BoxDecoration(color: on ? t.selectionQuiet : null, borderRadius: t.cornerControl),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (icon != null) ...[
                Icon(icon, size: 13, color: on ? iconColor : t.textDisabled),
                const SizedBox(width: 4),
              ],
              Text(
                label,
                style: Theme.of(context).textTheme.bodySmall?.copyWith(
                  color: on ? t.text : t.textSecondary,
                  fontFeatures: const [FontFeature.tabularFigures()],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
