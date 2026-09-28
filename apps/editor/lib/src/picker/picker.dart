// The one picker (blueprint item 9d): every place that asks "which one?"
// opens this popover — an asset slot, an entity slot, Open Document, Add
// Component and the command palette. A search field, the results grouped
// (by source file, folder or category), **None** first where the field may
// be empty, a preview pane on the right (a thumbnail and a few facts), and
// the same keys everywhere: ↑ ↓ to move, ⏎ to choose, esc to close.
//
// It sits under the field that asked (its right edge on the field's right
// edge, above it when there is no room below), or at the top of the window
// for the palette and Open Document. It opens without an animation, like
// every popup in the editor (item 9a).
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../theme/icons.dart';
import '../theme/theme.dart';
import '../theme/tokens.dart';

/// One row of a picker.
final class PickerItem<T> {
  const PickerItem({
    required this.value,
    required this.title,
    this.id,
    this.detail,
    this.group,
    this.groupDetail,
    this.icon,
    this.shortcut,
    this.note,
    this.enabled = true,
    this.keywords = '',
    this.description,
    this.facts = const [],
    this.preview,
    this.tab,
  });

  final T value;
  final String title;

  /// Names the row's key (`<prefix>.<id>`); the row's index when null.
  final String? id;

  /// Dim text after the title: a part's source name, a path.
  final String? detail;

  /// The group the row is listed under; consecutive rows share a header.
  final String? group;

  /// Dim text after the group's name (a folder).
  final String? groupDetail;
  final IconData? icon;

  /// A keyboard shortcut, drawn as a key cap at the row's end.
  final String? shortcut;

  /// Dim text at the row's end ("already on this entity").
  final String? note;

  /// A disabled row is listed dim and cannot be chosen.
  final bool enabled;

  /// More words the search matches (an id, a type name).
  final String keywords;

  /// A sentence under the facts in the preview pane.
  final String? description;
  final List<(String, String)> facts;

  /// The preview pane's picture (a texture's pixels, a swatch); the row's
  /// icon, large, when null.
  final WidgetBuilder? preview;

  /// The tab the row is listed on, when the picker has [showPicker]'s
  /// `tabs`.
  final String? tab;

  String get _haystack => '$title ${detail ?? ''} ${group ?? ''} ${groupDetail ?? ''} $keywords'.toLowerCase();
}

/// What a picker answered: a value, or **None** (the field is cleared).
/// A cancelled picker answers null instead.
final class PickerChoice<T> {
  const PickerChoice.value(T this.value) : isNone = false;
  const PickerChoice.none() : value = null, isNone = true;

  final T? value;
  final bool isNone;
}

/// Ranks an item against a query: 0 hides it, higher sorts first.
typedef PickerScore<T> = int Function(PickerItem<T> item, String query);

/// Opens the picker. [anchor] is the asking field's global rectangle; null
/// puts the picker at the top of the window. [allowNone] adds the **None**
/// row. With [score], a query ranks the results into one flat list (the
/// palette); without it, a query filters and the groups stay. With
/// [tabs] (an object field's Scene and Assets, ADR-0036 §6), each item is
/// listed on its [PickerItem.tab]; [tab] is the one shown first, and
/// **None** is on every tab.
Future<PickerChoice<T>?> showPicker<T>(
  BuildContext context, {
  required String title,
  required List<PickerItem<T>> items,
  String? subject,
  Rect? anchor,
  bool allowNone = false,
  String noneDetail = 'clears the field',
  T? current,
  String hint = 'Search',
  String keyPrefix = 'picker',
  String? countNoun,
  String empty = 'Nothing matches. Try part of a name or path.',
  PickerScore<T>? score,
  List<String>? tabs,
  String? tab,
}) {
  return showDialog<PickerChoice<T>>(
    context: context,
    animationStyle: AnimationStyle.noAnimation,
    useSafeArea: false,
    barrierColor: anchor == null ? const Color(0x33000000) : Colors.transparent,
    builder: (_) => _PickerRoute<T>(
      title: title,
      subject: subject,
      items: items,
      anchor: anchor,
      allowNone: allowNone,
      noneDetail: noneDetail,
      current: current,
      hint: hint,
      keyPrefix: keyPrefix,
      countNoun: countNoun,
      empty: empty,
      score: score,
      tabs: tabs,
      tab: tab,
    ),
  );
}

