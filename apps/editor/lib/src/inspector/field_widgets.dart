// One widget per field kind (blueprint §19.2e item 4; editor-ux §22.6):
// numbers with a label drag, bounds, step and unit (a slider when hinted),
// integers, booleans, strings (single or multiline), enum menus, vectors,
// rotation as Euler degrees over the stored quaternion, colours with a
// picker, asset references with the shared picker (item 9d) and drag-in,
// entity references, layer masks, arrays, nested objects and a JSON fallback.
//
// Every widget shows the *committed* value it is given and reports an edit
// through `onCommit` exactly once per user gesture: a drag previews locally
// and commits on release (one transaction, one undo step); a text field
// commits on ↩ or on losing focus, and ⎋ puts the committed value back.
// A mixed value (the selection disagrees) is drawn as `—` until edited.
//
// Item 9b: the Inspector reads left to right — one label column, the value
// left-aligned right after it, the unit right after the number, the axis
// letter inside a vector's field, an empty value `—` and an empty slot
// `None`. A number shows at most three decimals until it is clicked, then
// its full precision, so a narrow field never cuts a value off.
import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../picker/picker.dart';
import '../project/editor_project.dart';
import '../theme/icons.dart';
import '../theme/theme.dart';
import '../theme/tokens.dart';
import 'field_spec.dart';
import 'inspector_math.dart';

/// The `DragTarget` payload an asset row (the Project browser, item 5)
/// drags onto an asset field.
final class AssetDragPayload {
  const AssetDragPayload({required this.id, this.sub, required this.kind, required this.name});

  final String id;
  final String? sub;

  /// `mesh`, `material`, `texture`, … — what the field checks.
  final String kind;
  final String name;

  Map<String, Object?> toRef() => {'asset': id, if (sub != null && sub!.isNotEmpty) 'sub': sub};
}

typedef CommitValue = void Function(Object? value);

/// One inspector control, from the density preference (item 9a): a little
/// taller than a list row, because a field has a border of its own.
double _rowHeight(BuildContext context) => context.tokens.row + 4;

/// The label column (item 9b): the same width on every row, so the values
/// line up in one column down the whole Inspector — narrower only when the
/// panel is.
const _labelColumn = 112.0;
const _labelShare = 0.45;

/// Where a label column ends for a row [width] wide.
double labelColumnWidth(double width) => math.min(_labelColumn, width * _labelShare);

/// The gap between the label column and the value.
const _labelGap = 10.0;

/// A label on the left, a control on the right, the error under both.
class FieldRow extends StatelessWidget {
  const FieldRow({
    super.key,
    required this.label,
    required this.child,
    this.tooltip,
    this.error,
    this.overridden = false,
    this.onRevert,
    this.onApply,
    this.dragLabel,
    this.indent = 0,
    this.unit,
  });

  final String label;
  final String? tooltip;
  final Widget child;
  final String? error;

  /// Shown dim after the label — vectors carry their unit here, not in
  /// their narrow per-axis fields.
  final String? unit;

  /// Bold label with a revert arrow and an apply arrow (Prefab mode, item
  /// 7: a derived entity's field an override wrote — Revert drops the
  /// override, Apply writes it into the prefab).
  final bool overridden;
  final VoidCallback? onRevert;
  final VoidCallback? onApply;

  /// When set, the label is a horizontal drag surface (numbers).
  final Widget Function(Widget label)? dragLabel;
  final double indent;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    Widget text = Text(
      label,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: theme.textTheme.bodySmall?.copyWith(
        color: overridden ? context.tokens.text : context.tokens.textSecondary,
        fontWeight: overridden ? FontWeight.w600 : null,
      ),
    );
    if (tooltip != null && tooltip!.isNotEmpty) {
      text = Tooltip(message: tooltip!, waitDuration: const Duration(milliseconds: 500), child: text);
    }
    if (dragLabel != null) text = dragLabel!(text);
    return Padding(
      padding: EdgeInsets.only(left: indent, top: 2, bottom: 2),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          LayoutBuilder(
            builder: (context, c) => Row(
              crossAxisAlignment: CrossAxisAlignment.center,
              children: [
                SizedBox(
                  width: math.max(48, labelColumnWidth(c.maxWidth + indent) - indent),
                  child: Row(
                    children: [
                      Expanded(child: text),
                      if (unit != null && unit!.isNotEmpty)
                        Padding(
                          padding: const EdgeInsets.only(left: 5, right: 2),
                          child: Text(
                            unit!,
                            key: const ValueKey('field.unit'),
                            style: theme.textTheme.labelSmall?.copyWith(color: context.tokens.textDisabled),
                          ),
                        ),
                      if (overridden && onRevert != null)
                        IconButton(
                          key: const ValueKey('field.revert'),
                          tooltip: 'Revert override — back to the prefab\'s value',
                          icon: const Icon(FIcons.undo, size: 13),
                          padding: EdgeInsets.zero,
                          constraints: const BoxConstraints(minWidth: 18, minHeight: 18),
                          onPressed: onRevert,
                        ),
                      if (overridden && onApply != null)
                        IconButton(
                          key: const ValueKey('field.apply'),
                          tooltip: 'Apply to prefab — this value becomes the prefab\'s',
                          icon: const Icon(FIcons.upload, size: 13),
                          padding: EdgeInsets.zero,
                          constraints: const BoxConstraints(minWidth: 18, minHeight: 18),
                          onPressed: onApply,
                        ),
                    ],
                  ),
                ),
                const SizedBox(width: _labelGap),
                Expanded(child: child),
              ],
            ),
          ),
          if (error != null && error!.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 2, bottom: 2),
              child: Text(
                error!,
                key: const ValueKey('field.error'),
                style: theme.textTheme.bodySmall?.copyWith(color: context.tokens.error),
              ),
            ),
        ],
      ),
    );
  }
}

// ── Text entry shared by number and string fields ──────────────────────────

/// A compact text field that commits on ↩ / focus loss and restores on ⎋.
///
/// A single line is drawn as the Inspector's own field box (item 9b): the
/// recessed fill, a hairline that turns accent while editing, an optional
/// [leading] inside it (a vector's axis letter, a colour swatch), the
/// value left-aligned, the [unit] right after the value, and an optional
/// [trailing] (a pick button). Multiline text keeps the theme's field.
class _CommitText extends StatefulWidget {
  const _CommitText({
    super.key,
    required this.text,
    required this.onCommit,
    this.display,
    this.enabled = true,
    this.multiline = false,
    this.unit,
    this.hint = '—',
    this.monospace = false,
    this.leading,
    this.trailing,
    this.live,
  });

  /// The committed value in full; what an edit starts from.
  final String text;

  /// A newer [text] and [display] pushed between builds (a running game's
  /// value, during Play): shown at once unless the field is being edited,
  /// without rebuilding anything.
  final ValueListenable<({String text, String display})?>? live;

  /// A shorter form shown while the field is not being edited (a number's
  /// three decimals); [text] when null.
  final String? display;
  final void Function(String text) onCommit;
  final bool enabled;
  final bool multiline;
  final String? unit;

  /// Shown dim when the value is empty.
  final String? hint;
  final bool monospace;
  final Widget? leading;
  final Widget? trailing;

  @override
  State<_CommitText> createState() => _CommitTextState();
}

