// One widget per field kind (blueprint §19.2e item 4; editor-ux §22.6):
// numbers with a label drag, bounds, step and unit (a slider when hinted),
// integers, booleans, strings (single or multiline), enum menus, vectors,
// rotation as Euler degrees over the stored quaternion, colours with a
// picker, asset references with a searchable picker and drag-in, entity
// references, layer masks, arrays, nested objects and a JSON fallback.
//
// Every widget shows the *committed* value it is given and reports an edit
// through `onCommit` exactly once per user gesture: a drag previews locally
// and commits on release (one transaction, one undo step); a text field
// commits on ↩ or on losing focus, and ⎋ puts the committed value back.
// A mixed value (the selection disagrees) is drawn as `—` until edited.
import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../project/editor_project.dart';
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

const _rowHeight = 26.0;
const _labelWidth = 0.42;

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
      style: theme.textTheme.bodySmall?.copyWith(fontWeight: overridden ? FontWeight.bold : null),
    );
    if (tooltip != null && tooltip!.isNotEmpty) {
      text = Tooltip(message: tooltip!, waitDuration: const Duration(milliseconds: 500), child: text);
    }
    if (dragLabel != null) text = dragLabel!(text);
    return Padding(
      padding: EdgeInsets.only(left: indent, top: 1, bottom: 1),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          LayoutBuilder(
            builder: (context, c) => Row(
              crossAxisAlignment: CrossAxisAlignment.center,
              children: [
                SizedBox(
                  width: math.max(60, c.maxWidth * _labelWidth - indent),
                  child: Row(
                    children: [
                      Expanded(child: text),
                      if (unit != null && unit!.isNotEmpty)
                        Padding(
                          padding: const EdgeInsets.only(left: 4, right: 2),
                          child: Text(
                            unit!,
                            key: const ValueKey('field.unit'),
                            style: theme.textTheme.labelSmall?.copyWith(color: theme.hintColor),
                          ),
                        ),
                      if (overridden && onRevert != null)
                        IconButton(
                          key: const ValueKey('field.revert'),
                          tooltip: 'Revert override — back to the prefab\'s value',
                          icon: const Icon(Icons.undo, size: 14),
                          padding: EdgeInsets.zero,
                          constraints: const BoxConstraints(minWidth: 20, minHeight: 20),
                          onPressed: onRevert,
                        ),
                      if (overridden && onApply != null)
                        IconButton(
                          key: const ValueKey('field.apply'),
                          tooltip: 'Apply to prefab — this value becomes the prefab\'s',
                          icon: const Icon(Icons.upload, size: 14),
                          padding: EdgeInsets.zero,
                          constraints: const BoxConstraints(minWidth: 20, minHeight: 20),
                          onPressed: onApply,
                        ),
                    ],
                  ),
                ),
                const SizedBox(width: 6),
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
                style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.error),
              ),
            ),
        ],
      ),
    );
  }
}

// ── Text entry shared by number and string fields ──────────────────────────

/// A compact text field that commits on ↩ / focus loss and restores on ⎋.
class _CommitText extends StatefulWidget {
  const _CommitText({
    super.key,
    required this.text,
    required this.onCommit,
    this.enabled = true,
    this.multiline = false,
    this.suffix,
    this.hint,
    this.alignRight = false,
    this.monospace = false,
  });

  final String text;
  final void Function(String text) onCommit;
  final bool enabled;
  final bool multiline;
  final String? suffix;
  final String? hint;
  final bool alignRight;
  final bool monospace;

  @override
  State<_CommitText> createState() => _CommitTextState();
}

class _CommitTextState extends State<_CommitText> {
  late final _controller = TextEditingController(text: widget.text);
  final _focus = FocusNode(debugLabel: 'inspector.text');
  bool _editing = false;

  @override
  void initState() {
    super.initState();
    _focus.addListener(_onFocus);
  }

  @override
  void didUpdateWidget(covariant _CommitText old) {
    super.didUpdateWidget(old);
    // A new committed value replaces the text unless the user is mid-edit.
    if (!_editing && widget.text != _controller.text) _controller.text = widget.text;
  }

  @override
  void dispose() {
    _focus.removeListener(_onFocus);
    _focus.dispose();
    _controller.dispose();
    super.dispose();
  }

