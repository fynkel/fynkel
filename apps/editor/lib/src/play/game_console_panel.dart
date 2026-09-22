// The Game tab's view: the run's toolbar (Play / Stop, search, filters),
// the rows, the selected record and a status line with the state, the pid
// and the last stats window. It is the Project tab's twin on purpose —
// same chips, same collapse, same ↩ and double-click.
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import 'game_console.dart';
import 'game_record.dart';

class GameConsoleView extends StatefulWidget {
  const GameConsoleView({super.key, required this.model, required this.onPlay, required this.onStop, this.searchFocus});

  final GameConsoleModel model;
  final VoidCallback onPlay;
  final VoidCallback onStop;
  final FocusNode? searchFocus;

  @override
  State<GameConsoleView> createState() => _GameConsoleViewState();
}

class _GameConsoleViewState extends State<GameConsoleView> {
  final _search = TextEditingController();
  late final FocusNode _searchFocus = widget.searchFocus ?? FocusNode(debugLabel: 'console.game.search');
  final _listFocus = FocusNode(debugLabel: 'console.game.list');
  final _scroll = ScrollController();
  DateTime? _lastTap;
  String? _lastTapKey;
  int _shown = 0;

  GameConsoleModel get model => widget.model;

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
    _scroll.dispose();
    super.dispose();
  }

  void _moveSelection(int delta) {
    final es = model.entries;
    if (es.isEmpty) return;
    final current = es.indexWhere((e) => model.keyOf(e) == model.selectedKey);
    final next = current < 0 ? (delta > 0 ? 0 : es.length - 1) : (current + delta).clamp(0, es.length - 1);
    model.select(model.keyOf(es[next]));
  }

  /// Keeps the newest row in view while the user has not scrolled away.
  void _followTail(int count) {
    if (!model.follow || count == _shown) return;
    _shown = count;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || !_scroll.hasClients || !model.follow) return;
      _scroll.jumpTo(_scroll.position.maxScrollExtent);
    });
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: model,
      builder: (context, _) {
        final theme = Theme.of(context);
        final entries = model.entries;
        _followTail(entries.length);
        // The Console can be docked very short (and the tab bar took a
        // little of it): the detail then gets a share of what there is
        // instead of pushing the rows out of the panel.
        return LayoutBuilder(
          builder: (context, box) => Column(
            children: [
              _toolbar(theme),
              const Divider(height: 1),
              Expanded(
                child: entries.isEmpty
                    ? Center(
                        child: Text(
                          model.session.records.isEmpty
                              ? 'Not playing. ⌘P runs the project; its logs and diagnostics land here.'
                              : 'Nothing matches the filter.',
                          key: const Key('console.game.empty'),
                          style: theme.textTheme.bodyMedium?.copyWith(color: theme.disabledColor),
                          textAlign: TextAlign.center,
                        ),
                      )
                    : CallbackShortcuts(
                        bindings: {
                          const SingleActivator(LogicalKeyboardKey.arrowDown): () => _moveSelection(1),
                          const SingleActivator(LogicalKeyboardKey.arrowUp): () => _moveSelection(-1),
                          const SingleActivator(LogicalKeyboardKey.enter): () {
                            final r = model.selected?.record;
                            if (r != null) model.activate(r);
                          },
                          const SingleActivator(LogicalKeyboardKey.keyC, meta: true): model.copy,
                        },
                        child: Focus(
                          focusNode: _listFocus,
                          child: NotificationListener<ScrollNotification>(
                            onNotification: (n) {
                              if (n is ScrollUpdateNotification && n.dragDetails != null) {
                                final p = n.metrics;
                                model.setFollow(p.pixels >= p.maxScrollExtent - 8);
                              }
                              return false;
                            },
                            child: ListView.builder(
                              controller: _scroll,
                              itemCount: entries.length,
                              itemBuilder: (context, i) => _row(theme, entries[i]),
                            ),
                          ),
                        ),
                      ),
              ),
              if (model.selected != null) ...[
                const Divider(height: 1),
                _detail(theme, model.selected!, box.maxHeight * 0.4),
              ],
              const Divider(height: 1),
              _status(theme),
            ],
          ),
        );
      },
    );
  }

  Widget _toolbar(ThemeData theme) {
    final session = model.session;
    final active = session.isActive;
    return Padding(
      padding: const EdgeInsets.fromLTRB(8, 6, 8, 6),
      child: Row(
        children: [
          IconButton(
            key: const Key('console.game.play'),
            tooltip: active ? 'Stop the game (⌘.)' : 'Play the project (⌘P)',
            icon: Icon(
              active ? Icons.stop : Icons.play_arrow,
              size: 20,
              color: active ? theme.colorScheme.error : null,
            ),
            onPressed: active ? widget.onStop : widget.onPlay,
          ),
          Expanded(
            child: TextField(
              key: const Key('console.game.search'),
              controller: _search,
              focusNode: _searchFocus,
              decoration: InputDecoration(
                isDense: true,
                prefixIcon: const Icon(Icons.search, size: 18),
                hintText: 'Search the game\'s output…',
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
              key: Key('console.game.filter.${s.name}'),
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
            key: const Key('console.game.output'),
            label: const Text('Output'),
            tooltip: 'The process\'s own lines: the CLI, Flutter, anything the game printed',
            selected: model.showOutput,
            onSelected: model.setShowOutput,
            visualDensity: VisualDensity.compact,
          ),
          const SizedBox(width: 4),
          FilterChip(
            key: const Key('console.game.stats'),
            label: const Text('Stats'),
            tooltip: 'One row per stats window (the Profiler shows them properly, item 9)',
            selected: model.showStats,
            onSelected: model.setShowStats,
            visualDensity: VisualDensity.compact,
          ),
          const SizedBox(width: 4),
          FilterChip(
            key: const Key('console.game.collapse'),
            label: const Text('Collapse'),
            tooltip: 'Collapse identical rows into one with a count',
            selected: model.collapse,
            onSelected: model.setCollapse,
            visualDensity: VisualDensity.compact,
          ),
          const SizedBox(width: 4),
          IconButton(
            key: const Key('console.game.copy'),
            tooltip: 'Copy the selected row, or every visible row',
            icon: const Icon(Icons.copy, size: 18),
            onPressed: model.entries.isEmpty ? null : model.copy,
          ),
          IconButton(
            key: const Key('console.game.open'),
            tooltip: 'Open what this row is about (↩)',
            icon: const Icon(Icons.account_tree_outlined, size: 18),
            onPressed: model.canOpenSelected ? model.openSelected : null,
          ),
          IconButton(
            key: const Key('console.game.clear'),
            tooltip: 'Clear the Game tab (⇧⌘K)',
            icon: const Icon(Icons.clear_all, size: 18),
            onPressed: session.records.isEmpty ? null : session.clear,
          ),
        ],
      ),
    );
  }

  Widget _row(ThemeData theme, GameEntry e) {
    final r = e.record;
    final key = model.keyOf(e);
    final selected = key == model.selectedKey;
    final mono = theme.textTheme.bodySmall?.copyWith(fontFamily: 'Menlo', fontFamilyFallback: const ['monospace']);
    return InkWell(
      key: Key('console.game.row.$key'),
      onTap: () {
        final now = DateTime.now();
        final isDouble = _lastTapKey == key && _lastTap != null && now.difference(_lastTap!) < kDoubleTapTimeout;
        _lastTap = now;
        _lastTapKey = key;
        model.select(key);
        _listFocus.requestFocus();
        if (isDouble) model.activate(r);
      },
      child: Container(
        color: selected ? theme.colorScheme.primaryContainer : null,
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
        child: Row(
          children: [
            Icon(_iconOf(r.severity), size: 14, color: _colorOf(theme, r.severity)),
            const SizedBox(width: 6),
            SizedBox(
              width: 54,
              child: Text(r.tick == null ? '' : '${r.tick}', style: mono, textAlign: TextAlign.right),
            ),
            const SizedBox(width: 8),
            SizedBox(
              width: 74,
              child: Text(r.source ?? r.kind.name, style: mono, overflow: TextOverflow.ellipsis),
            ),
            if (r.code != null) ...[const SizedBox(width: 6), SizedBox(width: 84, child: Text(r.code!, style: mono))],
            const SizedBox(width: 6),
            Expanded(child: Text(r.message, overflow: TextOverflow.ellipsis)),
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
              constraints: const BoxConstraints(maxWidth: 260),
              child: Text(model.location(r), style: mono, overflow: TextOverflow.ellipsis, textAlign: TextAlign.right),
            ),
          ],
        ),
      ),
    );
  }

  Widget _detail(ThemeData theme, GameEntry e, double maxHeight) {
    final r = e.record;
    final mono = theme.textTheme.bodySmall?.copyWith(fontFamily: 'Menlo', fontFamilyFallback: const ['monospace']);
    return Container(
      key: const Key('console.game.detail'),
      constraints: BoxConstraints(maxHeight: maxHeight.clamp(40.0, 140.0)),
      padding: const EdgeInsets.fromLTRB(12, 8, 12, 8),
      alignment: Alignment.topLeft,
      child: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SelectableText(r.text, style: theme.textTheme.bodyMedium),
            if (r.entity != null && r.id == null)
              Text(
                'entity 0x${r.entity!.toRadixString(16)} — the game named no authored entity for it',
                style: theme.textTheme.bodySmall,
              ),
            if (r.id != null) SelectableText('id ${r.id}', style: mono),
            if (r.suggestion != null) Text('Suggestion: ${r.suggestion}', style: theme.textTheme.bodySmall),
            for (final rel in r.related) Text('related: ${rel.code} ${rel.message}', style: theme.textTheme.bodySmall),
            if (r.kind == GameRecordKind.stats && r.raw != null) SelectableText(r.raw!, style: mono),
          ],
        ),
      ),
    );
  }

  Widget _status(ThemeData theme) {
    final session = model.session;
    final stats = session.lastStats;
    final dropped = session.droppedRecords;
    final text = [
      session.statusText,
      // The game's pid while it is up, the CLI's until the game announces
      // itself: the number a `kill` from a terminal needs, never a number
      // the two could be confused for.
      if (session.isActive)
        if (session.gamePid case final game?) 'game pid $game' else if (session.pid case final cli?) 'cli pid $cli',
      if (stats != null) '${stats['fps']} fps · ${stats['ticks']} ticks/s · ${stats['entities']} entities',
      if (session.failure != null) session.failure!,
      if (dropped > 0) '$dropped older rows dropped',
      if (session.runningOlderFiles) 'edited since Play — changes apply on the next Play',
    ].join(' · ');
    return Padding(
      padding: const EdgeInsets.fromLTRB(10, 4, 10, 4),
      child: Row(
        children: [
          Expanded(
            child: Text(
              text,
              key: const Key('console.game.status'),
              style: theme.textTheme.bodySmall?.copyWith(
                color: session.crashed || session.failure != null ? theme.colorScheme.error : theme.disabledColor,
              ),
              overflow: TextOverflow.ellipsis,
            ),
          ),
        ],
      ),
    );
  }

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