class _CommitTextState extends State<_CommitText> {
  late final _controller = TextEditingController(text: _shown);
  final _focus = FocusNode(debugLabel: 'inspector.text');
  bool _editing = false;
  bool _hover = false;

  /// The last [_CommitText.live] value since the widget was rebuilt.
  ({String text, String display})? _pushed;

  String get _text => _pushed?.text ?? widget.text;
  String get _shown => _pushed?.display ?? widget.display ?? widget.text;

  /// The single-line text field of the last build and what it was built
  /// from. A new value reaches it through [_controller] alone, so a field
  /// whose value moves many times a second (the running game's, during
  /// Play) re-lays out its text without rebuilding the field.
  TextField? _field;
  Object? _fieldInputs;

  @override
  void initState() {
    super.initState();
    _focus.addListener(_onFocus);
    widget.live?.addListener(_onLive);
  }

  @override
  void didUpdateWidget(covariant _CommitText old) {
    super.didUpdateWidget(old);
    if (!identical(old.live, widget.live)) {
      old.live?.removeListener(_onLive);
      widget.live?.addListener(_onLive);
    }
    // The widget carries the newest value again.
    _pushed = null;
    // A new committed value replaces the text unless the user is mid-edit.
    if (!_editing && _shown != _controller.text) _controller.text = _shown;
  }

  void _onLive() {
    final v = widget.live?.value;
    if (v == null || !mounted) return;
    _pushed = v;
    // Never under the user's typing: the edit wins, as it does on a rebuild.
    if (!_editing && _controller.text != v.display) _controller.text = v.display;
  }

  @override
  void dispose() {
    widget.live?.removeListener(_onLive);
    _focus.removeListener(_onFocus);
    _focus.dispose();
    _controller.dispose();
    super.dispose();
  }

  void _onFocus() {
    if (_focus.hasFocus) {
      _editing = true;
      // Editing starts from the full value, all of it selected.
      _controller.value = TextEditingValue(
        text: _text,
        selection: TextSelection(baseOffset: 0, extentOffset: _text.length),
      );
    } else if (_editing) {
      _commit();
    }
    if (mounted) setState(() {});
  }

  void _commit() {
    _editing = false;
    if (_controller.text != _text) {
      widget.onCommit(_controller.text);
    }
    // Whatever the outcome, show the committed value (a refusal keeps the old one).
    if (mounted) setState(() => _controller.text = _shown);
  }

  void _cancel() {
    _editing = false;
    _controller.text = _shown;
    _focus.unfocus();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    final style = widget.monospace
        ? monoStyle(context, color: widget.enabled ? t.text : t.textDisabled)
        : theme.textTheme.bodySmall?.copyWith(color: widget.enabled ? t.text : t.textDisabled);
    if (widget.multiline) {
      return CallbackShortcuts(
        bindings: {const SingleActivator(LogicalKeyboardKey.escape): _cancel},
        child: TextField(
          controller: _controller,
          focusNode: _focus,
          enabled: widget.enabled,
          maxLines: null,
          minLines: 3,
          keyboardType: TextInputType.multiline,
          style: style,
          decoration: InputDecoration(hintText: widget.hint),
        ),
      );
    }
    final textLeft = widget.leading == null ? 6.0 : 3.0;
    final inputs = (widget.enabled, style, widget.hint, t.textDisabled, textLeft);
    final field = _fieldInputs == inputs ? _field! : _field = TextField(
      controller: _controller,
      focusNode: _focus,
      enabled: widget.enabled,
      maxLines: 1,
      style: style,
      decoration: InputDecoration(
        isCollapsed: true,
        filled: false,
        border: InputBorder.none,
        enabledBorder: InputBorder.none,
        focusedBorder: InputBorder.none,
        disabledBorder: InputBorder.none,
        hintText: widget.hint,
        hintStyle: style?.copyWith(color: t.textDisabled),
        contentPadding: EdgeInsets.only(left: textLeft, right: 4),
      ),
      onSubmitted: (_) {
        _commit();
        _focus.unfocus();
      },
    );
    _fieldInputs = inputs;
    final edge = !widget.enabled
        ? t.line
        : _focus.hasFocus
        ? t.accent
        : _hover
        ? t.textDisabled
        : t.lineStrong;
    return CallbackShortcuts(
      bindings: {const SingleActivator(LogicalKeyboardKey.escape): _cancel},
      child: MouseRegion(
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: Container(
          height: t.fieldHeight,
          decoration: BoxDecoration(
            color: t.field,
            border: Border.all(color: edge),
            borderRadius: t.cornerControl,
          ),
          child: Row(
            children: [
              ?widget.leading,
              Expanded(
                child: widget.unit == null || widget.unit!.isEmpty
                    ? Center(child: field)
                    : Stack(
                        alignment: Alignment.centerLeft,
                        children: [
                          Center(child: field),
                          Positioned.fill(
                            child: IgnorePointer(
                              child: _UnitAfterText(
                                controller: _controller,
                                style: style,
                                unit: widget.unit!,
                                left: textLeft,
                              ),
                            ),
                          ),
                        ],
                      ),
              ),
              ?widget.trailing,
            ],
          ),
        ),
      ),
    );
  }
}

/// The unit drawn right after the value (`20 m`, `45°`) — measured, so it
/// follows the number instead of sitting at the far end of the field, and
/// left out when the value would run into it.
class _UnitAfterText extends StatelessWidget {
  const _UnitAfterText({required this.controller, required this.style, required this.unit, required this.left});

  final TextEditingController controller;
  final TextStyle? style;
  final String unit;
  final double left;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final scaler = MediaQuery.textScalerOf(context);
    final unitStyle = style?.copyWith(color: t.textDisabled);
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        if (controller.text.isEmpty) return const SizedBox.shrink();
        final value = TextPainter(
          text: TextSpan(text: controller.text, style: style),
          textDirection: TextDirection.ltr,
          textScaler: scaler,
          maxLines: 1,
        )..layout();
        final u = TextPainter(
          text: TextSpan(text: unit, style: unitStyle),
          textDirection: TextDirection.ltr,
          textScaler: scaler,
          maxLines: 1,
        )..layout();
        final x = left + value.width + (unit == '°' ? 0.5 : 3);
        final unitWidth = u.width;
        value.dispose();
        u.dispose();
        return LayoutBuilder(
          builder: (context, c) {
            if (x + unitWidth + 2 > c.maxWidth) return const SizedBox.shrink();
            return Padding(
              padding: EdgeInsets.only(left: x),
              child: Align(
                alignment: Alignment.centerLeft,
                child: Text(unit, key: const ValueKey('number.unit'), style: unitStyle, maxLines: 1),
              ),
            );
          },
        );
      },
    );
  }
}

// ── Numbers ────────────────────────────────────────────────────────────────

/// A number: text entry (plain numbers or `1 + 2 * 3`), a drag on the
/// label, bounds, a step, a unit; `angle` fields are shown in degrees and
/// stored in radians; `slider` fields add a slider. `integer` writes ints.
class NumberField extends StatefulWidget {
  const NumberField({
    super.key,
    required this.value,
    required this.onCommit,
    this.mixed = false,
    this.previous,
    this.min,
    this.max,
    this.step,
    this.unit,
    this.angle = false,
    this.slider = false,
    this.integer = false,
    this.enabled = true,
    this.label,
    this.tooltip,
    this.error,
    this.overridden = false,
    this.onRevert,
    this.onApply,
    this.compact = false,
    this.onDragState,
    this.onPreview,
    this.live,
  });