  void _onFocus() {
    if (_focus.hasFocus) {
      _editing = true;
      _controller.selection = TextSelection(baseOffset: 0, extentOffset: _controller.text.length);
    } else if (_editing) {
      _commit();
    }
  }

  void _commit() {
    _editing = false;
    if (_controller.text != widget.text) {
      widget.onCommit(_controller.text);
    }
    // Whatever the outcome, show the committed value (a refusal keeps the old one).
    if (mounted) setState(() => _controller.text = widget.text);
  }

  void _cancel() {
    _editing = false;
    _controller.text = widget.text;
    _focus.unfocus();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return CallbackShortcuts(
      bindings: {const SingleActivator(LogicalKeyboardKey.escape): _cancel},
      child: TextField(
        controller: _controller,
        focusNode: _focus,
        enabled: widget.enabled,
        maxLines: widget.multiline ? null : 1,
        minLines: widget.multiline ? 3 : 1,
        keyboardType: widget.multiline ? TextInputType.multiline : TextInputType.text,
        textAlign: widget.alignRight ? TextAlign.right : TextAlign.left,
        style: theme.textTheme.bodySmall?.copyWith(fontFamily: widget.monospace ? 'monospace' : null),
        decoration: InputDecoration(
          isDense: true,
          hintText: widget.hint,
          suffixText: widget.suffix,
          suffixStyle: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
          contentPadding: const EdgeInsets.symmetric(horizontal: 6, vertical: 5),
          border: const OutlineInputBorder(),
        ),
        onSubmitted: (_) {
          _commit();
          if (!widget.multiline) _focus.unfocus();
        },
      ),
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
  });

  /// The committed value (null: unset / nullable).
  final num? value;
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

  @override
  State<NumberField> createState() => _NumberFieldState();
}

class _NumberFieldState extends State<NumberField> {
  double? _preview;
  double _dragStart = 0;
  double _dragAccum = 0;

  bool get _showsDegrees => widget.angle;

  double _toShown(double stored) => _showsDegrees ? stored * 180 / math.pi : stored;
  double _toStored(double shown) => _showsDegrees ? shown * math.pi / 180 : shown;

  /// Degrees converted from a stored float carry noise (`45.000002`): shown to three decimals.
  int get _decimals => widget.angle || widget.unit == '°' ? 3 : 6;

  String get _text {
    if (_preview != null) return formatNumber(_toShown(_preview!), integer: widget.integer, decimals: _decimals);
    if (widget.mixed) return '—';
    final v = widget.value;
    if (v == null) return '';
    return formatNumber(_toShown(v.toDouble()), integer: widget.integer, decimals: _decimals);
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
    widget.onCommit(jsonNumber(stored, previous: widget.previous ?? widget.value, integer: widget.integer));
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
    _dragStart = _toShown((widget.value ?? widget.min ?? 0).toDouble());
    _dragAccum = 0;
    setState(() => _preview = widget.value?.toDouble() ?? (widget.min ?? 0).toDouble());
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
  }