/// The global rectangle of the widget [context] belongs to — what a field
/// passes as the picker's anchor.
Rect? globalRectOf(BuildContext context) {
  final box = context.findRenderObject();
  if (box is! RenderBox || !box.hasSize) return null;
  return box.localToGlobal(Offset.zero) & box.size;
}

const _width = 600.0;
const _previewWidth = 196.0;
const _listMax = 300.0;
const _chromeHeight = 120.0; // title, search, footer

class _PickerRoute<T> extends StatelessWidget {
  const _PickerRoute({
    required this.title,
    required this.subject,
    required this.items,
    required this.anchor,
    required this.allowNone,
    required this.noneDetail,
    required this.current,
    required this.hint,
    required this.keyPrefix,
    required this.countNoun,
    required this.empty,
    required this.score,
    required this.tabs,
    required this.tab,
  });

  final String title;
  final String? subject;
  final List<PickerItem<T>> items;
  final Rect? anchor;
  final bool allowNone;
  final String noneDetail;
  final T? current;
  final String hint;
  final String keyPrefix;
  final String? countNoun;
  final String empty;
  final PickerScore<T>? score;
  final List<String>? tabs;
  final String? tab;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, c) {
        final width = _width.clamp(0.0, c.maxWidth - 16).toDouble();
        final height = _listMax + _chromeHeight;
        var left = (c.maxWidth - width) / 2;
        var top = 44.0;
        final a = anchor;
        if (a != null) {
          left = a.right - width;
          top = a.bottom + 4;
          if (top + height > c.maxHeight - 8) top = a.top - height - 4;
        }
        left = left.clamp(8.0, (c.maxWidth - width - 8).clamp(8.0, double.infinity)).toDouble();
        top = top.clamp(8.0, (c.maxHeight - 48).clamp(8.0, double.infinity)).toDouble();
        return Stack(
          children: [
            Positioned(
              left: left,
              top: top,
              width: width,
              child: _Picker<T>(
                key: Key(keyPrefix),
                title: title,
                subject: subject,
                items: items,
                allowNone: allowNone,
                noneDetail: noneDetail,
                current: current,
                hint: hint,
                keyPrefix: keyPrefix,
                countNoun: countNoun,
                empty: empty,
                score: score,
                tabs: tabs,
                tab: tab,
                maxListHeight: (c.maxHeight - top - _chromeHeight - 8).clamp(120.0, _listMax).toDouble(),
              ),
            ),
          ],
        );
      },
    );
  }
}

/// A row as shown: a group header, the None row, or an item.
sealed class _Row<T> {
  const _Row();
}

final class _Header<T> extends _Row<T> {
  const _Header(this.name, this.detail);
  final String name;
  final String? detail;
}

final class _NoneRow<T> extends _Row<T> {
  const _NoneRow();
}

final class _ItemRow<T> extends _Row<T> {
  const _ItemRow(this.item, this.index);
  final PickerItem<T> item;

  /// The item's place in the original list (its default key).
  final int index;
}

class _Picker<T> extends StatefulWidget {
  const _Picker({
    super.key,
    required this.title,
    required this.subject,
    required this.items,
    required this.allowNone,
    required this.noneDetail,
    required this.current,
    required this.hint,
    required this.keyPrefix,
    required this.countNoun,
    required this.empty,
    required this.score,
    required this.tabs,
    required this.tab,
    required this.maxListHeight,
  });

  final String title;
  final String? subject;
  final List<PickerItem<T>> items;
  final bool allowNone;
  final String noneDetail;
  final T? current;
  final String hint;
  final String keyPrefix;
  final String? countNoun;
  final String empty;
  final PickerScore<T>? score;
  final List<String>? tabs;
  final String? tab;
  final double maxListHeight;