  /// The committed value (null: unset / nullable).
  final num? value;

  /// [value] as it moves between builds (a running game's, during Play):
  /// the text follows it without a rebuild. Takes over from [value].
  final ValueListenable<num?>? live;
  final bool mixed;

  /// The value the document holds (for int-ness); defaults to [value].
  final Object? previous;
  final void Function(Object value) onCommit;
  final num? min;
  final num? max;
  final num? step;
  final String? unit;
  final bool angle;
  final bool slider;
  final bool integer;
  final bool enabled;

  /// When set, the widget draws its own [FieldRow]; otherwise the bare control.
  final String? label;
  final String? tooltip;
  final String? error;

  /// The row's Prefab-mode marks (see [FieldRow.overridden]).
  final bool overridden;
  final VoidCallback? onRevert;
  final VoidCallback? onApply;

  /// A vector component: narrow, with the axis letter as the drag handle.
  final bool compact;
  final void Function(bool dragging)? onDragState;

  /// While the label is dragged: the value it would commit (stored form),
  /// for the Scene view to follow before the release; null when the drag
  /// ends, just before the commit.
  final void Function(double? value)? onPreview;

  @override
  State<NumberField> createState() => _NumberFieldState();
}

class _NumberFieldState extends State<NumberField> {
  double? _preview;
  double _dragStart = 0;
  double _dragAccum = 0;

  /// [NumberField.live]'s value as the text field shows it.
  final _shownLive = ValueNotifier<({String text, String display})?>(null);

  num? get _value => widget.live != null ? widget.live!.value : widget.value;

  @override
  void initState() {
    super.initState();
    widget.live?.addListener(_onLive);
  }

  @override
  void didUpdateWidget(covariant NumberField old) {
    super.didUpdateWidget(old);
    if (old.live != widget.live) {
      old.live?.removeListener(_onLive);
      widget.live?.addListener(_onLive);
    }
  }

  @override
  void dispose() {
    widget.live?.removeListener(_onLive);
    _shownLive.dispose();
    super.dispose();
  }

  void _onLive() {
    if (!mounted || _preview != null) return; // a drag shows its own value
    if (widget.slider) {
      setState(() {}); // the slider's thumb moves too
      return;
    }
    _shownLive.value = (text: _text, display: _display);
  }

  bool get _showsDegrees => widget.angle;

  double _toShown(double stored) => _showsDegrees ? stored * 180 / math.pi : stored;
  double _toStored(double shown) => _showsDegrees ? shown * math.pi / 180 : shown;

  /// Degrees converted from a stored float carry noise (`45.000002`): shown to three decimals.
  int get _decimals => widget.angle || widget.unit == '°' ? 3 : 6;

  String get _text {
    if (_preview != null) return formatNumber(_toShown(_preview!), integer: widget.integer, decimals: _decimals);
    if (widget.mixed) return '—';
    final v = _value;
    if (v == null) return '';
    return formatNumber(_toShown(v.toDouble()), integer: widget.integer, decimals: _decimals);
  }

  /// What the field shows until it is clicked: three decimals at most.
  String get _display {
    if (widget.mixed) return '—';
    final v = _preview ?? _value?.toDouble();
    if (v == null) return '';
    return formatNumberShort(_toShown(v), integer: widget.integer);
  }

  double get _dragStep {
    final s = widget.step;
    if (s != null) return _toShown(s.toDouble());
    if (widget.integer) return 1;
    final min = widget.min, max = widget.max;
    if (min != null && max != null) return _toShown((max - min).toDouble()) / 200;
    return _showsDegrees ? 1 : 0.01;
  }

  void _commitShown(double shown) {
    var stored = _toStored(shown);
    if (widget.integer) stored = stored.roundToDouble();
    final previous = widget.live != null ? _value : widget.previous ?? widget.value;
    widget.onCommit(jsonNumber(stored, previous: previous, integer: widget.integer));
  }

  void _onText(String text) {
    final shown = parseNumber(text);
    if (shown == null) {
      // Not a number: the committed value comes back (the text field does that itself).
      return;
    }
    _commitShown(shown);
  }

  void _dragStarted(DragStartDetails d) {
    _dragStart = _toShown((_value ?? widget.min ?? 0).toDouble());
    _dragAccum = 0;
    setState(() => _preview = _value?.toDouble() ?? (widget.min ?? 0).toDouble());
    widget.onDragState?.call(true);
  }

  void _dragUpdate(DragUpdateDetails d) {
    var factor = 1.0;
    if (HardwareKeyboard.instance.isShiftPressed) factor = 10;
    if (HardwareKeyboard.instance.isAltPressed) factor = 0.1;
    _dragAccum += d.delta.dx * _dragStep * factor;
    var shown = _dragStart + _dragAccum;
    if (widget.integer) shown = shown.roundToDouble();
    final min = widget.min, max = widget.max;
    if (min != null) shown = math.max(shown, _toShown(min.toDouble()));
    if (max != null) shown = math.min(shown, _toShown(max.toDouble()));
    setState(() => _preview = _toStored(shown));
    widget.onPreview?.call(_preview);
  }

  void _dragEnded() {
    final p = _preview;
    setState(() => _preview = null);
    widget.onDragState?.call(false);
    widget.onPreview?.call(null);
    if (p == null) return;
    final before = _value?.toDouble();
    if (before != null && (p - before).abs() < 1e-12) return;
    _commitShown(_toShown(p));
  }

  Widget _dragSurface(Widget child) => MouseRegion(
    cursor: SystemMouseCursors.resizeLeftRight,
    child: GestureDetector(
      behavior: HitTestBehavior.opaque,
      dragStartBehavior: DragStartBehavior.down,
      onHorizontalDragStart: widget.enabled ? _dragStarted : null,
      onHorizontalDragUpdate: widget.enabled ? _dragUpdate : null,
      onHorizontalDragEnd: widget.enabled ? (_) => _dragEnded() : null,
      onHorizontalDragCancel: widget.enabled ? _dragEnded : null,
      child: child,
    ),
  );

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final unit = widget.unit ?? (widget.angle ? '°' : null);
    // A compact (per-axis) field is too narrow for "20 m"; the row's label carries the unit. Degrees stay.
    final shownUnit = widget.compact && unit != '°' ? null : unit;
    // A vector component: its axis letter inside the field, in the gizmo's colour, is the drag handle.
    final letter = widget.compact ? widget.label : null;
    final text = _CommitText(
      key: const ValueKey('number.text'),
      text: _text,
      display: _display,
      live: widget.live == null ? null : _shownLive,
      enabled: widget.enabled,
      unit: shownUnit,
      leading: letter == null
          ? null
          : _dragSurface(
              Padding(
                key: const ValueKey('number.axis'),
                padding: const EdgeInsets.only(left: 6),
                child: SizedBox(
                  width: 9,
                  child: Text(
                    letter,
                    style: TextStyle(
                      fontFamily: uiFontFamily,
                      fontSize: 10,
                      fontWeight: FontWeight.w700,
                      color: switch (letter) {
                        'X' => t.axisX,
                        'Y' => t.axisY,
                        'Z' => t.axisZ,
                        _ => t.textSecondary,
                      },
                    ),
                  ),
                ),
              ),
            ),
      onCommit: _onText,
    );
    Widget control = text;
    final min = widget.min, max = widget.max;
    if (widget.slider && min != null && max != null) {
      final current = (_preview ?? _value ?? min).toDouble().clamp(min.toDouble(), max.toDouble());
      control = Row(
        children: [
          Expanded(
            flex: 3,
            child: SliderTheme(
              data: SliderTheme.of(context)
                  .copyWith(trackHeight: 2, thumbShape: const RoundSliderThumbShape(enabledThumbRadius: 6)),
              child: Slider(
                min: min.toDouble(),
                max: max.toDouble(),
                value: current,
                onChangeStart: widget.enabled ? (_) => widget.onDragState?.call(true) : null,
                onChanged: widget.enabled
                    ? (v) => setState(() => _preview = widget.integer ? v.roundToDouble() : v)
                    : null,
                onChangeEnd: widget.enabled ? (_) => _dragEnded() : null,
              ),
            ),
          ),
          const SizedBox(width: 6),
          SizedBox(width: 72, child: text),
        ],
      );
    }
    if (widget.compact || widget.label == null) return control;
    return FieldRow(
      label: widget.label!,
      tooltip: widget.tooltip,
      error: widget.error,
      overridden: widget.overridden,
      onRevert: widget.onRevert,
      onApply: widget.onApply,
      dragLabel: _dragSurface,
      child: control,
    );
  }
}