  void _dragEnded() {
    final p = _preview;
    setState(() => _preview = null);
    widget.onDragState?.call(false);
    if (p == null) return;
    final before = widget.value?.toDouble();
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
    final theme = Theme.of(context);
    final unit = widget.unit ?? (widget.angle ? '°' : null);
    // A compact (per-axis) field is too narrow for "20 m" plus a suffix; the row's label carries the unit.
    final suffix = widget.compact && unit != '°' ? null : unit;
    final text = _CommitText(
      key: const ValueKey('number.text'),
      text: _text,
      enabled: widget.enabled,
      alignRight: true,
      suffix: suffix,
      onCommit: _onText,
    );
    Widget control = text;
    final min = widget.min, max = widget.max;
    if (widget.slider && min != null && max != null) {
      final current = (_preview ?? widget.value ?? min).toDouble().clamp(min.toDouble(), max.toDouble());
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
          SizedBox(width: 72, child: text),
        ],
      );
    }
    if (widget.compact) {
      return Row(
        children: [
          if (widget.label != null)
            _dragSurface(
              Padding(
                padding: const EdgeInsets.only(right: 3),
                child: Text(widget.label!, style: theme.textTheme.labelSmall?.copyWith(color: theme.hintColor)),
              ),
            ),
          Expanded(child: control),
        ],
      );
    }
    if (widget.label == null) return control;
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
      height: _rowHeight,
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
    hint: hint,
    multiline: multiline,
    enabled: enabled,
    onCommit: onCommit,
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
      height: _rowHeight,
      child: PopupMenuButton<String>(
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
                  SizedBox(width: 18, child: v == value && !mixed ? const Icon(Icons.check, size: 14) : null),
                  Text(humanize(v), style: theme.textTheme.bodySmall),
                ],
              ),
            ),
        ],
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 6),
          decoration: BoxDecoration(
            border: Border.all(color: theme.dividerColor),
            borderRadius: BorderRadius.circular(4),
          ),
          child: Row(
            children: [
              Expanded(
                child: Text(
                  shown.isEmpty ? '' : humanize(shown),
                  style: theme.textTheme.bodySmall,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              Icon(Icons.arrow_drop_down, size: 16, color: theme.hintColor),
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
  });

  final List<num>? value;
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
            onCommit: (v) => onCommitComponent(i, v),
          ),
        ),
      ],
    ],
  );
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
    this.enabled = true,
  });

  final List<num>? quaternion;
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

  Euler get _euler {
    final q = widget.quaternion;
    if (q == null || q.length != 4) return Euler.zero;
    if (_shown != null && _for != null && sameRotation(_for!, q, eps: 1e-9)) return _shown!;
    final e = Euler.fromQuaternion(q).clean;
    _shown = e;
    _for = List.of(q);
    return e;
  }

  void _commit(int i, Object v) {
    final e = _euler.withComponent(i, (v as num).toDouble());
    final q = e.toQuaternion();
    _shown = e;
    _for = q;
    widget.onCommit(q);
  }

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
              value: widget.mixed ? null : e[i],
              previous: 0.5, // degrees are shown as decimals; the quaternion decides the stored form
              mixed: widget.mixed,
              enabled: widget.enabled,
              unit: '°',
              step: 1,
              onCommit: (v) => _commit(i, v),
            ),
          ),
        ],
      ],
    );
  }
}

// ── Colour ─────────────────────────────────────────────────────────────────