  @override
  State<_Picker<T>> createState() => _PickerState<T>();
}

class _PickerState<T> extends State<_Picker<T>> {
  final _query = TextEditingController();
  final _scroll = ScrollController();
  late List<_Row<T>> _rows;

  /// Index into the choosable rows (None and items), not into [_rows].
  int _cursor = 0;

  /// The tab shown, when there are tabs.
  String? _tab;

  @override
  void initState() {
    super.initState();
    _tab = widget.tabs == null ? null : (widget.tab ?? widget.tabs!.first);
    _rows = _build('');
    // Start on what the field holds now, so ⏎ keeps it and ↑↓ move from it.
    final choosable = _choosable;
    final at = choosable.indexWhere((r) => r is _ItemRow<T> && r.item.value == widget.current);
    if (widget.current != null && at >= 0) _cursor = at;
    WidgetsBinding.instance.addPostFrameCallback((_) => _reveal());
  }

  @override
  void dispose() {
    _query.dispose();
    _scroll.dispose();
    super.dispose();
  }

  List<_Row<T>> get _choosable => [
    for (final r in _rows)
      if (r is! _Header<T>) r,
  ];

  bool _onTab(PickerItem<T> it) => _tab == null || it.tab == _tab;

  List<_Row<T>> _build(String query) {
    final q = query.trim().toLowerCase();
    final out = <_Row<T>>[];
    if (q.isEmpty) {
      if (widget.allowNone) out.add(const _NoneRow());
      String? group;
      for (final (i, it) in widget.items.indexed) {
        if (!_onTab(it)) continue;
        if (it.group != null && it.group != group) out.add(_Header(it.group!, it.groupDetail));
        group = it.group;
        out.add(_ItemRow(it, i));
      }
      return out;
    }
    final score = widget.score;
    if (score != null) {
      final ranked = [
        for (final (i, it) in widget.items.indexed)
          if (_onTab(it) && score(it, q) > 0) (i, it, score(it, q)),
      ]..sort((a, b) => b.$3 != a.$3 ? b.$3.compareTo(a.$3) : a.$1.compareTo(b.$1));
      return [for (final r in ranked) _ItemRow(r.$2, r.$1)];
    }
    final words = q.split(RegExp(r'\s+'));
    String? group;
    for (final (i, it) in widget.items.indexed) {
      final hay = it._haystack;
      if (!_onTab(it) || !words.every(hay.contains)) continue;
      if (it.group != null && it.group != group) out.add(_Header(it.group!, it.groupDetail));
      group = it.group;
      out.add(_ItemRow(it, i));
    }
    return out;
  }

  void _showTab(String tab) {
    if (tab == _tab) return;
    setState(() {
      _tab = tab;
      _rows = _build(_query.text);
      _cursor = 0;
    });
    _scroll.hasClients ? _scroll.jumpTo(0) : null;
  }

  void _onQuery(String q) {
    setState(() {
      _rows = _build(q);
      _cursor = 0;
    });
    _scroll.hasClients ? _scroll.jumpTo(0) : null;
  }

  double get _rowHeight => context.tokens.row + 6;
  double get _headerHeight => context.tokens.row + 4;

  void _move(int delta) {
    final n = _choosable.length;
    if (n == 0) return;
    setState(() => _cursor = (_cursor + delta).clamp(0, n - 1));
    _reveal();
  }

  /// Scrolls the cursor's row into view.
  void _reveal() {
    if (!mounted || !_scroll.hasClients) return;
    var y = 0.0;
    var seen = -1;
    double h = _rowHeight;
    for (final r in _rows) {
      h = r is _Header<T> ? _headerHeight : _rowHeight;
      if (r is! _Header<T>) seen++;
      if (seen == _cursor && r is! _Header<T>) break;
      y += h;
    }
    final view = _scroll.position.viewportDimension;
    final top = _scroll.offset;
    if (y < top) {
      _scroll.jumpTo(y);
    } else if (y + h > top + view) {
      _scroll.jumpTo((y + h - view).clamp(0.0, _scroll.position.maxScrollExtent));
    }
  }