/// `1`, `0.5`, `12.3457` — the text a number shows (6 significant decimals).
String formatNumber(double v, {bool integer = false, int decimals = 6}) {
  if (integer) return v.round().toString();
  if (v == v.roundToDouble() && v.abs() < 1e15) return v.round().toString();
  var s = v.toStringAsFixed(decimals);
  if (s.contains('.')) s = s.replaceFirst(RegExp(r'0+$'), '').replaceFirst(RegExp(r'\.$'), '');
  return s == '-0' ? '0' : s;
}

/// The number a field shows while it is not being edited (item 9b): three
/// decimals at most, so it fits a narrow field; a value too small for
/// that keeps three significant digits rather than showing as `0`.
String formatNumberShort(double v, {bool integer = false}) {
  if (integer) return v.round().toString();
  final s = formatNumber(v, decimals: 3);
  if (s != '0' || v == 0) return s;
  var p = v.toStringAsPrecision(3);
  if (p.contains('.') && !p.contains('e')) p = p.replaceFirst(RegExp(r'0+$'), '').replaceFirst(RegExp(r'\.$'), '');
  return p;
}

// ── Boolean ────────────────────────────────────────────────────────────────

class BoolField extends StatelessWidget {
  const BoolField({super.key, required this.value, required this.onCommit, this.mixed = false, this.enabled = true});

  final bool? value;
  final bool mixed;
  final bool enabled;
  final void Function(bool value) onCommit;

  @override
  Widget build(BuildContext context) => Align(
    alignment: Alignment.centerLeft,
    child: SizedBox(
      height: _rowHeight(context),
      child: Checkbox(
        tristate: mixed,
        value: mixed ? null : (value ?? false),
        onChanged: enabled ? (v) => onCommit(mixed ? true : (v ?? false)) : null,
        materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
        visualDensity: VisualDensity.compact,
      ),
    ),
  );
}

// ── String ─────────────────────────────────────────────────────────────────

class StringField extends StatelessWidget {
  const StringField({
    super.key,
    required this.value,
    required this.onCommit,
    this.mixed = false,
    this.multiline = false,
    this.enabled = true,
    this.hint,
  });

  final String? value;
  final bool mixed;
  final bool multiline;
  final bool enabled;
  final String? hint;
  final void Function(String value) onCommit;

  @override
  Widget build(BuildContext context) => _CommitText(
    text: mixed ? '—' : (value ?? ''),
    hint: hint ?? '—',
    multiline: multiline,
    enabled: enabled,
    onCommit: onCommit,
  );
}

/// The Inspector's field box around something that is not a text field
/// (a menu, a mask, an asset slot): the same fill, hairline and height.
class FieldBox extends StatelessWidget {
  const FieldBox({super.key, required this.child, this.edge, this.padding = const EdgeInsets.only(left: 6)});

  final Widget child;
  final Color? edge;
  final EdgeInsetsGeometry padding;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return Container(
      height: t.fieldHeight,
      padding: padding,
      decoration: BoxDecoration(
        color: t.field,
        border: Border.all(color: edge ?? t.lineStrong),
        borderRadius: t.cornerControl,
      ),
      child: child,
    );
  }
}

/// A value inside a [FieldBox]: the text, or `—` dim when there is none.
Widget _boxText(BuildContext context, String text, {Key? key, String empty = '—'}) {
  final t = context.tokens;
  return Text(
    text.isEmpty ? empty : text,
    key: key,
    maxLines: 1,
    overflow: TextOverflow.ellipsis,
    style: Theme.of(context).textTheme.bodySmall?.copyWith(color: text.isEmpty ? t.textDisabled : t.text),
  );
}

// ── Enum ───────────────────────────────────────────────────────────────────

class EnumField extends StatelessWidget {
  const EnumField({
    super.key,
    required this.value,
    required this.values,
    required this.onCommit,
    this.mixed = false,
    this.enabled = true,
  });

  final String? value;
  final List<String> values;
  final bool mixed;
  final bool enabled;
  final void Function(String value) onCommit;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final shown = mixed ? '—' : (value ?? '');
    return SizedBox(
      height: context.tokens.fieldHeight,
      child: PopupMenuButton<String>(
        popUpAnimationStyle: AnimationStyle.noAnimation,
        enabled: enabled,
        tooltip: '',
        onSelected: onCommit,
        itemBuilder: (_) => [
          for (final v in values)
            PopupMenuItem(
              value: v,
              height: 28,
              child: Row(
                children: [
                  SizedBox(width: 18, child: v == value && !mixed ? const Icon(FIcons.check, size: 14) : null),
                  Text(humanize(v), style: theme.textTheme.bodySmall),
                ],
              ),
            ),
        ],
        child: FieldBox(
          padding: const EdgeInsets.only(left: 6, right: 4),
          child: Row(
            children: [
              Expanded(child: _boxText(context, shown.isEmpty || mixed ? '' : humanize(shown))),
              Icon(FIcons.chevronDown, size: 12, color: context.tokens.textDisabled),
            ],
          ),
        ),
      ),
    );
  }
}

// ── Vectors ────────────────────────────────────────────────────────────────

const _axisNames = ['X', 'Y', 'Z', 'W'];

/// `vec2`/`vec3`/`vec4`: one [NumberField] per component; each component
/// commits alone (`onCommitComponent(i, v)`) so a drag on X is one step.
class VecField extends StatelessWidget {
  const VecField({
    super.key,
    required this.value,
    required this.components,
    required this.onCommitComponent,
    this.mixed = const [],
    this.enabled = true,
    this.unit,
    this.step,
    this.min,
    this.max,
    this.integer = false,
    this.names = _axisNames,
    this.live,
    this.onPreviewComponent,
  });

  final List<num>? value;

  /// [value] as it moves between builds (see [NumberField.live]).
  final ValueListenable<Object?>? live;
  final int components;