/// A linear RGB(A) colour: a swatch that opens the picker, the hex next to it.
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
      context: context,
      builder: (_) => ColorPickerDialog(initial: start, alpha: components == 4),
    );
    if (picked != null) onCommit(picked);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Row(
      children: [
        InkWell(
          key: const ValueKey('color.swatch'),
          onTap: enabled ? () => _pick(context) : null,
          child: Container(
            width: 36,
            height: 20,
            decoration: BoxDecoration(
              color: mixed ? null : _color,
              gradient: mixed ? LinearGradient(colors: [theme.disabledColor, theme.cardColor]) : null,
              border: Border.all(color: theme.dividerColor),
              borderRadius: BorderRadius.circular(3),
            ),
          ),
        ),
        const SizedBox(width: 6),
        Expanded(
          child: _CommitText(
            key: const ValueKey('color.hex'),
            text: mixed || value == null || value!.length < 3 ? '—' : linearRgbToHex(value!),
            enabled: enabled,
            monospace: true,
            onCommit: (t) {
              final rgb = hexToLinearRgb(t);
              if (rgb == null) return;
              if (components == 4 && rgb.length == 3) {
                rgb.add(value != null && value!.length > 3 ? value![3].toDouble() : 1.0);
              }
              if (components == 3 && rgb.length == 4) rgb.removeLast();
              onCommit(rgb);
            },
          ),
        ),
      ],
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

/// `{ asset, sub? }`: the asset's name, a picker, drag-in from the Project
/// browser, Clear when the field is nullable. An unknown asset is refused
/// by the model's precheck (`FYN-A0004`) before any command is sent.
class AssetField extends StatelessWidget {
  const AssetField({
    super.key,
    required this.value,
    required this.describe,
    required this.candidates,
    required this.onCommit,
    this.kind,
    this.mixed = false,
    this.nullable = false,
    this.enabled = true,
  });

  final Map<String, Object?>? value;
  final String Function(Map<String, Object?>? ref) describe;

  /// The assets the picker lists (already filtered by [kind]).
  final List<AssetEntry> Function() candidates;
  final String? kind;
  final bool mixed;
  final bool nullable;
  final bool enabled;
  final void Function(Map<String, Object?>? ref) onCommit;

  Future<void> _pick(BuildContext context) async {
    final picked = await showDialog<Map<String, Object?>?>(
      context: context,
      builder: (_) => AssetPickerDialog(kind: kind, entries: candidates(), current: value),
    );
    if (picked != null) onCommit(picked['asset'] == null ? null : picked);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return DragTarget<AssetDragPayload>(
      onWillAcceptWithDetails: (d) => enabled && (kind == null || d.data.kind == kind),
      onAcceptWithDetails: (d) => onCommit(d.data.toRef()),
      builder: (context, candidatesIn, rejected) {
        final hot = candidatesIn.isNotEmpty;
        return Container(
          height: _rowHeight,
          decoration: BoxDecoration(
            border: Border.all(
              color: hot
                  ? theme.colorScheme.primary
                  : (rejected.isNotEmpty ? theme.colorScheme.error : theme.dividerColor),
            ),
            borderRadius: BorderRadius.circular(4),
          ),
          child: Row(
            children: [
              const SizedBox(width: 6),
              Icon(assetKindIcon(kind), size: 14, color: theme.hintColor),
              const SizedBox(width: 4),
              Expanded(
                child: Text(
                  mixed ? '—' : describe(value),
                  key: const ValueKey('asset.name'),
                  style: theme.textTheme.bodySmall?.copyWith(color: value == null && !mixed ? theme.hintColor : null),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              if (nullable && value != null && enabled)
                IconButton(
                  tooltip: 'Clear',
                  icon: const Icon(Icons.close, size: 14),
                  padding: EdgeInsets.zero,
                  constraints: const BoxConstraints(minWidth: 22, minHeight: 22),
                  onPressed: () => onCommit(null),
                ),
              IconButton(
                key: const ValueKey('asset.pick'),
                tooltip: 'Choose…',
                icon: const Icon(Icons.more_horiz, size: 16),
                padding: EdgeInsets.zero,
                constraints: const BoxConstraints(minWidth: 22, minHeight: 22),
                onPressed: enabled ? () => _pick(context) : null,
              ),
            ],
          ),
        );
      },
    );
  }
}

IconData assetKindIcon(String? kind) => switch (kind) {
  'mesh' => Icons.view_in_ar_outlined,
  'material' => Icons.palette_outlined,
  'texture' => Icons.image_outlined,
  'prefab' => Icons.widgets_outlined,
  'scene' => Icons.landscape_outlined,
  'audio' || 'sound' => Icons.audiotrack,
  'clip' => Icons.animation,
  'skin' || 'rig' => Icons.accessibility_new,
  'volume' => Icons.tune,
  _ => Icons.insert_drive_file_outlined,
};

/// The searchable asset list; answers `{asset, sub?}`, `{asset: null}` for
/// "None", or null when cancelled.
class AssetPickerDialog extends StatefulWidget {
  const AssetPickerDialog({super.key, required this.entries, this.kind, this.current});

  final List<AssetEntry> entries;
  final String? kind;
  final Map<String, Object?>? current;

  @override
  State<AssetPickerDialog> createState() => _AssetPickerDialogState();
}

class _AssetPickerDialogState extends State<AssetPickerDialog> {
  final _query = TextEditingController();

  /// The rows: a document asset is one row; a source offers one row per matching sub-asset.
  List<(String title, String subtitle, Map<String, Object?> ref)> get _rows {
    final q = _query.text.trim().toLowerCase();
    final out = <(String, String, Map<String, Object?>)>[];
    for (final a in widget.entries) {
      if (!a.isSource) {
        if (widget.kind == null || a.document == widget.kind) out.add((a.name, a.path, {'asset': a.id}));
        continue;
      }
      // A source referenced whole (an audio clip): one row without a sub.
      if (a.wholeKind != null && (widget.kind == null || a.wholeKind == widget.kind)) {
        out.add((a.name, a.path, {'asset': a.id}));
      }
      for (final sub in a.subAssets.keys) {
        if (widget.kind != null && AssetEntry.subKind(sub) != widget.kind) continue;
        out.add(('${a.name} › $sub', a.path, {'asset': a.id, 'sub': sub}));
      }
    }
    out.retainWhere((r) => q.isEmpty || r.$1.toLowerCase().contains(q) || r.$2.toLowerCase().contains(q));
    out.sort((a, b) => a.$1.toLowerCase().compareTo(b.$1.toLowerCase()));
    return out;
  }

  @override
  void dispose() {
    _query.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final rows = _rows;
    final cur = widget.current;
    return AlertDialog(
      title: Text(widget.kind == null ? 'Choose an asset' : 'Choose a ${widget.kind}'),
      content: SizedBox(
        width: 420,
        height: 360,
        child: Column(
          children: [
            TextField(
              key: const ValueKey('assetPicker.search'),
              controller: _query,
              autofocus: true,
              decoration: const InputDecoration(
                isDense: true,
                prefixIcon: Icon(Icons.search, size: 16),
                hintText: 'Search by name or path',
              ),
              onChanged: (_) => setState(() {}),
              onSubmitted: (_) {
                if (rows.length == 1) Navigator.of(context).pop(rows.single.$3);
              },
            ),
            const SizedBox(height: 6),
            Expanded(
              child: rows.isEmpty
                  ? Center(child: Text('No ${widget.kind ?? 'asset'} matches.', style: theme.textTheme.bodySmall))
                  : ListView.builder(
                      itemCount: rows.length,
                      itemExtent: 40,
                      itemBuilder: (context, i) {
                        final (title, subtitle, ref) = rows[i];
                        final selected =
                            cur != null && cur['asset'] == ref['asset'] && (cur['sub'] ?? '') == (ref['sub'] ?? '');
                        return ListTile(
                          key: ValueKey('assetPicker.row.$i'),
                          dense: true,
                          selected: selected,
                          leading: Icon(
                            assetKindIcon(
                              widget.kind ?? (ref['sub'] != null ? AssetEntry.subKind(ref['sub'] as String) : null),
                            ),
                            size: 16,
                          ),
                          title: Text(title, style: theme.textTheme.bodySmall, overflow: TextOverflow.ellipsis),
                          subtitle: Text(
                            subtitle,
                            style: theme.textTheme.labelSmall?.copyWith(color: theme.hintColor),
                            overflow: TextOverflow.ellipsis,
                          ),
                          onTap: () => Navigator.of(context).pop(ref),
                        );
                      },
                    ),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop({'asset': null}), child: const Text('None')),
        TextButton(onPressed: () => Navigator.of(context).pop(), child: const Text('Cancel')),
      ],
    );
  }
}

// ── Entity reference ───────────────────────────────────────────────────────

/// An entity named by its name (what authored components store): a text
/// field with the document's names offered as you type.
class EntityField extends StatelessWidget {
  const EntityField({
    super.key,
    required this.value,
    required this.names,
    required this.onCommit,
    this.mixed = false,
    this.enabled = true,
  });

  final String? value;
  final List<String> Function() names;
  final bool mixed;
  final bool enabled;
  final void Function(String name) onCommit;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Row(
      children: [
        Expanded(
          child: StringField(value: value, mixed: mixed, enabled: enabled, hint: 'Entity name', onCommit: onCommit),
        ),
        SizedBox(
          height: _rowHeight,
          child: PopupMenuButton<String>(
            key: const ValueKey('entity.pick'),
            enabled: enabled,
            tooltip: 'Choose an entity',
            icon: Icon(Icons.adjust, size: 16, color: theme.hintColor),
            padding: EdgeInsets.zero,
            onSelected: onCommit,
            itemBuilder: (_) => [
              for (final n in names())
                PopupMenuItem(
                  value: n,
                  height: 26,
                  child: Text(n, style: theme.textTheme.bodySmall),
                ),
            ],
          ),
        ),
      ],
    );
  }
}

// ── Layer mask ─────────────────────────────────────────────────────────────

/// A 32-bit mask: the project's physics layer names as check rows (bits
/// past the named layers show as `Layer N`), and the number itself.
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
      height: _rowHeight,
      child: PopupMenuButton<int>(
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
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 6),
          decoration: BoxDecoration(
            border: Border.all(color: theme.dividerColor),
            borderRadius: BorderRadius.circular(4),
          ),
          child: Row(
            children: [
              Expanded(
                child: Text(
                  _summary(),
                  key: const ValueKey('layerMask.summary'),
                  style: theme.textTheme.bodySmall,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              Text(
                mixed || value == null ? '' : '$v',
                style: theme.textTheme.labelSmall?.copyWith(color: theme.hintColor),
              ),
              Icon(Icons.arrow_drop_down, size: 16, color: theme.hintColor),
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