  void _choose(_Row<T> row) {
    switch (row) {
      case _NoneRow<T>():
        Navigator.of(context).pop(PickerChoice<T>.none());
      case _ItemRow<T>(:final item) when item.enabled:
        Navigator.of(context).pop(PickerChoice<T>.value(item.value));
      default:
        break;
    }
  }

  /// Unity's Scene / Assets tab: a quiet label, filled when shown.
  Widget _tabButton(BuildContext context, String tab) {
    final t = context.tokens;
    final on = tab == _tab;
    return Semantics(
      key: Key('${widget.keyPrefix}.tab.$tab'),
      button: true,
      selected: on,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: () => _showTab(tab),
          child: Container(
            margin: const EdgeInsets.only(left: 2),
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
            decoration: BoxDecoration(color: on ? t.lineStrong : null, borderRadius: t.cornerControl),
            child: Text(
              tab,
              style: Theme.of(context).textTheme.labelSmall?.copyWith(
                color: on ? t.text : t.textSecondary,
                fontWeight: on ? FontWeight.w600 : null,
              ),
            ),
          ),
        ),
      ),
    );
  }

  void _chooseCursor() {
    final c = _choosable;
    if (_cursor < c.length) _choose(c[_cursor]);
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final choosable = _choosable;
    if (_cursor >= choosable.length) _cursor = choosable.isEmpty ? 0 : choosable.length - 1;
    final at = choosable.isEmpty ? null : choosable[_cursor];
    final total = widget.items.where(_onTab).length;
    final shown = choosable.whereType<_ItemRow<T>>().length;
    final small = theme.textTheme.bodySmall?.copyWith(color: t.textDisabled);
    return Material(
      color: t.popup,
      elevation: 0,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(8),
        side: BorderSide(color: t.lineStrong),
      ),
      clipBehavior: Clip.antiAlias,
      shadowColor: t.shadow,
      child: DecoratedBox(
        decoration: BoxDecoration(
          boxShadow: [BoxShadow(color: t.shadow, blurRadius: 32, offset: const Offset(0, 12))],
        ),
        child: CallbackShortcuts(
          bindings: {
            const SingleActivator(LogicalKeyboardKey.arrowDown): () => _move(1),
            const SingleActivator(LogicalKeyboardKey.arrowUp): () => _move(-1),
            const SingleActivator(LogicalKeyboardKey.pageDown): () => _move(8),
            const SingleActivator(LogicalKeyboardKey.pageUp): () => _move(-8),
            const SingleActivator(LogicalKeyboardKey.enter): _chooseCursor,
            const SingleActivator(LogicalKeyboardKey.numpadEnter): _chooseCursor,
          },
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: EdgeInsets.fromLTRB(10, 8, widget.tabs == null ? 10 : 8, 0),
                child: Row(
                  children: [
                    Expanded(
                      child: Text.rich(
                        TextSpan(
                          children: [
                            TextSpan(
                              text: widget.title,
                              style: TextStyle(color: t.textSecondary, fontWeight: FontWeight.w600),
                            ),
                            if (widget.subject != null) TextSpan(text: ' for ${widget.subject}'),
                          ],
                        ),
                        key: Key('${widget.keyPrefix}.title'),
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.labelSmall?.copyWith(color: t.textDisabled),
                      ),
                    ),
                    for (final tab in widget.tabs ?? const <String>[]) _tabButton(context, tab),
                  ],
                ),
              ),
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        Padding(
                          padding: const EdgeInsets.fromLTRB(10, 6, 8, 6),
                          child: SizedBox(
                            height: t.row + 6,
                            child: TextField(
                              key: Key('${widget.keyPrefix}.search'),
                              controller: _query,
                              autofocus: true,
                              style: theme.textTheme.bodyMedium,
                              decoration: InputDecoration(
                                hintText: widget.hint,
                                prefixIcon: Icon(FIcons.search, size: 13, color: t.textDisabled),
                                prefixIconConstraints: const BoxConstraints(minWidth: 26, minHeight: 20),
                                contentPadding: EdgeInsets.symmetric(horizontal: 6, vertical: (t.row - 10) / 2),
                              ),
                              onChanged: _onQuery,
                              onSubmitted: (_) => _chooseCursor(),
                            ),
                          ),
                        ),
                        SizedBox(
                          height: widget.maxListHeight,
                          child: _rows.isEmpty
                              ? Center(
                                  child: Padding(
                                    padding: const EdgeInsets.all(16),
                                    child: Text(
                                      total == 0 && _query.text.isEmpty
                                          ? 'There is nothing to choose.'
                                          : widget.empty,
                                      key: Key('${widget.keyPrefix}.empty'),
                                      textAlign: TextAlign.center,
                                      style: small,
                                    ),
                                  ),
                                )
                              : ListView.builder(
                                  controller: _scroll,
                                  padding: const EdgeInsets.fromLTRB(4, 0, 4, 6),
                                  itemCount: _rows.length,
                                  itemBuilder: (context, i) => _row(context, _rows[i], at),
                                ),
                        ),
                      ],
                    ),
                  ),
                  Container(
                    width: _previewWidth,
                    height: widget.maxListHeight + t.row + 18,
                    decoration: BoxDecoration(border: Border(left: t.hairline)),
                    padding: const EdgeInsets.all(10),
                    child: _preview(context, at),
                  ),
                ],
              ),
              Container(
                decoration: BoxDecoration(border: Border(top: t.hairline)),
                padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
                child: DefaultTextStyle.merge(
                  style: small,
                  child: Row(
                    children: [
                      _hint(t, '↑↓', 'move'),
                      const SizedBox(width: 14),
                      _hint(t, '⏎', 'choose'),
                      const SizedBox(width: 14),
                      _hint(t, 'esc', 'close'),
                      const Spacer(),
                      Text(
                        '$shown of $total${widget.countNoun == null ? '' : ' ${widget.countNoun}'}',
                        key: Key('${widget.keyPrefix}.count'),
                      ),
                    ],
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _hint(FynkelTokens t, String key, String what) => Text.rich(
    TextSpan(
      children: [
        TextSpan(
          text: key,
          style: TextStyle(color: t.textSecondary, fontWeight: FontWeight.w500),
        ),
        TextSpan(text: ' $what'),
      ],
    ),
  );

  Widget _row(BuildContext context, _Row<T> row, _Row<T>? at) {
    final t = context.tokens;
    final theme = Theme.of(context);
    if (row is _Header<T>) {
      return SizedBox(
        height: _headerHeight,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(8, 6, 8, 0),
          child: Row(
            children: [
              Text(
                row.name.toUpperCase(),
                style: TextStyle(
                  fontFamily: uiFontFamily,
                  fontSize: 10,
                  fontWeight: FontWeight.w600,
                  letterSpacing: 0.7,
                  color: t.textDisabled,
                ),
              ),
              if (row.detail != null && row.detail!.isNotEmpty) ...[
                const SizedBox(width: 6),
                Flexible(
                  child: Text(
                    row.detail!,
                    overflow: TextOverflow.ellipsis,
                    style: theme.textTheme.labelSmall?.copyWith(color: t.textDisabled),
                  ),
                ),
              ],
            ],
          ),
        ),
      );
    }
    final on = identical(row, at);
    final item = row is _ItemRow<T> ? row.item : null;
    final enabled = item?.enabled ?? true;
    final fg = on ? t.onSelection : (enabled ? t.text : t.textDisabled);
    final dim = on ? t.onSelection.withValues(alpha: .78) : t.textDisabled;
    final key = row is _NoneRow<T>
        ? Key('${widget.keyPrefix}.none')
        : Key('${widget.keyPrefix}.${item!.id ?? 'row.${(row as _ItemRow<T>).index}'}');
    return MouseRegion(
      onHover: (_) {
        final i = _choosable.indexOf(row);
        if (i >= 0 && i != _cursor) setState(() => _cursor = i);
      },
      child: GestureDetector(
        key: key,
        behavior: HitTestBehavior.opaque,
        onTap: () => _choose(row),
        child: Container(
          height: _rowHeight,
          padding: const EdgeInsets.symmetric(horizontal: 8),
          decoration: BoxDecoration(color: on ? t.selection : null, borderRadius: BorderRadius.circular(4)),
          child: Row(
            children: [
              Icon(item == null ? FIcons.none : (item.icon ?? FIcons.insertDriveFileOutlined), size: 14, color: dim),
              const SizedBox(width: 8),
              Flexible(
                flex: 0,
                child: Text(
                  item == null ? 'None' : item.title,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.bodyMedium?.copyWith(color: fg),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  item == null ? '' : (item.detail ?? ''),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.bodySmall?.copyWith(color: dim),
                ),
              ),
              if (item == null)
                Text(widget.noneDetail, style: theme.textTheme.labelSmall?.copyWith(color: dim))
              else if (item.note != null)
                Text(
                  item.note!,
                  key: Key('${widget.keyPrefix}.note.${item.id ?? (row as _ItemRow<T>).index}'),
                  style: theme.textTheme.labelSmall?.copyWith(color: dim),
                )
              else if (item.shortcut != null)
                Container(
                  padding: const EdgeInsets.symmetric(horizontal: 5, vertical: 1),
                  decoration: BoxDecoration(
                    color: on ? null : t.field,
                    border: Border.all(color: on ? t.onSelection.withValues(alpha: .4) : t.lineStrong),
                    borderRadius: t.cornerControl,
                  ),
                  child: Text(
                    item.shortcut!,
                    style: theme.textTheme.labelSmall?.copyWith(color: on ? t.onSelection : t.textSecondary),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _preview(BuildContext context, _Row<T>? at) {
    final t = context.tokens;
    final theme = Theme.of(context);
    if (at == null) return const SizedBox.shrink();
    final item = at is _ItemRow<T> ? at.item : null;
    final big = Container(
      height: 124,
      decoration: BoxDecoration(
        color: t.field,
        border: Border.all(color: t.line),
        borderRadius: BorderRadius.circular(6),
      ),
      clipBehavior: Clip.antiAlias,
      child: item?.preview != null
          ? item!.preview!(context)
          : Center(child: Icon(item?.icon ?? FIcons.none, size: 28, color: t.textDisabled)),
    );
    final facts = item == null ? const [('Result', 'the field is left empty')] : item.facts;
    return Column(
      key: Key('${widget.keyPrefix}.preview'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        big,
        const SizedBox(height: 8),
        Text(
          item == null ? 'None' : (item.detail == null ? item.title : '${item.title}  ${item.detail}'),
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
          style: theme.textTheme.bodyMedium?.copyWith(fontWeight: FontWeight.w600),
        ),
        const SizedBox(height: 6),
        for (final (k, v) in facts)
          Padding(
            padding: const EdgeInsets.only(bottom: 3),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(k, style: theme.textTheme.labelSmall?.copyWith(color: t.textDisabled)),
                const SizedBox(width: 10),
                Expanded(
                  child: Text(
                    v,
                    textAlign: TextAlign.right,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: theme.textTheme.labelSmall?.copyWith(color: t.text),
                  ),
                ),
              ],
            ),
          ),
        if (item?.description != null) ...[
          const SizedBox(height: 4),
          Flexible(
            child: Text(
              item!.description!,
              overflow: TextOverflow.fade,
              style: theme.textTheme.labelSmall?.copyWith(color: t.textSecondary),
            ),
          ),
        ],
      ],
    );
  }
}