  /// Per component: the selection disagrees.
  final List<bool> mixed;
  final bool enabled;
  final String? unit;
  final num? step;
  final num? min;
  final num? max;
  final bool integer;
  final List<String> names;
  final void Function(int index, Object value) onCommitComponent;

  /// A component's label drag (see [NumberField.onPreview]).
  final void Function(int index, double? value)? onPreviewComponent;

  @override
  Widget build(BuildContext context) => Row(
    children: [
      for (var i = 0; i < components; i++) ...[
        if (i > 0) const SizedBox(width: 4),
        Expanded(
          child: NumberField(
            key: ValueKey('vec.$i'),
            compact: true,
            label: i < names.length ? names[i] : '$i',
            value: value != null && i < value!.length ? value![i] : null,
            previous: value != null && i < value!.length ? value![i] : null,
            mixed: i < mixed.length && mixed[i],
            enabled: enabled,
            unit: unit,
            step: step,
            min: min,
            max: max,
            integer: integer,
            live: live == null ? null : _Component(live!, i),
            onCommit: (v) => onCommitComponent(i, v),
            onPreview: onPreviewComponent == null ? null : (v) => onPreviewComponent!(i, v),
          ),
        ),
      ],
    ],
  );
}

/// Component [index] of a vector [source], as a listenable (equal for the
/// same source and index, so a rebuild does not resubscribe).
final class _Component implements ValueListenable<num?> {
  const _Component(this.source, this.index);

  final ValueListenable<Object?> source;
  final int index;

  @override
  num? get value {
    final v = source.value;
    return v is List && index < v.length && v[index] is num ? v[index] as num : null;
  }

  @override
  void addListener(VoidCallback listener) => source.addListener(listener);
  @override
  void removeListener(VoidCallback listener) => source.removeListener(listener);

  @override
  bool operator ==(Object other) => other is _Component && identical(other.source, source) && other.index == index;
  @override
  int get hashCode => Object.hash(identityHashCode(source), index);
}

/// A unit quaternion `[x, y, z, w]` edited as Euler degrees. Each axis
/// commits the whole quaternion (one `setField`); a drag on one axis is one
/// step. The displayed angles come from the stored quaternion, so what you
/// see is what is saved.
class EulerField extends StatefulWidget {
  const EulerField({
    super.key,
    required this.quaternion,
    required this.onCommit,
    this.mixed = false,
    this.mixedAxes,
    this.onCommitAxis,
    this.onPreviewAxis,
    this.enabled = true,
    this.live,
    this.hintKey,
  });

  final List<num>? quaternion;

  /// Where the angles typed here are remembered ([EulerHints]); null: not.
  final String? hintKey;

  /// Several objects: which of X, Y, Z differ among them (each shows "—"
  /// alone); with [onCommitAxis], an edit changes only that axis on each.
  final List<bool>? mixedAxes;

  /// Several objects: the edited axis and its degrees, for each object to
  /// apply to its own angles (Unity changes that axis only).
  final void Function(int axis, double degrees)? onCommitAxis;

  /// An axis's label drag: the degrees it would commit, null at the end.
  final void Function(int axis, double? degrees)? onPreviewAxis;

  /// [quaternion] as it moves between builds (see [NumberField.live]).
  final ValueListenable<Object?>? live;
  final bool mixed;
  final bool enabled;
  final void Function(List<double> quaternion) onCommit;

  @override
  State<EulerField> createState() => _EulerFieldState();
}

class _EulerFieldState extends State<EulerField> {
  /// The angles as last shown; kept across a commit so a drag on Y does not
  /// snap X when the quaternion's decomposition prefers another triple.
  Euler? _shown;
  List<num>? _for;

  /// The angles [EulerField.live] moved to, per axis.
  final _axes = [for (var i = 0; i < 3; i++) ValueNotifier<num?>(null)];

  List<num>? get _quaternion {
    final live = widget.live;
    if (live == null) return widget.quaternion;
    final q = live.value;
    return q is List && q.length == 4 ? [for (final v in q) v is num ? v : 0] : null;
  }

  @override
  void initState() {
    super.initState();
    widget.live?.addListener(_onLive);
    _setAxes();
  }

  @override
  void didUpdateWidget(covariant EulerField old) {
    super.didUpdateWidget(old);
    if (!identical(old.live, widget.live)) {
      old.live?.removeListener(_onLive);
      widget.live?.addListener(_onLive);
    }
    _setAxes();
  }

  @override
  void dispose() {
    widget.live?.removeListener(_onLive);
    for (final a in _axes) {
      a.dispose();
    }
    super.dispose();
  }

  void _onLive() {
    if (mounted) _setAxes();
  }

  void _setAxes() {
    if (widget.live == null) return;
    final e = _euler;
    for (var i = 0; i < 3; i++) {
      _axes[i].value = widget.mixed ? null : e[i];
    }
  }

  Euler get _euler {
    final q = _quaternion;
    if (q == null || q.length != 4) return Euler.zero;
    if (_shown != null && _for != null && sameRotation(_for!, q, eps: 1e-9)) return _shown!;
    final e = EulerHints.of(widget.hintKey, q);
    _shown = e;
    _for = List.of(q);
    return e;
  }

  void _commit(int i, Object v) {
    if (widget.onCommitAxis != null) {
      widget.onCommitAxis!(i, (v as num).toDouble());
      return;
    }
    final e = _euler.withComponent(i, (v as num).toDouble());
    final q = e.toQuaternion();
    _shown = e;
    _for = q;
    EulerHints.remember(widget.hintKey, q, e);
    widget.onCommit(q);
  }

  bool _mixed(int i) => widget.mixedAxes?[i] ?? widget.mixed;

  @override
  Widget build(BuildContext context) {
    final e = _euler;
    return Row(
      children: [
        for (var i = 0; i < 3; i++) ...[
          if (i > 0) const SizedBox(width: 4),
          Expanded(
            child: NumberField(
              key: ValueKey('euler.$i'),
              compact: true,
              label: _axisNames[i],
              value: _mixed(i) ? null : e[i],
              previous: 0.5, // degrees are shown as decimals; the quaternion decides the stored form
              mixed: _mixed(i),
              enabled: widget.enabled,
              unit: '°',
              step: 1,
              live: widget.live == null ? null : _axes[i],
              onCommit: (v) => _commit(i, v),
              onPreview: widget.onPreviewAxis == null ? null : (v) => widget.onPreviewAxis!(i, v),
            ),
          ),
        ],
      ],
    );
  }
}

// ── Colour ─────────────────────────────────────────────────────────────────

/// A linear RGB(A) colour: its hex in a field with the swatch inside, at
/// the start; the swatch opens the colour picker.
class ColorField extends StatelessWidget {
  const ColorField({
    super.key,
    required this.value,
    required this.onCommit,
    this.mixed = false,
    this.enabled = true,
    this.components = 3,
  });

  final List<num>? value;
  final bool mixed;
  final bool enabled;
  final int components;
  final void Function(List<double> rgb) onCommit;

  Color get _color {
    final v = value;
    if (v == null || v.length < 3) return Colors.transparent;
    return Color.fromARGB(
      v.length > 3 ? (v[3].toDouble().clamp(0.0, 1.0) * 255).round() : 255,
      (linearToSrgb(v[0].toDouble()) * 255).round(),
      (linearToSrgb(v[1].toDouble()) * 255).round(),
      (linearToSrgb(v[2].toDouble()) * 255).round(),
    );
  }

  Future<void> _pick(BuildContext context) async {
    final start = value == null || value!.length < 3
        ? List<double>.filled(components, 1.0)
        : [for (final v in value!) v.toDouble()];
    final picked = await showDialog<List<double>>(
      animationStyle: AnimationStyle.noAnimation,
      context: context,
      builder: (_) => ColorPickerDialog(initial: start, alpha: components == 4),
    );
    if (picked != null) onCommit(picked);
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return _CommitText(
      key: const ValueKey('color.hex'),
      text: mixed || value == null || value!.length < 3 ? '—' : linearRgbToHex(value!),
      enabled: enabled,
      monospace: true,
      // The swatch sits inside the field, at its start; a click on it opens the picker.
      leading: Padding(
        padding: const EdgeInsets.only(left: 3),
        child: MouseRegion(
          cursor: enabled ? SystemMouseCursors.click : MouseCursor.defer,
          child: GestureDetector(
            key: const ValueKey('color.swatch'),
            onTap: enabled ? () => _pick(context) : null,
            child: Container(
              width: 26,
              height: t.fieldHeight - 8,
              decoration: BoxDecoration(
                color: mixed ? null : _color,
                gradient: mixed ? LinearGradient(colors: [t.textDisabled, t.field]) : null,
                border: Border.all(color: t.lineStrong),
                borderRadius: BorderRadius.circular(2),
              ),
            ),
          ),
        ),
      ),
      onCommit: (text) {
        final rgb = hexToLinearRgb(text);
        if (rgb == null) return;
        if (components == 4 && rgb.length == 3) {
          rgb.add(value != null && value!.length > 3 ? value![3].toDouble() : 1.0);
        }
        if (components == 3 && rgb.length == 4) rgb.removeLast();
        onCommit(rgb);
      },
    );
  }
}

/// HSV picker over sRGB with a hex field; the result is linear RGB(A).
class ColorPickerDialog extends StatefulWidget {
  const ColorPickerDialog({super.key, required this.initial, this.alpha = false});

  final List<double> initial;
  final bool alpha;

  @override
  State<ColorPickerDialog> createState() => _ColorPickerDialogState();
}

class _ColorPickerDialogState extends State<ColorPickerDialog> {
  late double _h, _s, _v, _a;

  @override
  void initState() {
    super.initState();
    final (h, s, v) = rgbToHsv(
      linearToSrgb(widget.initial[0]),
      linearToSrgb(widget.initial[1]),
      linearToSrgb(widget.initial[2]),
    );
    _h = h;
    _s = s;
    _v = v;
    _a = widget.initial.length > 3 ? widget.initial[3] : 1;
  }

  List<double> get _linear {
    final (r, g, b) = hsvToRgb(_h, _s, _v);
    return [srgbToLinear(r), srgbToLinear(g), srgbToLinear(b), if (widget.alpha) _a];
  }

  Color get _color {
    final (r, g, b) = hsvToRgb(_h, _s, _v);
    return Color.fromARGB(255, (r * 255).round(), (g * 255).round(), (b * 255).round());
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return AlertDialog(
      title: const Text('Colour'),
      content: SizedBox(
        width: 300,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            // Saturation / value square.
            SizedBox(
              height: 160,
              child: LayoutBuilder(
                builder: (context, c) => GestureDetector(
                  onPanDown: (d) => _svAt(d.localPosition, c.biggest),
                  onPanUpdate: (d) => _svAt(d.localPosition, c.biggest),
                  child: CustomPaint(painter: _SvPainter(_h, _s, _v), size: c.biggest),
                ),
              ),
            ),
            const SizedBox(height: 8),
            Row(
              children: [
                const SizedBox(width: 24, child: Text('H')),
                Expanded(
                  child: Slider(min: 0, max: 360, value: _h, onChanged: (v) => setState(() => _h = v)),
                ),
              ],
            ),
            if (widget.alpha)
              Row(
                children: [
                  const SizedBox(width: 24, child: Text('A')),
                  Expanded(
                    child: Slider(min: 0, max: 1, value: _a, onChanged: (v) => setState(() => _a = v)),
                  ),
                ],
              ),
            Row(
              children: [
                Container(
                  width: 40,
                  height: 24,
                  decoration: BoxDecoration(
                    color: _color,
                    border: Border.all(color: theme.dividerColor),
                    borderRadius: BorderRadius.circular(3),
                  ),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: _CommitText(
                    key: const ValueKey('picker.hex'),
                    text: linearRgbToHex(_linear),
                    monospace: true,
                    onCommit: (t) {
                      final rgb = hexToLinearRgb(t);
                      if (rgb == null) return;
                      final (h, s, v) = rgbToHsv(linearToSrgb(rgb[0]), linearToSrgb(rgb[1]), linearToSrgb(rgb[2]));
                      setState(() {
                        _h = h;
                        _s = s;
                        _v = v;
                        if (rgb.length > 3) _a = rgb[3];
                      });
                    },
                  ),
                ),
              ],
            ),
            const SizedBox(height: 4),
            Text(
              'Linear ${_linear.map((c) => c.toStringAsFixed(3)).join(', ')}',
              style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: const Text('Cancel')),
        FilledButton(onPressed: () => Navigator.of(context).pop(_linear), child: const Text('OK')),
      ],
    );
  }

  void _svAt(Offset p, Size size) => setState(() {
    _s = (p.dx / size.width).clamp(0.0, 1.0);
    _v = 1 - (p.dy / size.height).clamp(0.0, 1.0);
  });
}

class _SvPainter extends CustomPainter {
  _SvPainter(this.h, this.s, this.v);
  final double h, s, v;

  @override
  void paint(Canvas canvas, Size size) {
    final rect = Offset.zero & size;
    final (r, g, b) = hsvToRgb(h, 1, 1);
    final hue = Color.fromARGB(255, (r * 255).round(), (g * 255).round(), (b * 255).round());
    canvas.drawRect(rect, Paint()..shader = LinearGradient(colors: [Colors.white, hue]).createShader(rect));
    canvas.drawRect(
      rect,
      Paint()
        ..shader = const LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [Colors.transparent, Colors.black],
        ).createShader(rect),
    );
    final c = Offset(s * size.width, (1 - v) * size.height);
    canvas.drawCircle(
      c,
      6,
      Paint()
        ..color = Colors.white
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2,
    );
    canvas.drawCircle(
      c,
      7,
      Paint()
        ..color = Colors.black54
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1,
    );
  }

  @override
  bool shouldRepaint(_SvPainter old) => old.h != h || old.s != s || old.v != v;
}

// ── Asset reference ────────────────────────────────────────────────────────

/// `{ asset, sub? }` as a slot (item 9b/9d): the asset's picture or icon,
/// its name with the part dim after it (`robot › mesh/0`), **None** dim when
/// empty, and inside the box a clear button (a nullable field) and ⊙,
/// which opens [showPicker] with the assets of the field's kind grouped by
/// folder and by source file. A drag from the Project browser drops in. An
/// unknown asset is refused by the model's precheck (`FYN-A0004`) before
/// any command is sent.
class AssetField extends StatefulWidget {
  const AssetField({
    super.key,
    required this.value,
    required this.describe,
    required this.candidates,
    required this.onCommit,
    this.kind,
    this.subject,
    this.thumbnail,
    this.mixed = false,
    this.nullable = false,
    this.enabled = true,
    this.onPing,
  });

  final Map<String, Object?>? value;
  final String Function(Map<String, Object?>? ref) describe;

  /// A click on the asset's name: Unity's ping (the asset outlined in the
  /// Project browser). Without it — or with nothing assigned — the click
  /// opens the picker, as the ⊙ button does.
  final void Function(String assetId)? onPing;

  /// The assets the picker lists (already filtered by [kind]).
  final List<AssetEntry> Function() candidates;
  final String? kind;

  /// What the picker's title names (the field's label).
  final String? subject;

  /// An asset's picture at a size (a texture's pixels, a material's
  /// swatch); the kind's icon when null.
  final Widget Function(AssetEntry entry, double size)? thumbnail;
  final bool mixed;
  final bool nullable;
  final bool enabled;
  final void Function(Map<String, Object?>? ref) onCommit;

  @override
  State<AssetField> createState() => _AssetFieldState();
}

/// `id` or `id#sub`: a picker row's value (a map would not compare equal).
String _refKey(Map<String, Object?>? ref) {
  if (ref == null || ref['asset'] == null) return '';
  final sub = ref['sub'] as String?;
  return sub == null || sub.isEmpty ? '${ref['asset']}' : '${ref['asset']}#$sub';
}

Map<String, Object?> _refOf(String key) {
  final at = key.indexOf('#');
  return at < 0 ? {'asset': key} : {'asset': key.substring(0, at), 'sub': key.substring(at + 1)};
}

String _folderOf(String path) {
  final at = path.lastIndexOf('/');
  return at < 0 ? '' : path.substring(0, at);
}

/// The picker's rows for an `asset(<kind>)` field: documents and whole
/// sources under their folder, a source's parts under the source file.
List<PickerItem<String>> assetPickerItems(
  List<AssetEntry> entries, {
  String? kind,
  Widget Function(AssetEntry entry, double size)? thumbnail,
}) {
  final byFolder = <String, List<PickerItem<String>>>{};
  final bySource = <AssetEntry, List<PickerItem<String>>>{};
  List<(String, String)> facts(AssetEntry a, String k, [String? sub]) => [
    ('Kind', k),
    if (sub != null) ('Part', sub),
    ('File', a.path),
    if (a.importer != null) ('Importer', a.importer!),
  ];
  WidgetBuilder? picture(AssetEntry a) {
    final th = thumbnail;
    return th == null ? null : (context) => th(a, 124);
  }

  for (final a in entries) {
    final folder = _folderOf(a.path);
    if (!a.isSource) {
      if (kind != null && a.document != kind) continue;
      final k = a.document ?? 'document';
      byFolder
          .putIfAbsent(folder, () => [])
          .add(
            PickerItem(
              value: a.id,
              title: a.name,
              icon: assetKindIcon(k),
              group: folder.isEmpty ? 'assets' : folder,
              keywords: '${a.path} ${a.id}',
              facts: facts(a, k),
              preview: picture(a),
            ),
          );
      continue;
    }
    final whole = a.wholeKind;
    if (whole != null && (kind == null || whole == kind)) {
      byFolder
          .putIfAbsent(folder, () => [])
          .add(
            PickerItem(
              value: a.id,
              title: a.name,
              icon: assetKindIcon(whole),
              group: folder.isEmpty ? 'assets' : folder,
              keywords: '${a.path} ${a.id}',
              facts: facts(a, whole),
              preview: picture(a),
            ),
          );
    }
    for (final sub in a.subAssets.keys) {
      final k = AssetEntry.subKind(sub);
      if (kind != null && k != kind) continue;
      bySource
          .putIfAbsent(a, () => [])
          .add(
            PickerItem(
              value: '${a.id}#$sub',
              title: sub,
              icon: assetKindIcon(k),
              group: a.name,
              groupDetail: a.path,
              keywords: '${a.name} ${a.path} ${a.id}',
              facts: facts(a, k, sub),
            ),
          );
    }
  }
  int byName(String a, String b) => a.toLowerCase().compareTo(b.toLowerCase());
  final groups = <(String, List<PickerItem<String>>)>[
    for (final e in byFolder.entries) (e.key, e.value..sort((a, b) => byName(a.title, b.title))),
    for (final e in bySource.entries) ('${e.key.path}\u0000', e.value),
  ]..sort((a, b) => byName(a.$1, b.$1));
  return [for (final g in groups) ...g.$2];
}

class _AssetFieldState extends State<AssetField> {
  Future<void> _pick() async {
    final kind = widget.kind;
    final choice = await showPicker<String>(
      context,
      title: kind == null ? 'Choose an asset' : 'Choose a $kind',
      subject: widget.subject,
      anchor: globalRectOf(context),
      items: assetPickerItems(widget.candidates(), kind: kind, thumbnail: widget.thumbnail),
      allowNone: widget.nullable,
      current: _refKey(widget.value).isEmpty ? null : _refKey(widget.value),
      hint: 'Search by name or path',
      keyPrefix: 'assetPicker',
      countNoun: kind == null ? 'assets' : '${kind}s',
      empty: 'No ${kind ?? 'asset'} matches.',
    );
    if (!mounted || choice == null) return;
    widget.onCommit(choice.isNone ? null : _refOf(choice.value!));
  }

  AssetEntry? _entry() {
    final id = widget.value?['asset'];
    if (id == null || widget.thumbnail == null) return null;
    for (final a in widget.candidates()) {
      if (a.id == id) return a;
    }
    return null;
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final style = theme.textTheme.bodySmall;
    final value = widget.value;
    final face = widget.mixed ? '—' : widget.describe(value);
    final split = face.indexOf(' › ');
    final name = split < 0 ? face : face.substring(0, split);
    final part = split < 0 ? null : face.substring(split);
    final empty = value == null && !widget.mixed;
    final entry = _entry();
    return DragTarget<AssetDragPayload>(
      onWillAcceptWithDetails: (d) => widget.enabled && (widget.kind == null || d.data.kind == widget.kind),
      onAcceptWithDetails: (d) => widget.onCommit(d.data.toRef()),
      builder: (context, candidatesIn, rejected) {
        final edge = candidatesIn.isNotEmpty ? t.accent : (rejected.isNotEmpty ? t.error : null);
        return FieldBox(
          edge: edge,
          padding: const EdgeInsets.only(left: 5, right: 1),
          child: Row(
            children: [
              if (entry != null && !empty)
                SizedBox.square(dimension: 14, child: widget.thumbnail!(entry, 14))
              else
                Icon(assetKindIcon(widget.kind), size: 13, color: empty ? t.textDisabled : t.textSecondary),
              const SizedBox(width: 5),
              Expanded(
                child: MouseRegion(
                  cursor: widget.enabled ? SystemMouseCursors.click : MouseCursor.defer,
                  child: GestureDetector(
                    behavior: HitTestBehavior.opaque,
                    onTap: switch ((widget.onPing, value?['asset'], widget.mixed)) {
                      (final ping?, final String id, false) => () => ping(id),
                      _ => widget.enabled ? _pick : null,
                    },
                    child: Align(
                      alignment: Alignment.centerLeft,
                      child: Text.rich(
                        TextSpan(
                          children: [
                            TextSpan(text: name),
                            if (part != null)
                              TextSpan(
                                text: part,
                                style: TextStyle(color: t.textDisabled),
                              ),
                          ],
                        ),
                        key: const ValueKey('asset.name'),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: style?.copyWith(color: empty ? t.textDisabled : t.text),
                      ),
                    ),
                  ),
                ),
              ),
              if (widget.nullable && value != null && widget.enabled)
                _InBoxButton(tooltip: 'Clear', icon: FIcons.close, onPressed: () => widget.onCommit(null)),
              _InBoxButton(
                key: const ValueKey('asset.pick'),
                tooltip: 'Choose…',
                icon: FIcons.pick,
                onPressed: widget.enabled ? _pick : null,
              ),
            ],
          ),
        );
      },
    );
  }
}

/// A small button inside a field box (⊙, ×): no fill until hovered.
class _InBoxButton extends StatelessWidget {
  const _InBoxButton({super.key, required this.tooltip, required this.icon, required this.onPressed});

  final String tooltip;
  final IconData icon;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return IconButton(
      tooltip: tooltip,
      icon: Icon(icon, size: 12),
      color: t.textSecondary,
      disabledColor: t.textDisabled,
      padding: EdgeInsets.zero,
      visualDensity: VisualDensity.compact,
      constraints: BoxConstraints.tightFor(width: t.fieldHeight - 2, height: t.fieldHeight - 2),
      style: IconButton.styleFrom(shape: RoundedRectangleBorder(borderRadius: t.cornerControl)),
      onPressed: onPressed,
    );
  }
}

IconData assetKindIcon(String? kind) => switch (kind) {
  'mesh' => FIcons.viewInArOutlined,
  'material' => FIcons.paletteOutlined,
  'texture' => FIcons.imageOutlined,
  'prefab' => FIcons.widgetsOutlined,
  'scene' => FIcons.landscapeOutlined,
  'audio' || 'sound' => FIcons.audiotrack,
  'clip' => FIcons.animation,
  'skin' || 'rig' => FIcons.accessibilityNew,
  'volume' => FIcons.tune,
  _ => FIcons.insertDriveFileOutlined,
};

// ── Entity reference ───────────────────────────────────────────────────────

/// An entity named by its name (what authored components store): a text
/// field with ⊙ inside, which opens [showPicker] with the document's names.
class EntityField extends StatelessWidget {
  const EntityField({
    super.key,
    required this.value,
    required this.names,
    required this.onCommit,
    this.subject,
    this.mixed = false,
    this.enabled = true,
  });

  final String? value;
  final List<String> Function() names;
  final String? subject;
  final bool mixed;
  final bool enabled;
  final void Function(String name) onCommit;

  @override
  Widget build(BuildContext context) {
    return Builder(
      builder: (context) => _CommitText(
        text: mixed ? '—' : (value ?? ''),
        hint: 'None',
        enabled: enabled,
        onCommit: onCommit,
        trailing: _InBoxButton(
          key: const ValueKey('entity.pick'),
          tooltip: 'Choose an entity',
          icon: FIcons.pick,
          onPressed: enabled
              ? () async {
                  final choice = await showPicker<String>(
                    context,
                    title: 'Choose an entity',
                    subject: subject,
                    anchor: globalRectOf(context),
                    items: [
                      for (final n in names())
                        PickerItem(value: n, id: 'entity.$n', title: n, icon: FIcons.cropSquare, facts: [('Name', n)]),
                    ],
                    current: value,
                    hint: 'Search by name',
                    keyPrefix: 'entityPicker',
                    countNoun: 'entities',
                    empty: 'No entity in this document has that name.',
                  );
                  if (choice != null && !choice.isNone) onCommit(choice.value!);
                }
              : null,
        ),
      ),
    );
  }
}

// ── Layer mask ─────────────────────────────────────────────────────────────

/// A 32-bit mask: the project's physics layer names as check rows (bits
/// past the named layers show as `Layer N`), summarised by name.
class LayerMaskField extends StatelessWidget {
  const LayerMaskField({
    super.key,
    required this.value,
    required this.layers,
    required this.onCommit,
    this.mixed = false,
    this.enabled = true,
  });

  final int? value;
  final List<String> layers;
  final bool mixed;
  final bool enabled;
  final void Function(int mask) onCommit;

  String _summary() {
    final v = value;
    if (mixed) return '—';
    if (v == null) return '';
    if (v == 0) return 'Nothing';
    if (v == 0xFFFFFFFF) return 'Everything';
    final names = <String>[];
    for (var i = 0; i < 32; i++) {
      if ((v >> i) & 1 == 1) names.add(i < layers.length ? layers[i] : 'Layer $i');
    }
    return names.join(', ');
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final v = value ?? 0;
    final rows = math.max(layers.length, _highestBit(v) + 1);
    return SizedBox(
      height: context.tokens.fieldHeight,
      child: PopupMenuButton<int>(
        popUpAnimationStyle: AnimationStyle.noAnimation,
        enabled: enabled,
        tooltip: '',
        onSelected: (bit) {
          if (bit == -1) return onCommit(0xFFFFFFFF);
          if (bit == -2) return onCommit(0);
          onCommit(v ^ (1 << bit));
        },
        itemBuilder: (_) => [
          const PopupMenuItem(value: -1, height: 26, child: Text('Everything')),
          const PopupMenuItem(value: -2, height: 26, child: Text('Nothing')),
          const PopupMenuDivider(),
          for (var i = 0; i < rows; i++)
            CheckedPopupMenuItem(
              value: i,
              height: 26,
              checked: (v >> i) & 1 == 1,
              child: Text(i < layers.length ? layers[i] : 'Layer $i', style: theme.textTheme.bodySmall),
            ),
        ],
        child: FieldBox(
          padding: const EdgeInsets.only(left: 6, right: 4),
          child: Row(
            children: [
              Expanded(child: _boxText(context, _summary(), key: const ValueKey('layerMask.summary'))),
              Icon(FIcons.chevronDown, size: 12, color: context.tokens.textDisabled),
            ],
          ),
        ),
      ),
    );
  }

  static int _highestBit(int v) {
    var b = -1;
    for (var i = 0; i < 32; i++) {
      if ((v >> i) & 1 == 1) b = i;
    }
    return b;
  }
}

// ── JSON fallback ──────────────────────────────────────────────────────────

/// Any value as JSON text; commits when the text parses.
class JsonField extends StatelessWidget {
  const JsonField({super.key, required this.value, required this.onCommit, this.mixed = false, this.enabled = true});

  final Object? value;
  final bool mixed;
  final bool enabled;
  final void Function(Object? value) onCommit;

  @override
  Widget build(BuildContext context) => _CommitText(
    text: mixed ? '—' : prettyJson(value),
    multiline: value is Map || value is List,
    monospace: true,
    enabled: enabled,
    onCommit: (t) {
      final parsed = tryParseJson(t);
      if (parsed.$1) onCommit(parsed.$2);
    },
  );
}

String prettyJson(Object? v) {
  if (v is Map || v is List) return const JsonEncoder.withIndent('  ').convert(v);
  return jsonEncode(v);
}

(bool, Object?) tryParseJson(String text) {
  try {
    return (true, jsonDecode(text));
  } catch (_) {
    return (false, null);
  }
}
